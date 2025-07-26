use clap::Parser;
use anyhow::Error;

use home_server::{
    cli::{Cli, Command, FixtureActions, ServerActions}, 
    services::scanner::MediaScanner, 
    utils::{config::get_config, db::get_application_db}, 
    web::routes::create_router}
;


#[tokio::main]
async fn main() -> Result<(), Error> {
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
        },

        Command::Server { action } => {
            match action {
                ServerActions::DryStart => {
                    let db = get_application_db().await?;
                    let app = create_router(db.get_pool()).await?;

                    let address = "0.0.0.0:8080";
                    let listener = tokio::net::TcpListener::bind(address).await?;

                    println!("Listening on http://{}", address);

                    axum::serve(listener, app).await?;
                },

                ServerActions::Scan => {
                    let config = get_config()?;
                    let scanner = MediaScanner::new(config.media.music_path.clone());
                    let scanning_result = scanner.scan_music_lib()?;

                    if scanning_result.descriptors.is_empty() && scanning_result.errors.is_empty() {
                        println!("Music library is empty. Consider adding some tracks into ./data/media/music/");
                    }
                }
            }
        }
    }

    Ok(())
}
