use std::sync::Arc;

use sqlx::SqlitePool;

use crate::repository::RepositoryError;

pub mod routes;
pub mod handlers;
pub mod template_builders;

#[derive(Debug, thiserror::Error)]
pub enum WebLayerError {
    #[error("{0}")]
    RepositoryError(#[from] RepositoryError),

    #[error("{0}")]
    AskamaError(#[from] askama::Error)
}

#[derive(Clone)]
pub struct AppState {
    pub pool: &'static SqlitePool,
    pub index_html: Arc<String>
}