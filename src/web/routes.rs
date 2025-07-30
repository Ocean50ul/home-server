use std::sync::Arc;

use sqlx::SqlitePool;
use tower_http::services::{ServeDir};
use axum::{routing::{get}, Router};

use crate::web::{handlers::{serve_index, serve_track}, AppState, WebLayerError};
use super::template_builders::build_index_page;

pub async fn create_router(pool: &'static SqlitePool) -> Result<Router<()>, WebLayerError> {
    let index_html = build_index_page(pool).await?;
    let app_state = AppState { pool, index_html: Arc::new(index_html) };

    let app: Router<()> = Router::new()
        .route("/", get(serve_index))
        .route("/tracks/{id}", get(serve_track)) 
        .nest_service("/static", ServeDir::new("static"))
        .with_state(app_state);

    Ok(app)
}