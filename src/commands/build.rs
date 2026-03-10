use crate::{
    build::{PrepareBuildOptions, prepare_build},
    cli::BuildArgs,
    packer::{PackOptions, pack_directory},
    plan::BuildPlan,
    project::load_project_metadata,
};
use miette::{IntoDiagnostic, Result, miette};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn run(args: BuildArgs) -> Result<()> {
    let metadata = load_project_metadata(&args.project, args.python.as_deref())?;
    let plan = BuildPlan::resolve(metadata, args.entrypoint.as_deref())?;
    let prepared = prepare_build(
        &plan,
        &PrepareBuildOptions {
            work_dir: args.work_dir.clone(),
        },
    )?;
    let runner_path = bundled_runner_path()?;
    let output_path = resolve_output_path(&plan, args.output.as_deref())?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    let exec_relpath = prepared.launcher_relpath.to_string_lossy().to_string();
    let manifest = pack_directory(
        &prepared.stage_dir,
        &exec_relpath,
        &output_path,
        &PackOptions {
            runner_path,
            unique_id: true,
        },
    )?;

    println!("Built `{}`", plan.package_name);
    println!("Output: {}", output_path.display());
    println!("Build id: {}", manifest.build_uid);
    println!("Work dir: {}", prepared.work_dir.display());
    println!("Logs dir: {}", prepared.logs_dir.display());
    println!("Conda prefix: {}", prepared.conda_prefix.display());
    println!("Inner uv env: {}", prepared.inner_env_path.display());
    println!("Packed env: {}", prepared.packed_env_path.display());
    println!("Stage dir: {}", prepared.stage_dir.display());
    println!("Launcher: {}", prepared.launcher_relpath.display());

    Ok(())
}

fn bundled_runner_path() -> Result<PathBuf> {
    let self_path = env::current_exe().into_diagnostic()?;
    let runner_name = if cfg!(windows) {
        "pybin-runner.exe"
    } else {
        "pybin-runner"
    };
    let runner_path = self_path.with_file_name(runner_name);

    if runner_path.is_file() {
        return Ok(runner_path);
    }

    Err(miette!(
        "could not find the internal runner binary at `{}`",
        runner_path.display()
    ))
}

fn resolve_output_path(plan: &BuildPlan, output_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = output_override {
        return Ok(path.to_path_buf());
    }

    let mut filename = plan.entrypoint_name.clone();
    if cfg!(windows) {
        filename.push_str(".exe");
    }

    Ok(plan.project_root.join("dist").join(filename))
}
