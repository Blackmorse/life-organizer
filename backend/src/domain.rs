use serde::{Deserialize, Serialize};

/// Unfinished ordinary work absent from today's picks, ordered by urgency.
pub struct BacklogTask {
    pub description: String,
    pub category_id: i64,
    pub category: String,
    pub urgency: TaskUrgency,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReminderTaskInput {
    pub description: String,
    #[serde(default)]
    pub detailed_description: String,
    /// Unix milliseconds; for all-day work, only the saved-zone date is meaningful.
    pub due_at: i64,
    #[serde(default)]
    pub all_day: bool,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderTask {
    pub id: i64,
    #[serde(flatten)]
    pub details: ReminderTaskInput,
    pub completed: bool,
    pub expires_at: Option<i64>,
    /// End of today's manual-selection window, or None when not eligible.
    pub available_today_until: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledTaskInput {
    pub description: String,
    #[serde(default)]
    pub detailed_description: String,
    pub weekdays: Vec<i16>,
    pub timezone: String,
    #[serde(default)]
    pub subtasks: Vec<Subtask>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: i64,
    pub available_today_until: Option<i64>,
    #[serde(flatten)]
    pub details: ScheduledTaskInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledOccurrence {
    pub id: i64,
    pub scheduled_task_id: i64,
    pub description: String,
    #[serde(default)]
    pub detailed_description: String,
    pub subtasks: Vec<Subtask>,
    pub completed: bool,
    pub expires_at: i64,
}

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

/// Identity namespace and presentation section for a daily summary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DailyTaskSource {
    Ordinary,
    Scheduled,
    Reminder,
}

/// A current, user-owned daily pick resolved in a single database snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyTaskSummary {
    pub source: DailyTaskSource,
    pub task_id: i64,
    /// None for category-free scheduled occurrences and reminders.
    pub category_id: Option<i64>,
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
    #[serde(default)]
    pub detailed_description: String,
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
    #[serde(default)]
    pub detailed_description: String,
    pub completed: bool,
}

impl Subtask {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: None,
            description: description.into(),
            detailed_description: String::new(),
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
