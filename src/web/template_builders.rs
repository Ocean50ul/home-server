use askama::Template;
use futures::TryStreamExt;
use sqlx::SqlitePool;

use crate::{domain::track::Track, repository::SqliteTracksRepository};
use super::WebLayerError;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    tracks: &'a Vec<Track>,
}

pub async fn build_index_page(db_pool: &SqlitePool) -> Result<String, WebLayerError> {
    let tracks = SqliteTracksRepository::new().stream_all(db_pool).await.try_collect::<Vec<_>>().await?;
    let template = IndexTemplate { tracks: &tracks };
    let html = template.render()?;

    Ok(html)
}