use std::{ffi::c_void, mem::size_of};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
        Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
    },
};

use crate::infra::{EduMindError, Result};

/// Owning Windows Job Object that terminates descendants when dropped.
pub(super) struct WindowsJob {
    handle: usize,
}

impl WindowsJob {
    pub(super) fn attach(process_id: u32, memory_limit_bytes: Option<u64>) -> Result<Self> {
        let memory_limit_bytes = memory_limit_bytes
            .map(|value| {
                usize::try_from(value).map_err(|_| {
                    EduMindError::Process(
                        "configured process memory cap cannot fit this platform".to_owned(),
                    )
                })
            })
            .transpose()?;
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        if process.is_null() {
            return Err(last_error("failed to open guarded child process"));
        }
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            unsafe {
                CloseHandle(process);
            }
            return Err(last_error("failed to create Windows Job Object"));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(memory_limit_bytes) = memory_limit_bytes {
            limits.ProcessMemoryLimit = memory_limit_bytes;
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        }
        let size =
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|_| {
                EduMindError::Process("Windows Job Object limits structure is too large".to_owned())
            })?;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void,
                size,
            )
        };
        if configured == 0 {
            let error = last_error("failed to configure Windows Job Object");
            unsafe {
                CloseHandle(process);
                CloseHandle(job);
            }
            return Err(error);
        }
        let assigned = unsafe { AssignProcessToJobObject(job, process) };
        unsafe {
            CloseHandle(process);
        }
        if assigned == 0 {
            let error = last_error("failed to assign guarded child to Windows Job Object");
            unsafe {
                CloseHandle(job);
            }
            return Err(error);
        }
        Ok(Self {
            handle: job as usize,
        })
    }
}

impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}

fn last_error(context: &str) -> EduMindError {
    EduMindError::Process(format!("{context}: {}", std::io::Error::last_os_error()))
}
