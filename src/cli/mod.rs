pub mod fixtures;

use clap::{Parser, Subcommand};

#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Fixtures {
        #[command(subcommand)]
        action: FixtureActions
    },

    Server {
        #[command(subcommand)]
        action: ServerActions
    }
}

#[derive(Subcommand)]
pub enum FixtureActions {
    Prepare,
    Cleanup
}

#[derive(Subcommand)]
pub enum ServerActions {
    DryStart,
    Scan
}