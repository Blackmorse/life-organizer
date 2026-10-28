use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Both application startup and integration tests use this bootstrap path.
pub async fn connect_and_migrate(database_url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
