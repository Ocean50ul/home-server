use axum::{extract::{Path, Request, State}, http::{StatusCode}, response::{Html, IntoResponse}};
use tower_http::services::ServeFile;
use uuid::Uuid;
use tower::util::ServiceExt;

use crate::{repository::SqliteTracksRepository, web::AppState};

pub async fn serve_index(State(state): State<AppState>) -> impl IntoResponse {
    Html(state.index_html.as_ref().clone())
}

pub async fn serve_track(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match SqliteTracksRepository::new().by_id_fetch(state.pool, id).await {
        Ok(Some(track)) => {
            let request: Request<()> = Request::default();
            let serve_result = ServeFile::new(track.file_path()).oneshot(request).await;

            match serve_result {
                Ok(response) => response.into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to serve file: {}\nTrack: {:?}", err, track)
                ).into_response(),
            }
        },

        Ok(None) => (StatusCode::NOT_FOUND, "Track not found").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
}