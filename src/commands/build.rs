use crate::cli::BuildArgs;
use miette::{IntoDiagnostic, Result, miette};
use std::env;

pub fn run(args: BuildArgs) -> Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;

    Err(miette!(
        "build is not implemented yet.\n\
         project: {}\n\
         cwd: {}\n\
         output: {}\n\
         entrypoint: {}\n\
         python override: {}",
        args.project.display(),
        cwd.display(),
        args.output
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<default>".to_string()),
        args.entrypoint.as_deref().unwrap_or("<auto>"),
        args.python.as_deref().unwrap_or("<from project>"),
    ))
}
