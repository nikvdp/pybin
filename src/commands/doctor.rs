use crate::cli::DoctorArgs;
use miette::{IntoDiagnostic, Result, miette};
use std::env;

pub fn run(args: DoctorArgs) -> Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;

    Err(miette!(
        "doctor is not implemented yet.\n\
         project: {}\n\
         cwd: {}\n\
         planned checks: conda, uv, conda-pack, and runner prerequisites",
        args.project.display(),
        cwd.display(),
    ))
}
