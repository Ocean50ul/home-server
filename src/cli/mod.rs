use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Serve(ServerArgs),
    Prepare(PrepareArgs),
}

/// Arguments for the `serve` command
#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("action").required(false)
))]
pub struct ServerArgs {
    /// Start the server without actually running services
    #[arg(long, group = "action")]
    pub dry_start: bool,

    /// Scan the media library
    #[arg(long, group = "action")]
    pub scan: bool,

    /// Resample audio files
    #[arg(long, group = "action")]
    pub resample: bool,

    /// Sync with a remote backup
    #[arg(long, group = "action")]
    pub sync: bool,
}

/// Arguments for the `prepare` command
#[derive(Args, Debug)]
pub struct PrepareArgs {
    /// Use development-specific settings
    #[arg(long)]
    pub dev: bool,
}