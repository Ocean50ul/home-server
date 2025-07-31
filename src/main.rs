use std::path::{PathBuf};

use clap::Parser;
use anyhow::Error;

use home_server::{
    cli::{Cli, Commands}, 
    services::{prepare::{run_prepare_devspace, run_prepare_userspace}, resample::{FfmpegResampler, ResampleConfig, ResampleService, ResampleStrategy}, scanner::MediaScanner, sync::MusicLibSyncService}, 
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
                run_prepare_devspace().await?;
            } else {
                println!("\n\nRunning preparation service..");
                run_prepare_userspace().await?;
                println!("Preparation service is complete.");
            }
        }
    }


    Ok(())
}
