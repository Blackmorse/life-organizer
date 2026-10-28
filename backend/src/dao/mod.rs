mod category;
mod task;

pub use category::{CategoryDao, PgCategoryDao};
pub use task::{PgTaskDao, TaskDao};

#[derive(Debug, thiserror::Error)]
pub enum DaoError {
    #[error("category does not exist")]
    CategoryMissing,
    #[error("category still contains tasks")]
    CategoryNotEmpty,
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
