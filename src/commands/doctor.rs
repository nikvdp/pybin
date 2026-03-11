use crate::project::supported_project_markers;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    env::consts::OS,
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

pub fn require_host_prerequisites(project_root: &Path) -> Result<()> {
    if supported_project_markers(project_root).is_empty() {
        return Err(miette!(
            "`{}` does not contain a supported project marker (`pyproject.toml`, `setup.py`, or `requirements.txt`)",
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
        .wrap_err_with(missing_conda_guidance)?;

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

fn missing_conda_guidance() -> String {
    let mut message = String::from(
        "failed to run `conda --version`; `conda` does not appear to be installed or on PATH.\n\n",
    );
    message.push_str("Recommended installer: Miniforge from conda-forge.\n");

    match OS {
        "macos" | "linux" => {
            message.push_str("Official quick install commands:\n");
            message.push_str("  curl -L -O \"https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-$(uname)-$(uname -m).sh\"\n");
            message.push_str("  bash Miniforge3-$(uname)-$(uname -m).sh\n");
            message.push_str("  ~/miniforge3/bin/conda init\n");
            message.push_str("  exec $SHELL\n");
            message.push_str("  conda --version\n");
        }
        "windows" => {
            message.push_str("Official download page:\n");
            message.push_str("  https://conda-forge.org/download/\n");
            message.push_str("Official silent install command after downloading the installer:\n");
            message.push_str("  start /wait \"\" Miniforge3-Windows-x86_64.exe /InstallationType=JustMe /RegisterPython=0 /S /D=%UserProfile%\\Miniforge3\n");
            message.push_str("Then open a new shell and run:\n");
            message.push_str("  conda --version\n");
        }
        _ => {
            message.push_str("Official download page:\n");
            message.push_str("  https://conda-forge.org/download/\n");
            message.push_str("After installing, open a new shell and run:\n");
            message.push_str("  conda --version\n");
        }
    }

    message
}

fn first_non_empty_line(bytes: &[u8]) -> Option<OsString> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(OsString::from)
}

#[cfg(test)]
mod tests {
    use super::missing_conda_guidance;

    #[test]
    fn missing_conda_guidance_mentions_miniforge() {
        let guidance = missing_conda_guidance();
        assert!(guidance.contains("Miniforge"));
        assert!(
            guidance.contains("https://conda-forge.org/download/")
                || guidance.contains("github.com/conda-forge/miniforge/releases/latest/download")
        );
        assert!(guidance.contains("conda --version"));
    }
}
