use crate::{
    build::{BuildPhase, BuildProgress, PrepareBuildOptions, prepare_build, run_phase},
    cli::BuildArgs,
    commands::doctor::require_host_prerequisites,
    packer::{PackOptions, pack_directory},
    plan::BuildPlan,
    project::{PythonRequest, PythonRequestSource, load_project_metadata},
};
use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use miette::{IntoDiagnostic, Result};
use std::{
    env, fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    time::Duration,
};

pub fn run(args: BuildArgs) -> Result<()> {
    let mut ui = BuildUi::new();
    require_host_prerequisites(&args.project)?;
    let metadata = load_project_metadata(&args.project, args.python.as_deref())?;
    let entrypoint_source = if args.entrypoint.is_some() {
        EntrypointSource::ExplicitOverride
    } else {
        EntrypointSource::AutoDetected
    };
    let plan = BuildPlan::resolve(metadata, args.entrypoint.as_deref())?;
    let output_path = resolve_output_path(&plan, args.output.as_deref())?;
    ui.print_build_header(&plan, &output_path, entrypoint_source);
    let prepared = prepare_build(
        &plan,
        &PrepareBuildOptions {
            work_dir: args.work_dir.clone(),
        },
        &mut ui,
    )?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    let exec_relpath = prepared.launcher_relpath.to_string_lossy().to_string();
    let manifest = run_phase(&mut ui, BuildPhase::AssembleExecutable, || {
        pack_directory(
            &prepared.stage_dir,
            &exec_relpath,
            &output_path,
            &PackOptions {
                stub_path: None,
                unique_id: true,
            },
        )
    })?;
    ui.finish();

    let packaged_entry_shim = prepared.stage_dir.join(&prepared.launcher_relpath);

    println!();
    println!("{}", heading_done("Done"));
    println!(
        "  {} {}",
        label("Final executable:"),
        primary_path(&output_path)
    );
    println!("  {} {}", label("Run it:"), path_value(&output_path));
    println!("  {} {}", label("Build id:"), value(&manifest.build_uid));
    println!(
        "  {} {}",
        label("Logs directory:"),
        path_value(&prepared.logs_dir)
    );

    println!();
    println!("{}", heading_debug("Debug paths"));
    println!(
        "  {} {}",
        label("Work directory:"),
        path_value(&prepared.work_dir)
    );
    println!(
        "  {} {}",
        label("Conda build prefix:"),
        path_value(&prepared.conda_prefix)
    );
    println!(
        "  {} {}",
        label("Inner uv env:"),
        path_value(&prepared.inner_env_path)
    );
    println!(
        "  {} {}",
        label("Packed conda archive:"),
        path_value(&prepared.packed_env_path)
    );
    println!(
        "  {} {}",
        label("Staging directory:"),
        path_value(&prepared.stage_dir)
    );
    println!(
        "  {} {}",
        label("Packaged entry shim:"),
        path_value(&packaged_entry_shim)
    );

    Ok(())
}

struct BuildUi {
    mode: ProgressMode,
    current_step: usize,
    total_steps: usize,
}

enum ProgressMode {
    Spinner(ProgressBar),
    Plain,
}

impl BuildUi {
    fn new() -> Self {
        let mode = if spinner_supported() {
            let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            spinner.set_style(
                ProgressStyle::with_template("[{pos}/{len}] {msg} {spinner}")
                    .expect("spinner template"),
            );
            spinner.set_length(BuildPhase::ALL_PHASES.len() as u64);
            spinner.enable_steady_tick(Duration::from_millis(100));
            ProgressMode::Spinner(spinner)
        } else {
            ProgressMode::Plain
        };

        Self {
            mode,
            current_step: 0,
            total_steps: BuildPhase::ALL_PHASES.len(),
        }
    }

    fn print_build_header(
        &mut self,
        plan: &BuildPlan,
        output_path: &Path,
        entrypoint_source: EntrypointSource,
    ) {
        self.println(&heading("Build plan"));
        self.println(&format!(
            "  {} {}",
            label("Project root:"),
            path_value(&plan.project_root)
        ));
        self.println(&format!(
            "  {} {}",
            label("Package:"),
            value(&plan.package_name)
        ));
        self.println(&format!(
            "  {} {}",
            label("Final executable:"),
            primary_path(output_path)
        ));
        self.println(&format!(
            "  {} {} -> {} ({})",
            label("Entrypoint:"),
            value(&plan.entrypoint_name),
            value(&plan.entrypoint_target),
            subtle(entrypoint_source.description())
        ));
        self.println(&format!(
            "  {} {}",
            label("Python request:"),
            plan.python_request
                .as_ref()
                .map(format_python_request)
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    fn finish(&mut self) {
        if let ProgressMode::Spinner(spinner) = &self.mode {
            spinner.finish_and_clear();
        }
    }

    fn println(&self, line: &str) {
        match &self.mode {
            ProgressMode::Spinner(spinner) => spinner.println(line.to_string()),
            ProgressMode::Plain => eprintln!("{line}"),
        }
    }

    fn step_prefix(&self) -> String {
        format!("[{}/{}]", self.current_step, self.total_steps)
    }
}

impl BuildProgress for BuildUi {
    fn on_layout_ready(
        &mut self,
        _plan: &BuildPlan,
        work_dir: &Path,
        logs_dir: &Path,
        _conda_prefix: &Path,
        _inner_env_path: &Path,
    ) {
        self.println(&format!(
            "  {} {}",
            label("Work directory:"),
            path_value(work_dir)
        ));
        self.println(&format!(
            "  {} {}",
            label("Logs directory:"),
            path_value(logs_dir)
        ));
        self.println("");
        self.println(&heading("Progress"));
    }

    fn on_phase_start(&mut self, phase: BuildPhase) {
        self.current_step += 1;
        match &self.mode {
            ProgressMode::Spinner(spinner) => {
                spinner.set_position(self.current_step as u64);
                spinner.set_message(phase.start_message().to_string());
            }
            ProgressMode::Plain => {
                let message = format!(
                    "{} {}",
                    subtle(&self.step_prefix()),
                    value(phase.start_message())
                );
                eprintln!("{message}...");
            }
        }
    }

    fn on_phase_complete(&mut self, phase: BuildPhase, elapsed: Duration) {
        let message = format!(
            "{} {} ({})",
            subtle(&self.step_prefix()),
            value(phase.success_message()),
            subtle(&format_elapsed(elapsed))
        );
        match &self.mode {
            ProgressMode::Spinner(spinner) => spinner.println(message),
            ProgressMode::Plain => eprintln!("{message}"),
        }
    }

    fn on_phase_failed(&mut self, phase: BuildPhase, elapsed: Duration, error: &miette::Report) {
        let message = format!(
            "{} {} {}: {}",
            subtle(&self.step_prefix()),
            danger(&format!("{} failed", phase.title())),
            subtle(&format!("after {}", format_elapsed(elapsed))),
            error
        );
        match &self.mode {
            ProgressMode::Spinner(spinner) => spinner.abandon_with_message(message),
            ProgressMode::Plain => eprintln!("{message}"),
        }
    }
}

fn spinner_supported() -> bool {
    interactive_ui_enabled()
}

fn format_elapsed(elapsed: Duration) -> String {
    if elapsed.as_secs() >= 60 {
        format!("{:.1}m", elapsed.as_secs_f64() / 60.0)
    } else if elapsed.as_secs() >= 1 {
        format!("{:.1}s", elapsed.as_secs_f64())
    } else {
        format!("{}ms", elapsed.as_millis())
    }
}

#[derive(Clone, Copy)]
enum EntrypointSource {
    AutoDetected,
    ExplicitOverride,
}

impl EntrypointSource {
    fn description(self) -> &'static str {
        match self {
            EntrypointSource::AutoDetected => "auto-detected from `[project.scripts]`",
            EntrypointSource::ExplicitOverride => "set explicitly with `--entrypoint`",
        }
    }
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

fn heading_debug(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).bold().dim().to_string()
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

fn danger(text: &str) -> String {
    if interactive_ui_enabled() {
        style(text).red().bold().to_string()
    } else {
        text.to_string()
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
