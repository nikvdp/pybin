use crate::{
    cli::InspectArgs,
    plan::BuildPlan,
    project::{PythonRequestSource, load_project_metadata},
};
use miette::Result;

pub fn run(args: InspectArgs) -> Result<()> {
    let metadata = load_project_metadata(&args.project, args.python.as_deref())?;
    let plan = BuildPlan::resolve(metadata, args.entrypoint.as_deref())?;

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

    Ok(())
}

fn format_python_request(request: &crate::project::PythonRequest) -> String {
    let source = match request.source {
        PythonRequestSource::Override => "override",
        PythonRequestSource::DotPythonVersion => ".python-version",
        PythonRequestSource::RequiresPython => "project.requires-python",
    };

    format!("{} ({source})", request.value)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
