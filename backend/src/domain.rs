use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub telegram_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyTask {
    pub task_id: i64,
    /// Empty for a whole-task selection.
    pub subtask_id: String,
    pub expires_at: i64,
}

/// A current, user-owned daily pick resolved in a single database snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyTaskSummary {
    pub task_id: i64,
    pub category: String,
    pub task: String,
    pub subtask: Option<String>,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub user_id: i64,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub category_id: i64,
    pub description: String,
    pub urgency: TaskUrgency,
    pub subtasks: Vec<Subtask>,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subtask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub description: String,
    pub completed: bool,
}

impl Subtask {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: None,
            description: description.into(),
            completed: false,
        }
    }
}

/// Planning horizon, independent of task category or completion status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskUrgency {
    #[default]
    Someday,
    Upcoming,
    Soon,
}

impl TaskUrgency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Someday => "someday",
            Self::Upcoming => "upcoming",
            Self::Soon => "soon",
        }
    }
}

impl TryFrom<&str> for TaskUrgency {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "someday" => Ok(Self::Someday),
            "upcoming" => Ok(Self::Upcoming),
            "soon" => Ok(Self::Soon),
            _ => Err("unknown task urgency"),
        }
    }
}
