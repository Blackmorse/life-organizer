use super::{ServiceError, notification::backlog_messages};
use crate::dao::BacklogDao;
use std::sync::Arc;

pub struct BacklogService {
    dao: Arc<dyn BacklogDao>,
}

impl BacklogService {
    pub fn new(dao: Arc<dyn BacklogDao>) -> Self {
        Self { dao }
    }

    pub async fn current_messages(&self) -> Result<Vec<String>, ServiceError> {
        Ok(backlog_messages(&self.dao.list().await?))
    }
}
