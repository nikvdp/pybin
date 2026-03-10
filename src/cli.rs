use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "pybin",
    version,
    about = "Package uv-managed Python apps into self-extracting binaries",
    long_about = "pybin builds a self-extracting executable for a uv-managed Python \
project by using an outer conda prefix as the relocatable host and an inner uv-managed \
environment as the application runtime.",
    after_help = "This repository is still in bootstrap mode. The command surface is \
stable enough to wire up the remaining build pipeline, but most actions are not \
implemented yet."
)]
pub struct Cli {
    #[arg(
        short = 'v',
        long = "verbose",
        action = ArgAction::Count,
        global = true,
        help = "Increase log verbosity. Repeat for more detail."
    )]
    pub verbose: u8,

    #[arg(
        short = 'q',
        long = "quiet",
        global = true,
        help = "Suppress non-error log output."
    )]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build a self-extracting binary from a uv project
    Build(BuildArgs),
    /// Inspect a project and show the planned build inputs
    Inspect(InspectArgs),
    /// Check whether required tools are present before a build
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    #[arg(
        default_value = ".",
        value_name = "PROJECT",
        help = "Path to the uv project root."
    )]
    pub project: PathBuf,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the final executable to this path."
    )]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        value_name = "NAME",
        help = "Explicit entrypoint or script name to package."
    )]
    pub entrypoint: Option<String>,

    #[arg(
        long,
        value_name = "VERSION",
        help = "Override the Python version request before handing it to conda."
    )]
    pub python: Option<String>,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[arg(
        default_value = ".",
        value_name = "PROJECT",
        help = "Path to the uv project root."
    )]
    pub project: PathBuf,

    #[arg(
        long,
        value_name = "VERSION",
        help = "Override the Python version request before inspection."
    )]
    pub python: Option<String>,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(
        default_value = ".",
        value_name = "PROJECT",
        help = "Path to the uv project root."
    )]
    pub project: PathBuf,
}
