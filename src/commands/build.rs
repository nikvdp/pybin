use crate::{
    build::{PrepareBuildOptions, prepare_build},
    cli::BuildArgs,
    packer::{PackOptions, pack_directory},
    plan::BuildPlan,
    project::load_project_metadata,
};
use miette::{IntoDiagnostic, Result};
use std::{
    fs,
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
            stub_path: None,
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
