use std::{path::PathBuf, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
};

use crate::{
    config::types::ExecutionCapsConfig,
    infra::{EduMindError, Result},
};

#[cfg(windows)]
mod windows_job;

#[cfg(windows)]
use windows_job::WindowsJob;

/// Specification for one externally executed command under hard resource limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardedCommandSpec {
    program: String,
    args: Vec<String>,
    working_directory: Option<PathBuf>,
    timeout: Duration,
    stdout_max_bytes: usize,
    stderr_max_bytes: usize,
    memory_limit_bytes: Option<u64>,
    use_windows_job_object: bool,
}

impl GuardedCommandSpec {
    /// Creates a guarded command with conservative defaults.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_directory: None,
            timeout: Duration::from_secs(120),
            stdout_max_bytes: 1_048_576,
            stderr_max_bytes: 1_048_576,
            memory_limit_bytes: None,
            use_windows_job_object: cfg!(windows),
        }
    }

    /// Applies the configured process and output caps.
    pub fn from_execution_caps(
        program: impl Into<String>,
        caps: &ExecutionCapsConfig,
    ) -> Result<Self> {
        let memory_limit_bytes = caps
            .process_memory_limit_mb
            .map(|megabytes| {
                megabytes.checked_mul(1_048_576).ok_or_else(|| {
                    EduMindError::Process(
                        "configured process memory cap overflows bytes".to_owned(),
                    )
                })
            })
            .transpose()?;
        Ok(Self::new(program)
            .timeout(Duration::from_secs(caps.max_tool_timeout_secs))
            .stdout_max_bytes(caps.max_output_bytes)
            .stderr_max_bytes(caps.max_output_bytes)
            .memory_limit_bytes(memory_limit_bytes)
            .windows_job_object(caps.windows_job_objects))
    }

    /// Appends one process argument.
    #[must_use]
    pub fn arg(mut self, argument: impl Into<String>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Appends multiple process arguments.
    #[must_use]
    pub fn args<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(arguments.into_iter().map(Into::into));
        self
    }

    /// Sets the command working directory.
    #[must_use]
    pub fn working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Sets the maximum wall-clock execution time.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the maximum captured stdout bytes while still draining the child pipe.
    #[must_use]
    pub fn stdout_max_bytes(mut self, max_bytes: usize) -> Self {
        self.stdout_max_bytes = max_bytes;
        self
    }

    /// Sets the maximum captured stderr bytes while still draining the child pipe.
    #[must_use]
    pub fn stderr_max_bytes(mut self, max_bytes: usize) -> Self {
        self.stderr_max_bytes = max_bytes;
        self
    }

    /// Sets an optional process memory cap for Windows Job Objects.
    #[must_use]
    pub fn memory_limit_bytes(mut self, max_bytes: Option<u64>) -> Self {
        self.memory_limit_bytes = max_bytes;
        self
    }

    /// Enables or disables Windows Job Object attachment on supported platforms.
    #[must_use]
    pub fn windows_job_object(mut self, enabled: bool) -> Self {
        self.use_windows_job_object = enabled;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.program.trim().is_empty()
            || self.timeout.is_zero()
            || self.stdout_max_bytes == 0
            || self.stderr_max_bytes == 0
            || self.memory_limit_bytes == Some(0)
        {
            return Err(EduMindError::Process(
                "guarded command requires non-empty program and non-zero caps".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Bounded process output with an explicit truncation indicator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedOutput {
    pub text: String,
    pub truncated: bool,
}

/// Security controls applied to a completed process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessSandboxReport {
    pub kill_on_drop: bool,
    pub windows_job_object_requested: bool,
    pub windows_job_object_applied: bool,
    pub memory_limit_bytes: Option<u64>,
}

/// Outcome returned by a guarded external command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardedCommandResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub sandbox: ProcessSandboxReport,
}

/// Runs an external command with timeout, bounded output capture, kill-on-drop, and Job Objects.
pub async fn run_guarded_command(spec: GuardedCommandSpec) -> Result<GuardedCommandResult> {
    spec.validate()?;
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(working_directory) = &spec.working_directory {
        command.current_dir(working_directory);
    }
    let mut child = command.spawn().map_err(|error| {
        EduMindError::Process(format!("failed to start guarded command: {error}"))
    })?;
    #[cfg(windows)]
    let job = if spec.use_windows_job_object {
        let process_id = child.id().ok_or_else(|| {
            EduMindError::Process("guarded child did not expose a process ID".to_owned())
        })?;
        Some(WindowsJob::attach(process_id, spec.memory_limit_bytes)?)
    } else {
        None
    };
    #[cfg(windows)]
    let windows_job_object_applied = job.is_some();
    #[cfg(not(windows))]
    let windows_job_object_applied = false;

    let stdout = child.stdout.take().ok_or_else(|| {
        EduMindError::Process("guarded child stdout pipe was unavailable".to_owned())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        EduMindError::Process("guarded child stderr pipe was unavailable".to_owned())
    })?;
    let stdout_task = tokio::spawn(read_capped(stdout, spec.stdout_max_bytes));
    let stderr_task = tokio::spawn(read_capped(stderr, spec.stderr_max_bytes));
    let (status, timed_out) = match tokio::time::timeout(spec.timeout, child.wait()).await {
        Ok(status) => (
            status.map_err(|error| {
                EduMindError::Process(format!("failed while waiting for guarded command: {error}"))
            })?,
            false,
        ),
        Err(_) => {
            child.start_kill().map_err(|error| {
                EduMindError::Process(format!("failed to stop timed-out guarded command: {error}"))
            })?;
            (
                child.wait().await.map_err(|error| {
                    EduMindError::Process(format!(
                        "failed while waiting for timed-out guarded command: {error}"
                    ))
                })?,
                true,
            )
        }
    };
    let stdout = join_captured(stdout_task).await?;
    let stderr = join_captured(stderr_task).await?;
    #[cfg(windows)]
    drop(job);
    Ok(GuardedCommandResult {
        exit_code: status.code(),
        success: status.success() && !timed_out,
        timed_out,
        stdout,
        stderr,
        sandbox: ProcessSandboxReport {
            kill_on_drop: true,
            windows_job_object_requested: spec.use_windows_job_object,
            windows_job_object_applied,
            memory_limit_bytes: spec.memory_limit_bytes,
        },
    })
}

async fn read_capped<R>(mut reader: R, max_bytes: usize) -> Result<CapturedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8_192];
    let mut captured = Vec::with_capacity(max_bytes.min(buffer.len()));
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await.map_err(|error| {
            EduMindError::Process(format!("failed to read guarded command output: {error}"))
        })?;
        if read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(captured.len());
        let captured_bytes = read.min(remaining);
        captured.extend_from_slice(&buffer[..captured_bytes]);
        truncated |= captured_bytes < read;
    }
    Ok(CapturedOutput {
        text: String::from_utf8_lossy(&captured).into_owned(),
        truncated,
    })
}

async fn join_captured(task: JoinHandle<Result<CapturedOutput>>) -> Result<CapturedOutput> {
    task.await
        .map_err(|error| EduMindError::Process(format!("output capture task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{GuardedCommandSpec, run_guarded_command};

    fn echo_spec() -> GuardedCommandSpec {
        #[cfg(windows)]
        {
            GuardedCommandSpec::new("cmd")
                .args(["/C", "echo EduMind"])
                .windows_job_object(false)
        }
        #[cfg(not(windows))]
        {
            GuardedCommandSpec::new("sh")
                .args(["-c", "printf EduMind"])
                .windows_job_object(false)
        }
    }

    fn slow_spec() -> GuardedCommandSpec {
        #[cfg(windows)]
        {
            GuardedCommandSpec::new("cmd")
                .args(["/C", "ping 127.0.0.1 -n 3 > NUL"])
                .windows_job_object(false)
        }
        #[cfg(not(windows))]
        {
            GuardedCommandSpec::new("sh")
                .args(["-c", "sleep 1"])
                .windows_job_object(false)
        }
    }

    #[tokio::test]
    async fn captures_successful_process_output() {
        let result = run_guarded_command(echo_spec()).await.unwrap();

        assert!(result.success);
        assert!(result.stdout.text.contains("EduMind"));
        assert!(!result.sandbox.windows_job_object_requested);
    }

    #[tokio::test]
    async fn terminates_processes_that_exceed_the_timeout() {
        let result = run_guarded_command(slow_spec().timeout(Duration::from_millis(25)))
            .await
            .unwrap();

        assert!(result.timed_out);
        assert!(!result.success);
    }
}
