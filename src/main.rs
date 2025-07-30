use std::path::PathBuf;

use clap::Parser;
use anyhow::Error;

use home_server::{
    cli::{Cli, Commands}, 
    services::{prepare::run_prepare_userspace, resample::{FfmpegResampler, ResampleConfig, ResampleService, ResampleStrategy}, scanner::MediaScanner, sync::MusicLibSyncService}, 
    utils::{config::get_config, db::get_application_db}, 
    web::routes::create_router
};


#[tokio::main]
async fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Serve(args) => {

            if args.dry_start {

                let db = get_application_db().await?;
                let app = create_router(db.get_pool()).await?;

                let address = "0.0.0.0:8080";
                let listener = tokio::net::TcpListener::bind(address).await?;

                println!("Listening on http://{}", address);

                axum::serve(listener, app).await?;

            } else if args.scan {

                let config = get_config()?;
                let scanner = MediaScanner::new(config.media.music_path.clone());
                let scanning_result = scanner.scan_music_lib()?;

                if scanning_result.descriptors.is_empty() && scanning_result.errors.is_empty() {
                    println!("Music library is empty. Consider adding some tracks into ./data/media/music/");
                } else {
                    println!("{:?}", scanning_result);
                }

            } else if args.resample {

                let config = get_config()?;

                let scanner = MediaScanner::new(config.media.music_path.clone());
                let scanning_result = scanner.scan_music_lib()?;

                let resample_cofig = ResampleConfig {
                    strategy: ResampleStrategy::InPlace,
                    ..Default::default()
                };
                let ffmpeg_resampler = FfmpegResampler { ffmpeg_path: PathBuf::from("./ffmpeg/ffmpeg.exe")};
                let resample_service = ResampleService::new(resample_cofig, ffmpeg_resampler);

                let resample_report = resample_service.resample_library(&scanning_result);
                println!("{:?}", resample_report);

            } else if args.sync {

                let db = get_application_db().await?;
                let config = get_config()?;

                let sync_service = MusicLibSyncService::new(db.get_pool(), config.media.music_path.clone()).await?;
                let sync_report = sync_service.synchronize().await?;

                println!("{:?}", sync_report);

            } else {

                let db = get_application_db().await?;
                let config = get_config()?;

                let scanner = MediaScanner::new(config.media.music_path.clone());
                let scanning_result = scanner.scan_music_lib()?;

                let resample_cofig = ResampleConfig {
                    strategy: ResampleStrategy::InPlace,
                    ..Default::default()
                };

                let ffmpeg_resampler = FfmpegResampler { ffmpeg_path: PathBuf::from("./ffmpeg/ffmpeg.exe")};
                let resample_service = ResampleService::new(resample_cofig, ffmpeg_resampler);

                let _resample_report = resample_service.resample_library(&scanning_result);

                let sync_service = MusicLibSyncService::new(db.get_pool(), config.media.music_path.clone()).await?;
                let _sync_report = sync_service.synchronize().await?;

                let app = create_router(db.get_pool()).await?;

                let address = "0.0.0.0:8080";
                let listener = tokio::net::TcpListener::bind(address).await?;

                println!("Listening on http://{}", address);

                axum::serve(listener, app).await?;

            }
        },

        Commands::Prepare(args) => {
            
            if args.dev {
                println!("UNDER CONSTRUCTION");
            } else {
                println!("\nHello, this is preparation service!");
                println!("\nIn order for the server to work, we need to create couple of dirs, a DB instance and download ffmpeg.exe.");
                println!("The links for the ffmpeg and sha checksum are inside the config.toml: [ffmpeg_donwload_mirror] and [ffmpeg_sha_download_mirror].");
                println!("By default, we are using gyan.dev mirror.\n");
                println!("There is two types of preparation service: one that prepares environment for using the server and one that prepares it for development.");
                println!("By default, we prepare environment for using the server. To prepare dev environment you should run 'cargo run prepare --dev'.");
                println!("\nPlease, take a seat, have fun and press any key: ");

                run_prepare_userspace()?;
            }
        }
    }


    Ok(())
}
