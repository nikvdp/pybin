use crate::sfx::PayloadCompression;
use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "pybin",
    version,
    about = "Package uv-managed Python apps into self-extracting binaries",
    long_about = "pybin builds a self-extracting executable for a uv-managed Python \
project by combining a relocatable outer conda prefix with an inner uv-managed \
runtime environment.",
    after_help = "Only host `conda` is required. pybin installs `uv` and \
`conda-pack` inside the temporary build prefix, then packs the result into one \
self-extracting executable."
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
    /// Inspect whether a project is packable and show the resolved build plan
    Inspect(InspectArgs),
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
        value_name = "ENTRY",
        help = "Script name from project metadata, or an explicit mapping like `name=module:function`."
    )]
    pub entrypoint: Option<String>,

    #[arg(
        long,
        value_name = "VERSION",
        help = "Override the Python request before handing it to conda."
    )]
    pub python: Option<String>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Keep intermediate build artifacts in this directory."
    )]
    pub work_dir: Option<PathBuf>,

    #[arg(
        long,
        value_name = "COMMAND",
        help = "Run this shell command inside the build prefix instead of pybin's built-in install strategy."
    )]
    pub install_command: Option<String>,

    #[arg(
        long,
        value_enum,
        default_value_t = PayloadCompression::Zstd,
        help = "Compression format for the embedded runtime payload."
    )]
    pub compression: PayloadCompression,
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
        value_name = "ENTRY",
        help = "Script name from project metadata, or an explicit mapping like `name=module:function`."
    )]
    pub entrypoint: Option<String>,

    #[arg(
        long,
        value_name = "VERSION",
        help = "Override the Python request before inspection."
    )]
    pub python: Option<String>,

    #[arg(
        long,
        value_name = "COMMAND",
        help = "Preview a custom shell install command instead of pybin's built-in install strategy."
    )]
    pub install_command: Option<String>,
}
