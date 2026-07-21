use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a user-facing task.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub Uuid);

impl TaskId {
    /// Creates a new random task identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable identifier for an EduMind product module.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModuleId(pub String);

impl ModuleId {
    /// Creates an identifier after normalizing surrounding whitespace.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_owned())
    }
}

/// The first-class EduMind modules available to students.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StudyModule {
    ClassNotes,
    CtPrep,
    LabReport,
    Routine,
    Research,
    MissionVision,
}

impl StudyModule {
    /// Returns the stable routing identifier used by configuration and storage.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::ClassNotes => "class-notes",
            Self::CtPrep => "ct-prep",
            Self::LabReport => "lab-report",
            Self::Routine => "routine",
            Self::Research => "research",
            Self::MissionVision => "mission-vision",
        }
    }
}

/// Lifecycle state for a task initiated by a student or agent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Minimal durable task record shared by gateway, agents, and UI clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub module_id: ModuleId,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Creates a queued task with a UTC creation timestamp.
    #[must_use]
    pub fn new(module_id: ModuleId, title: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            id: TaskId::new(),
            module_id,
            title: title.into(),
            status: TaskStatus::Queued,
            created_at: now,
            updated_at: now,
        }
    }

    /// Updates the task lifecycle state and modification timestamp.
    pub fn transition_to(&mut self, status: TaskStatus, now: DateTime<Utc>) {
        self.status = status;
        self.updated_at = now;
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::{ModuleId, StudyModule, Task, TaskStatus};

    #[test]
    fn module_ids_are_stable() {
        assert_eq!(StudyModule::ClassNotes.id(), "class-notes");
        assert_eq!(StudyModule::MissionVision.id(), "mission-vision");
    }

    #[test]
    fn task_transition_updates_its_timestamp() {
        let created_at = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let updated_at = chrono::Utc.with_ymd_and_hms(2026, 7, 15, 10, 5, 0).unwrap();
        let mut task = Task::new(ModuleId::new("class-notes"), "Digest lecture", created_at);

        task.transition_to(TaskStatus::Running, updated_at);

        assert_eq!(task.status, TaskStatus::Running);
        assert_eq!(task.created_at, created_at);
        assert_eq!(task.updated_at, updated_at);
    }
}
