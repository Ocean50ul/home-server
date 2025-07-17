use axum::{response::{Html, IntoResponse}, routing::{get, get_service}, Router};
use tower_http::services::{ServeDir};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .nest_service("/static", get_service(ServeDir::new("./static")));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> impl IntoResponse {
    Html(include_str!("../static/index.html"))
}