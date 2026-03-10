use miette::{Result, miette};
use tracing_subscriber::{EnvFilter, fmt};

pub fn init(verbose: u8, quiet: bool) -> Result<()> {
    let filter = if quiet {
        EnvFilter::new("error")
    } else if verbose >= 2 {
        EnvFilter::new("debug")
    } else if verbose == 1 {
        EnvFilter::new("info")
    } else {
        EnvFilter::from_default_env()
    };

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|error| miette!(error.to_string()))?;

    Ok(())
}
