mod backlog;
mod category;
pub use backlog::{BacklogDao, PgBacklogDao};
mod daily;
mod daily_move;
pub mod daily_work;
mod reminder;
pub use reminder::{PgReminderDao, ReminderDao};
pub mod maintenance;
pub mod morning;
mod scheduled;
mod task;
mod user;
pub use scheduled::{PgScheduledDao, ScheduledDao, generate_scheduled};

pub use category::{CategoryDao, PgCategoryDao};
pub use daily::{DailyDao, DailyPlanningDao, PgDailyDao};
pub use task::{PgTaskDao, TaskDao};
pub use user::PgUserDao;

#[derive(Debug, thiserror::Error)]
pub enum DaoError {
    #[error("{0}")]
    InvalidMove(&'static str),
    #[error("subtask IDs must be unique and belong to the task")]
    InvalidSubtaskIds,
    #[error("category does not exist")]
    CategoryMissing,
    #[error("category still contains tasks")]
    CategoryNotEmpty,
    #[error("archived tasks are read-only; restore the task first")]
    TaskArchived,
    #[error("database operation failed")]
    Database(#[source] sqlx::Error),
}

impl From<sqlx::Error> for DaoError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|error| error.is_foreign_key_violation())
}
