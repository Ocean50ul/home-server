pub mod fixtures;

use std::{io, path::PathBuf};

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
    }
}

#[derive(Subcommand)]
pub enum FixtureActions {
    Prepare,
    Cleanup
}