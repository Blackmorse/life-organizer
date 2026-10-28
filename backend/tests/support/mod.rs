#![allow(dead_code)]

use std::sync::Arc;

use backend::{
    api::{AppState, router},
    dao::{PgCategoryDao, PgTaskDao},
    db::connect_and_migrate,
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
        Self {
            pool,
            _container: container,
        }
    }

    pub fn categories(&self) -> PgCategoryDao {
        PgCategoryDao::new(self.pool.clone())
    }

    pub fn tasks(&self) -> PgTaskDao {
        PgTaskDao::new(self.pool.clone())
    }

    pub fn category_service(&self) -> CategoryService {
        CategoryService::new(Arc::new(self.categories()))
    }

    pub fn task_service(&self) -> TaskService {
        TaskService::new(Arc::new(self.tasks()))
    }

    pub fn app(&self) -> axum::Router {
        router(AppState {
            categories: self.category_service(),
            tasks: self.task_service(),
        })
    }
}
