use sqlx::PgPool;

use crate::domain::User;

use super::DaoError;

#[derive(Clone)]
pub struct PgUserDao {
    pool: PgPool,
}

impl PgUserDao {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The unique constraint makes first visits safe under concurrent requests.
    pub async fn get_or_create(&self, telegram_id: i64) -> Result<User, DaoError> {
        let (id, telegram_id) = sqlx::query_as(
            "INSERT INTO users (telegram_id) VALUES ($1) \
             ON CONFLICT (telegram_id) DO UPDATE SET telegram_id = EXCLUDED.telegram_id \
             RETURNING id, telegram_id",
        )
        .bind(telegram_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(User { id, telegram_id })
    }
}
