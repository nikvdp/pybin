pub mod build;
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
use miette::{IntoDiagnostic, Result};
use std::env;

pub fn run() -> Result<()> {
    let self_path = env::current_exe().into_diagnostic()?;
    if sfx::has_embedded_bundle(&self_path)? {
        return runner::run();
    }

    let cli = Cli::parse();
    logging::init(cli.verbose, cli.quiet)?;

    match cli.command {
        Command::Build(args) => commands::build::run(args),
        Command::Inspect(args) => commands::inspect::run(args),
    }
}
