use crate::cli::InspectArgs;
use miette::{IntoDiagnostic, Result, miette};
use std::env;

pub fn run(args: InspectArgs) -> Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;

    Err(miette!(
        "inspect is not implemented yet.\n\
         project: {}\n\
         cwd: {}\n\
         python override: {}",
        args.project.display(),
        cwd.display(),
        args.python.as_deref().unwrap_or("<from project>"),
    ))
}
