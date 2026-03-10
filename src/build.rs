use crate::plan::BuildPlan;
use flate2::read::GzDecoder;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use tar::Archive;
use tracing::info;

#[derive(Debug, Clone)]
pub struct PrepareBuildOptions {
    pub work_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PreparedBuild {
    pub work_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub conda_prefix: PathBuf,
    pub inner_env_path: PathBuf,
    pub packed_env_path: PathBuf,
    pub stage_dir: PathBuf,
    pub launcher_relpath: PathBuf,
}

pub fn prepare_build(plan: &BuildPlan, options: &PrepareBuildOptions) -> Result<PreparedBuild> {
    let paths = BuildPaths::create(plan, options)?;
    fs::create_dir_all(&paths.logs_dir).into_diagnostic()?;

    let python_spec = conda_python_spec(plan);
    let inner_env_path = plan.inner_env_path_for(&paths.conda_prefix);
    let conda_python = conda_python_path(&paths.conda_prefix);
    let launcher_relpath = PathBuf::from("bin").join(format!("pybin-{}", plan.entrypoint_name));

    info!(
        project = %plan.project_root.display(),
        work_dir = %paths.work_dir.display(),
        conda_prefix = %paths.conda_prefix.display(),
        inner_env = %inner_env_path.display(),
        "preparing staged build",
    );

    run_logged(
        "conda-create",
        &paths.logs_dir,
        &plan.project_root,
        "conda",
        &[
            OsString::from("create"),
            OsString::from("-y"),
            OsString::from("-p"),
            paths.conda_prefix.as_os_str().to_os_string(),
            OsString::from(python_spec),
            OsString::from("uv"),
            OsString::from("conda-pack"),
        ],
        &[],
    )?;

    let mut uv_sync_args = vec![
        OsString::from("run"),
        OsString::from("-p"),
        paths.conda_prefix.as_os_str().to_os_string(),
        OsString::from("uv"),
        OsString::from("sync"),
        OsString::from("--no-editable"),
        OsString::from("--link-mode"),
        OsString::from("copy"),
        OsString::from("--python"),
        conda_python.as_os_str().to_os_string(),
    ];
    if plan.uv_lock_present {
        uv_sync_args.push(OsString::from("--frozen"));
    }

    run_logged(
        "uv-sync",
        &paths.logs_dir,
        &plan.project_root,
        "conda",
        &uv_sync_args,
        &[
            (
                "UV_PROJECT_ENVIRONMENT",
                inner_env_path.as_os_str().to_os_string(),
            ),
            ("UV_LINK_MODE", OsString::from("copy")),
        ],
    )?;

    run_logged(
        "conda-pack",
        &paths.logs_dir,
        &plan.project_root,
        "conda",
        &[
            OsString::from("run"),
            OsString::from("-p"),
            paths.conda_prefix.as_os_str().to_os_string(),
            OsString::from("conda-pack"),
            OsString::from("-p"),
            paths.conda_prefix.as_os_str().to_os_string(),
            OsString::from("-o"),
            paths.packed_env_path.as_os_str().to_os_string(),
            OsString::from("--force"),
        ],
        &[],
    )?;

    unpack_tarball(&paths.packed_env_path, &paths.stage_dir)?;
    write_launcher(plan, &paths.stage_dir, &launcher_relpath)?;

    Ok(PreparedBuild {
        work_dir: paths.work_dir,
        logs_dir: paths.logs_dir,
        conda_prefix: paths.conda_prefix,
        inner_env_path,
        packed_env_path: paths.packed_env_path,
        stage_dir: paths.stage_dir,
        launcher_relpath,
    })
}

#[derive(Debug)]
struct BuildPaths {
    work_dir: PathBuf,
    logs_dir: PathBuf,
    conda_prefix: PathBuf,
    packed_env_path: PathBuf,
    stage_dir: PathBuf,
}

impl BuildPaths {
    fn create(plan: &BuildPlan, options: &PrepareBuildOptions) -> Result<Self> {
        let root = if let Some(path) = &options.work_dir {
            path.clone()
        } else {
            let slug = slugify(&plan.package_name);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            plan.project_root
                .join("target")
                .join("pybin")
                .join(format!("{timestamp}-{slug}"))
        };

        fs::create_dir_all(&root).into_diagnostic()?;

        Ok(Self {
            logs_dir: root.join("logs"),
            conda_prefix: root.join("conda-prefix"),
            packed_env_path: root.join("conda-prefix.tar.gz"),
            stage_dir: root.join("stage"),
            work_dir: root,
        })
    }
}

fn conda_python_spec(plan: &BuildPlan) -> String {
    let Some(request) = plan.python_request.as_ref() else {
        return "python".to_string();
    };

    let value = request.value.trim();
    if value.starts_with("python") {
        value.to_string()
    } else if value
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, '<' | '>' | '=' | '!' | '~'))
    {
        format!("python{value}")
    } else {
        format!("python={value}")
    }
}

fn conda_python_path(conda_prefix: &Path) -> PathBuf {
    let executable = if cfg!(windows) {
        "python.exe"
    } else {
        "bin/python"
    };
    conda_prefix.join(executable)
}

fn unpack_tarball(archive_path: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination).into_diagnostic()?;
    }
    fs::create_dir_all(destination).into_diagnostic()?;

    let archive = fs::File::open(archive_path).into_diagnostic()?;
    let decoder = GzDecoder::new(archive);
    let mut tar = Archive::new(decoder);
    tar.unpack(destination)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to unpack `{}`", archive_path.display()))?;

    Ok(())
}

fn write_launcher(plan: &BuildPlan, stage_dir: &Path, launcher_relpath: &Path) -> Result<()> {
    let launcher_path = stage_dir.join(launcher_relpath);
    if let Some(parent) = launcher_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }

    let entrypoint = if cfg!(windows) {
        format!("{}.exe", plan.entrypoint_name)
    } else {
        plan.entrypoint_name.clone()
    };
    let inner_env = plan.inner_env_relative_path.to_string_lossy();
    let script = format!(
        "#!/usr/bin/env bash\n\
set -euo pipefail\n\
SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
ROOT_DIR=\"$(cd \"$SCRIPT_DIR/..\" && pwd)\"\n\
export PATH=\"$ROOT_DIR/{inner_env}/bin:$ROOT_DIR/bin:$PATH\"\n\
exec \"$ROOT_DIR/{inner_env}/bin/{entrypoint}\" \"$@\"\n"
    );

    fs::write(&launcher_path, script).into_diagnostic()?;

    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&launcher_path)
            .into_diagnostic()?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher_path, permissions).into_diagnostic()?;
    }

    Ok(())
}

fn run_logged(
    step_name: &str,
    logs_dir: &Path,
    current_dir: &Path,
    program: &str,
    args: &[OsString],
    envs: &[(&str, OsString)],
) -> Result<()> {
    let log_path = logs_dir.join(format!("{step_name}.log"));
    let mut command = Command::new(program);
    command.current_dir(current_dir).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    info!(
        step = step_name,
        program,
        cwd = %current_dir.display(),
        log = %log_path.display(),
        "running build step",
    );

    let output = command.output().into_diagnostic().wrap_err_with(|| {
        format!(
            "failed to spawn `{}` for step `{step_name}`",
            render_command(program, args)
        )
    })?;

    write_command_log(&log_path, current_dir, program, args, envs, &output)?;

    if output.status.success() {
        return Ok(());
    }

    Err(miette!(
        "build step `{step_name}` failed; see `{}` for stdout/stderr",
        log_path.display()
    ))
}

fn write_command_log(
    log_path: &Path,
    current_dir: &Path,
    program: &str,
    args: &[OsString],
    envs: &[(&str, OsString)],
    output: &Output,
) -> Result<()> {
    let mut body = String::new();
    body.push_str(&format!("cwd: {}\n", current_dir.display()));
    body.push_str(&format!("command: {}\n", render_command(program, args)));
    if !envs.is_empty() {
        body.push_str("env:\n");
        for (key, value) in envs {
            body.push_str(&format!("  {key}={}\n", value.to_string_lossy()));
        }
    }
    body.push_str(&format!("status: {}\n", output.status));
    body.push_str("\nstdout:\n");
    body.push_str(&String::from_utf8_lossy(&output.stdout));
    body.push_str("\n\nstderr:\n");
    body.push_str(&String::from_utf8_lossy(&output.stderr));

    fs::write(log_path, body).into_diagnostic()?;
    Ok(())
}

fn render_command(program: &str, args: &[OsString]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|value| render_os(value.as_os_str())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_os(value: &OsStr) -> String {
    let text = value.to_string_lossy();
    if text.contains([' ', '\t', '\n', '"']) {
        format!("{text:?}")
    } else {
        text.into_owned()
    }
}

fn slugify(input: &str) -> String {
    let slug: String = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{PythonRequest, PythonRequestSource};

    #[test]
    fn formats_exact_python_versions_for_conda() {
        let plan = BuildPlan {
            project_root: PathBuf::from("."),
            package_name: "demo".to_string(),
            python_request: Some(PythonRequest {
                value: "3.12".to_string(),
                source: PythonRequestSource::DotPythonVersion,
            }),
            entrypoint_name: "demo".to_string(),
            entrypoint_target: "demo.cli:main".to_string(),
            uv_lock_present: true,
            inner_env_relative_path: PathBuf::from("uv-env"),
        };

        assert_eq!(conda_python_spec(&plan), "python=3.12");
    }

    #[test]
    fn formats_requires_python_ranges_for_conda() {
        let plan = BuildPlan {
            project_root: PathBuf::from("."),
            package_name: "demo".to_string(),
            python_request: Some(PythonRequest {
                value: ">=3.12,<3.13".to_string(),
                source: PythonRequestSource::RequiresPython,
            }),
            entrypoint_name: "demo".to_string(),
            entrypoint_target: "demo.cli:main".to_string(),
            uv_lock_present: false,
            inner_env_relative_path: PathBuf::from("uv-env"),
        };

        assert_eq!(conda_python_spec(&plan), "python>=3.12,<3.13");
    }
}
