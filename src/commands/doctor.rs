use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

pub fn require_host_prerequisites(project_root: &Path) -> Result<()> {
    if !project_root.join("pyproject.toml").is_file() {
        return Err(miette!(
            "`{}` does not contain a `pyproject.toml` file",
            project_root.display()
        ));
    }

    check_conda().map(|_| ())
}

#[derive(Debug)]
pub(crate) struct CondaCheck {
    pub(crate) version_line: String,
}

pub(crate) fn check_conda() -> Result<CondaCheck> {
    let output = Command::new("conda")
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .into_diagnostic()
        .wrap_err("failed to run `conda --version`; install conda and ensure it is on PATH")?;

    if !output.status.success() {
        return Err(miette!(
            "`conda --version` failed.\nstdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let version_line = first_non_empty_line(&output.stdout)
        .or_else(|| first_non_empty_line(&output.stderr))
        .unwrap_or_else(|| OsString::from("conda <unknown>"))
        .to_string_lossy()
        .into_owned();

    Ok(CondaCheck { version_line })
}

fn first_non_empty_line(bytes: &[u8]) -> Option<OsString> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(OsString::from)
}
