#![allow(dead_code)]

use std::sync::Arc;

use backend::{
    api::{AppState, router},
    auth::TelegramAuth,
    dao::{PgCategoryDao, PgTaskDao, PgUserDao},
    db::connect_and_migrate,
    domain::User,
    service::{CategoryService, TaskService},
};
use sqlx::PgPool;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

pub struct TestDb {
    pub pool: PgPool,
    pub user: User,
    _container: ContainerAsync<GenericImage>,
}

impl TestDb {
    /// Every call creates an isolated container and runs production migrations.
    pub async fn new() -> Self {
        let container = GenericImage::new("postgres", "16-alpine")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stdout(
                "PostgreSQL init process complete; ready for start up.",
            ))
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_DB", "scheduler")
            .with_env_var("POSTGRES_USER", "scheduler")
            .with_env_var("POSTGRES_PASSWORD", "test-password")
            .start()
            .await
            .expect("start PostgreSQL container (Docker must be running)");
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("postgres://scheduler:test-password@{host}:{port}/scheduler");
        let pool = connect_and_migrate(&url)
            .await
            .expect("connect and migrate test database");
        let user = PgUserDao::new(pool.clone())
            .get_or_create(TELEGRAM_ID)
            .await
            .unwrap();
        Self {
            pool,
            user,
            _container: container,
        }
    }

    pub fn categories(&self) -> PgCategoryDao {
        PgCategoryDao::new(self.pool.clone(), self.user.id)
    }

    pub fn tasks(&self) -> PgTaskDao {
        PgTaskDao::new(self.pool.clone(), self.user.id)
    }

    pub fn category_service(&self) -> CategoryService {
        CategoryService::new(Arc::new(self.categories()))
    }

    pub fn task_service(&self) -> TaskService {
        TaskService::new(Arc::new(self.tasks()))
    }

    pub fn app(&self) -> axum::Router {
        router(AppState {
            pool: self.pool.clone(),
            auth: TelegramAuth::new(BOT_TOKEN),
        })
    }
}

pub const BOT_TOKEN: &str = "123456:test-only-token";
pub const TELEGRAM_ID: i64 = 7_000_000_001;

pub fn authorization_for(telegram_id: i64) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::time::{SystemTime, UNIX_EPOCH};
    let auth_date = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let user = serde_json::json!({"id": telegram_id, "first_name": "Test"}).to_string();
    let data_check_string = format!("auth_date={auth_date}\nuser={user}");
    let mut key = Hmac::<Sha256>::new_from_slice(b"WebAppData").unwrap();
    key.update(BOT_TOKEN.as_bytes());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key.finalize().into_bytes()).unwrap();
    mac.update(data_check_string.as_bytes());
    let hash = hex::encode(mac.finalize().into_bytes());
    let data = form_urlencoded::Serializer::new(String::new())
        .append_pair("user", &user)
        .append_pair("auth_date", &auth_date)
        .append_pair("hash", &hash)
        .finish();
    format!("tma {data}")
}
