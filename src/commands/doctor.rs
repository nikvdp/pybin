use crate::{
    cli::DoctorArgs,
    plan::BuildPlan,
    project::{PythonRequest, PythonRequestSource, load_project_metadata},
};
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

pub fn run(args: DoctorArgs) -> Result<()> {
    let metadata = load_project_metadata(&args.project, None)?;
    let plan = BuildPlan::resolve(metadata, None)?;
    let conda = check_conda()?;

    println!("project root: {}", plan.project_root.display());
    println!("package: {}", plan.package_name);
    println!(
        "entrypoint: {} -> {}",
        plan.entrypoint_name, plan.entrypoint_target
    );
    println!(
        "python request: {}",
        plan.python_request
            .as_ref()
            .map(format_python_request)
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!("uv.lock present: {}", yes_no(plan.uv_lock_present));
    println!(
        "inner uv env: <conda-prefix>/{}",
        plan.inner_env_relative_path.display()
    );
    println!();
    println!("host prerequisites:");
    println!("  [ok] conda: {}", conda.version_line);
    println!(
        "  [ok] pyproject.toml: {}",
        plan.project_root.join("pyproject.toml").display()
    );
    println!("  [ok] output stub: current pybin executable will be used as the SFX base");
    println!("  [info] host uv is not required; pybin installs uv inside the conda build prefix");
    println!("  [info] conda-pack is installed inside the conda build prefix during each build");
    println!();
    println!("ready: yes");

    Ok(())
}

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
struct CondaCheck {
    version_line: String,
}

fn check_conda() -> Result<CondaCheck> {
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

fn format_python_request(request: &PythonRequest) -> String {
    let source = match request.source {
        PythonRequestSource::Override => "override",
        PythonRequestSource::DotPythonVersion => ".python-version",
        PythonRequestSource::DotVenv => ".venv/pyvenv.cfg",
        PythonRequestSource::RequiresPython => "project.requires-python",
    };

    format!("{} ({source})", request.value)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
