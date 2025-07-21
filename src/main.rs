use axum::{response::{Html, IntoResponse}, routing::{get, get_service}, Router};
use clap::Parser;
use tower_http::services::{ServeDir};

use home_server::cli::{Cli, Command, FixtureActions};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Fixtures { action } => {
            match action {
                FixtureActions::Prepare => {
                    println!("hello from prepare");
                },

                FixtureActions::Cleanup => {
                    println!("hello from cealnup");
                }
            }
        }
    }
}

async fn run_app() -> Result<(), std::io::Error> {
    let app = Router::new()
        .route("/", get(root))
        .nest_service("/static", get_service(ServeDir::new("./static")));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await
}

async fn root() -> impl IntoResponse {
    Html(include_str!("../static/index.html"))
}