use crate::{
    build::{PrepareBuildOptions, prepare_build},
    cli::BuildArgs,
    plan::BuildPlan,
    project::load_project_metadata,
};
use miette::Result;

pub fn run(args: BuildArgs) -> Result<()> {
    let metadata = load_project_metadata(&args.project, args.python.as_deref())?;
    let plan = BuildPlan::resolve(metadata, args.entrypoint.as_deref())?;
    let prepared = prepare_build(
        &plan,
        &PrepareBuildOptions {
            work_dir: args.work_dir.clone(),
        },
    )?;

    println!("Prepared staged build for `{}`", plan.package_name);
    println!("Work dir: {}", prepared.work_dir.display());
    println!("Conda prefix: {}", prepared.conda_prefix.display());
    println!("Inner uv env: {}", prepared.inner_env_path.display());
    println!("Packed env: {}", prepared.packed_env_path.display());
    println!("Stage dir: {}", prepared.stage_dir.display());
    println!("Launcher: {}", prepared.launcher_relpath.display());

    if let Some(output) = args.output {
        println!(
            "Final executable output is not wired yet; requested path was {}",
            output.display()
        );
    }

    Ok(())
}
