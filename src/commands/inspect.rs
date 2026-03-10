use crate::{
    cli::InspectArgs,
    commands::doctor::check_conda,
    plan::BuildPlan,
    project::{
        PythonRequest, PythonRequestSource, load_project_metadata, supported_project_markers,
    },
};
use console::style;
use miette::{Result, miette};
use std::{
    env,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

pub fn run(args: InspectArgs) -> Result<()> {
    let entrypoint_source = if args.entrypoint.is_some() {
        "set explicitly with `--entrypoint`"
    } else {
        "auto-detected from project metadata"
    };
    let conda_check = check_conda();
    let plan_result = load_project_metadata(&args.project, args.python.as_deref())
        .and_then(|metadata| BuildPlan::resolve(metadata, args.entrypoint.as_deref()));

    println!("{}", heading("Resolved build plan"));
    match &plan_result {
        Ok(plan) => {
            let final_executable = resolve_output_path(plan);
            println!(
                "  {} {}",
                label("Project root:"),
                path_value(&plan.project_root)
            );
            println!("  {} {}", label("Package:"), value(&plan.package_name));
            println!(
                "  {} {}",
                label("Metadata source:"),
                value(plan.metadata_source.description())
            );
            println!(
                "  {} {}",
                label("Final executable:"),
                primary_path(&final_executable)
            );
            println!(
                "  {} {} -> {} ({})",
                label("Entrypoint:"),
                value(&plan.entrypoint_name),
                value(&plan.entrypoint_target),
                subtle(entrypoint_source)
            );
            println!(
                "  {} {}",
                label("Python request:"),
                plan.python_request
                    .as_ref()
                    .map(format_python_request)
                    .unwrap_or_else(|| "<none>".to_string())
            );
            println!(
                "  {} {}",
                label("Install mode:"),
                value(&plan.install_strategy.description())
            );
            println!(
                "  {} {}",
                label("uv.lock present:"),
                value(yes_no(plan.uv_lock_present))
            );
            println!(
                "  {} {}",
                label("Inner uv env:"),
                value(&format!(
                    "<conda-prefix>/{}",
                    plan.inner_env_relative_path.display()
                ))
            );
            println!(
                "  {} {}",
                label("Release shape:"),
                value("single pybin binary used as the SFX stub")
            );
        }
        Err(error) => {
            println!("  {} {}", label("Project root:"), path_value(&args.project));
            println!(
                "  {} {}",
                label("Status:"),
                danger("could not resolve a packable build plan")
            );
            println!("  {} {}", label("Reason:"), subtle(&error.to_string()));
        }
    }

    println!();
    println!("{}", heading("Host checks"));
    match &conda_check {
        Ok(conda) => {
            println!("  {} {}", ok_badge(), value("conda available"));
            println!("     {}", subtle(&conda.version_line));
        }
        Err(error) => {
            println!("  {} {}", fail_badge(), value("conda unavailable"));
            println!("     {}", subtle(&error.to_string()));
        }
    }

    let project_markers = supported_project_markers(&args.project);
    if !project_markers.is_empty() {
        println!(
            "  {} {}",
            ok_badge(),
            value("supported project marker found")
        );
        for marker in project_markers {
            println!("     {}", subtle(&marker.display().to_string()));
        }
    } else {
        println!(
            "  {} {}",
            fail_badge(),
            value("supported project marker missing")
        );
        println!(
            "     {}",
            subtle(&format!(
                "expected one of `{}`, `{}`, or `{}`",
                args.project.join("pyproject.toml").display(),
                args.project.join("setup.py").display(),
                args.project.join("requirements.txt").display()
            ))
        );
    }

    println!();
    let packable = conda_check.is_ok() && plan_result.is_ok();
    println!("{}", heading_done("Packable"));
    println!(
        "  {} {}",
        label("Status:"),
        if packable {
            success("yes")
        } else {
            danger("no")
        }
    );

    if packable {
        Ok(())
    } else {
        let reason = match (conda_check.err(), plan_result.err()) {
            (Some(error), Some(plan_error)) => format!("{plan_error}; {error}"),
            (Some(error), None) => error.to_string(),
            (None, Some(plan_error)) => plan_error.to_string(),
            (None, None) => "unknown packability failure".to_string(),
        };
        Err(miette!("project is not packable: {reason}"))
    }
}

fn format_python_request(request: &PythonRequest) -> String {
    let source = match request.source {
        PythonRequestSource::Override => "override",
        PythonRequestSource::DotPythonVersion => ".python-version",
        PythonRequestSource::DotVenv => ".venv/pyvenv.cfg",
        PythonRequestSource::RequiresPython => "project.requires-python",
        PythonRequestSource::PoetryDependency => "tool.poetry.dependencies.python",
        PythonRequestSource::SetupPyPythonRequires => "setup.py python_requires",
    };

    format!("{} ({source})", request.value)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn resolve_output_path(plan: &BuildPlan) -> PathBuf {
    let mut filename = plan.entrypoint_name.clone();
    if cfg!(windows) {
        filename.push_str(".exe");
    }

    plan.project_root.join("dist").join(filename)
}

fn heading(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).bold().to_string()
    } else {
        text.to_string()
    }
}

fn heading_done(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).green().bold().to_string()
    } else {
        text.to_string()
    }
}

fn label(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).dim().to_string()
    } else {
        text.to_string()
    }
}

fn value(text: &str) -> String {
    text.to_string()
}

fn subtle(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).dim().to_string()
    } else {
        text.to_string()
    }
}

fn success(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).green().bold().to_string()
    } else {
        text.to_string()
    }
}

fn danger(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).red().bold().to_string()
    } else {
        text.to_string()
    }
}

fn ok_badge() -> String {
    if interactive_ui_enabled() {
        style("[ok]").green().bold().to_string()
    } else {
        "[ok]".to_string()
    }
}

fn fail_badge() -> String {
    if interactive_ui_enabled() {
        style("[fail]").red().bold().to_string()
    } else {
        "[fail]".to_string()
    }
}

fn path_value(path: &Path) -> String {
    let value = path.display().to_string();
    if interactive_ui_enabled() {
        style(value).cyan().to_string()
    } else {
        value
    }
}

fn primary_path(path: &Path) -> String {
    let value = path.display().to_string();
    if interactive_ui_enabled() {
        style(value).green().bold().to_string()
    } else {
        value
    }
}

fn interactive_ui_enabled() -> bool {
    io::stdout().is_terminal()
        && io::stderr().is_terminal()
        && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}
