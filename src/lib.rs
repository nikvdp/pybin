pub mod cli;
pub mod commands;
pub mod logging;
pub mod packer;
pub mod plan;
pub mod project;
pub mod runner;
pub mod sfx;

use clap::Parser;
use cli::{Cli, Command};
use miette::Result;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    logging::init(cli.verbose, cli.quiet)?;

    match cli.command {
        Command::Build(args) => commands::build::run(args),
        Command::Inspect(args) => commands::inspect::run(args),
        Command::Doctor(args) => commands::doctor::run(args),
    }
}
