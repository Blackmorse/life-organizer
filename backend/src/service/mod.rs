mod category;
mod task;

pub use category::CategoryService;
pub use task::TaskService;

use crate::dao::DaoError;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("category still contains tasks")]
    Conflict,
    #[error("archived tasks are read-only; restore the task first")]
    TaskArchived,
    #[error("storage operation failed")]
    Storage(#[source] DaoError),
}

impl From<DaoError> for ServiceError {
    fn from(error: DaoError) -> Self {
        match error {
            DaoError::CategoryMissing => Self::NotFound("category"),
            DaoError::CategoryNotEmpty => Self::Conflict,
            DaoError::TaskArchived => Self::TaskArchived,
            error => Self::Storage(error),
        }
    }
}

fn validate_description(description: &str) -> Result<(), ServiceError> {
    if description.trim().is_empty() {
        Err(ServiceError::InvalidInput("description must not be blank"))
    } else {
        Ok(())
    }
}

fn validate_id(id: i64) -> Result<(), ServiceError> {
    if id <= 0 {
        Err(ServiceError::InvalidInput("id must be positive"))
    } else {
        Ok(())
    }
}
