use crate::{
    plan::BuildPlan,
    project::{PythonRequest, PythonRequestSource},
};
use flate2::read::GzDecoder;
use miette::{IntoDiagnostic, Result, miette};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use tar::Archive;

#[derive(Debug, Clone)]
pub struct PrepareBuildOptions {
    pub work_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PreparedBuild {
    pub work_dir: PathBuf,
    pub conda_prefix: PathBuf,
    pub inner_env_path: PathBuf,
    pub packed_env_path: PathBuf,
    pub stage_dir: PathBuf,
    pub launcher_relpath: PathBuf,
}

pub fn prepare_build(plan: &BuildPlan, options: &PrepareBuildOptions) -> Result<PreparedBuild> {
    let work_dir = resolve_work_dir(plan, options)?;
    fs::create_dir_all(&work_dir).into_diagnostic()?;

    let conda_prefix = work_dir.join("conda-prefix");
    let inner_env_path = plan.inner_env_path_for(&conda_prefix);
    let packed_env_path = work_dir.join("conda-env.tar.gz");
    let stage_dir = work_dir.join("stage");
    let launcher_relpath = PathBuf::from("bin").join(format!("pybin-{}", plan.entrypoint_name));

    create_outer_conda_env(plan.python_request.as_ref(), &conda_prefix)?;
    create_inner_uv_env(&conda_prefix, &inner_env_path)?;
    sync_project(plan, &conda_prefix, &inner_env_path)?;
    pack_outer_env(&conda_prefix, &packed_env_path)?;
    unpack_conda_env(&packed_env_path, &stage_dir)?;
    write_launcher(plan, &stage_dir, &launcher_relpath)?;

    Ok(PreparedBuild {
        work_dir,
        conda_prefix,
        inner_env_path,
        packed_env_path,
        stage_dir,
        launcher_relpath,
    })
}

fn resolve_work_dir(plan: &BuildPlan, options: &PrepareBuildOptions) -> Result<PathBuf> {
    if let Some(work_dir) = &options.work_dir {
        return Ok(work_dir.clone());
    }

    let build_root = plan.project_root.join(".pybin").join("builds");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(build_root.join(format!("{}-{timestamp}", plan.package_name)))
}

fn create_outer_conda_env(
    python_request: Option<&PythonRequest>,
    conda_prefix: &Path,
) -> Result<()> {
    let mut args = vec![
        "create".to_string(),
        "--yes".to_string(),
        "--prefix".to_string(),
        conda_prefix.display().to_string(),
    ];
    args.push(conda_python_spec(python_request));
    args.push("uv".to_string());
    args.push("conda-pack".to_string());

    run_command("conda", &args, None, &[])
}

fn create_inner_uv_env(conda_prefix: &Path, inner_env_path: &Path) -> Result<()> {
    let python_path = conda_prefix.join("bin").join("python");
    run_command(
        "conda",
        &[
            "run".to_string(),
            "--prefix".to_string(),
            conda_prefix.display().to_string(),
            "uv".to_string(),
            "venv".to_string(),
            inner_env_path.display().to_string(),
            "--python".to_string(),
            python_path.display().to_string(),
        ],
        None,
        &[],
    )
}

fn sync_project(plan: &BuildPlan, conda_prefix: &Path, inner_env_path: &Path) -> Result<()> {
    let mut args = vec![
        "run".to_string(),
        "--prefix".to_string(),
        conda_prefix.display().to_string(),
        "uv".to_string(),
        "sync".to_string(),
        "--project".to_string(),
        plan.project_root.display().to_string(),
        "--no-editable".to_string(),
    ];

    if plan.uv_lock_present {
        args.push("--frozen".to_string());
    }

    run_command(
        "conda",
        &args,
        Some(&plan.project_root),
        &[
            (
                "UV_PROJECT_ENVIRONMENT".to_string(),
                inner_env_path.display().to_string(),
            ),
            ("UV_LINK_MODE".to_string(), "copy".to_string()),
        ],
    )
}

fn pack_outer_env(conda_prefix: &Path, packed_env_path: &Path) -> Result<()> {
    if let Some(parent) = packed_env_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }

    run_command(
        "conda",
        &[
            "run".to_string(),
            "--prefix".to_string(),
            conda_prefix.display().to_string(),
            "conda-pack".to_string(),
            "--prefix".to_string(),
            conda_prefix.display().to_string(),
            "--force".to_string(),
            "--output".to_string(),
            packed_env_path.display().to_string(),
        ],
        None,
        &[],
    )
}

fn unpack_conda_env(packed_env_path: &Path, stage_dir: &Path) -> Result<()> {
    fs::create_dir_all(stage_dir).into_diagnostic()?;

    let tarball = File::open(packed_env_path).into_diagnostic()?;
    let gz = GzDecoder::new(tarball);
    let mut archive = Archive::new(gz);
    archive.unpack(stage_dir).into_diagnostic()?;
    Ok(())
}

fn write_launcher(plan: &BuildPlan, stage_dir: &Path, launcher_relpath: &Path) -> Result<()> {
    let launcher_path = stage_dir.join(launcher_relpath);
    if let Some(parent) = launcher_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }

    let inner_env = plan.inner_env_relative_path.to_string_lossy();
    let script = format!(
        "#!/usr/bin/env bash\n\
SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
ROOT_DIR=\"$(cd \"$SCRIPT_DIR/..\" && pwd)\"\n\
export PATH=\"$ROOT_DIR/{inner_env}/bin:$ROOT_DIR/bin:$PATH\"\n\
exec \"$ROOT_DIR/{inner_env}/bin/{entrypoint}\" \"$@\"\n",
        entrypoint = plan.entrypoint_name,
    );

    fs::write(&launcher_path, script).into_diagnostic()?;

    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&launcher_path)
            .into_diagnostic()?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&launcher_path, perms).into_diagnostic()?;
    }

    Ok(())
}

fn conda_python_spec(request: Option<&PythonRequest>) -> String {
    match request {
        None => "python".to_string(),
        Some(request) => match request.source {
            PythonRequestSource::DotPythonVersion | PythonRequestSource::Override => {
                format!("python={}", request.value)
            }
            PythonRequestSource::RequiresPython => {
                if request
                    .value
                    .starts_with(|c: char| matches!(c, '<' | '>' | '!' | '=' | '~'))
                {
                    format!("python{}", request.value)
                } else {
                    format!("python={}", request.value)
                }
            }
        },
    }
}

fn run_command(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    envs: &[(String, String)],
) -> Result<()> {
    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    for (key, value) in envs {
        command.env(key, value);
    }

    let status = command.status().into_diagnostic()?;
    if status.success() {
        return Ok(());
    }

    Err(miette!("command failed: {} {}", program, args.join(" ")))
}
