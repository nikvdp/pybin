use crate::plan::{BuildPlan, InstallStrategy};
use flate2::read::GzDecoder;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tar::Archive;
use tracing::info;

#[derive(Debug, Clone)]
pub struct PrepareBuildOptions {
    pub work_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum BuildPhase {
    CreateCondaPrefix,
    SyncUvProject,
    PackCondaPrefix,
    UnpackPackedPrefix,
    PruneStagedRuntime,
    StageProjectSource,
    WriteLauncher,
    AssembleExecutable,
}

impl BuildPhase {
    pub const PREPARE_PHASES: [BuildPhase; 7] = [
        BuildPhase::CreateCondaPrefix,
        BuildPhase::SyncUvProject,
        BuildPhase::PackCondaPrefix,
        BuildPhase::UnpackPackedPrefix,
        BuildPhase::PruneStagedRuntime,
        BuildPhase::StageProjectSource,
        BuildPhase::WriteLauncher,
    ];

    pub const ALL_PHASES: [BuildPhase; 8] = [
        BuildPhase::CreateCondaPrefix,
        BuildPhase::SyncUvProject,
        BuildPhase::PackCondaPrefix,
        BuildPhase::UnpackPackedPrefix,
        BuildPhase::PruneStagedRuntime,
        BuildPhase::StageProjectSource,
        BuildPhase::WriteLauncher,
        BuildPhase::AssembleExecutable,
    ];

    pub fn title(self) -> &'static str {
        match self {
            BuildPhase::CreateCondaPrefix => "Create conda build prefix",
            BuildPhase::SyncUvProject => "Install project into build environment",
            BuildPhase::PackCondaPrefix => "Pack relocatable conda prefix",
            BuildPhase::UnpackPackedPrefix => "Unpack staged runtime tree",
            BuildPhase::PruneStagedRuntime => "Prune non-runtime files from staged tree",
            BuildPhase::StageProjectSource => "Stage local project source overlay",
            BuildPhase::WriteLauncher => "Write packaged launcher shim",
            BuildPhase::AssembleExecutable => "Assemble self-extracting executable",
        }
    }

    pub fn start_message(self) -> &'static str {
        match self {
            BuildPhase::CreateCondaPrefix => {
                "Creating conda build prefix with Python, uv, and conda-pack"
            }
            BuildPhase::SyncUvProject => {
                "Creating the packaged uv environment and installing the app"
            }
            BuildPhase::PackCondaPrefix => "Packing the relocatable conda prefix",
            BuildPhase::UnpackPackedPrefix => "Unpacking the packed prefix into the staging area",
            BuildPhase::PruneStagedRuntime => "Pruning non-runtime files from the staged tree",
            BuildPhase::StageProjectSource => {
                "Staging local project source into the packaged runtime"
            }
            BuildPhase::WriteLauncher => "Writing the packaged entry shim",
            BuildPhase::AssembleExecutable => "Assembling the final self-extracting binary",
        }
    }

    pub fn success_message(self) -> &'static str {
        match self {
            BuildPhase::CreateCondaPrefix => "Created conda build prefix",
            BuildPhase::SyncUvProject => "Installed project into packaged runtime",
            BuildPhase::PackCondaPrefix => "Packed relocatable conda prefix",
            BuildPhase::UnpackPackedPrefix => "Unpacked staged runtime tree",
            BuildPhase::PruneStagedRuntime => "Pruned non-runtime files from staged tree",
            BuildPhase::StageProjectSource => "Staged local project source overlay",
            BuildPhase::WriteLauncher => "Wrote packaged entry shim",
            BuildPhase::AssembleExecutable => "Assembled self-extracting executable",
        }
    }
}

pub trait BuildProgress {
    fn on_layout_ready(
        &mut self,
        _plan: &BuildPlan,
        _work_dir: &Path,
        _logs_dir: &Path,
        _conda_prefix: &Path,
        _inner_env_path: &Path,
    ) {
    }

    fn on_phase_start(&mut self, _phase: BuildPhase) {}

    fn on_phase_complete(&mut self, _phase: BuildPhase, _elapsed: Duration) {}

    fn on_phase_failed(&mut self, _phase: BuildPhase, _elapsed: Duration, _error: &miette::Report) {
    }
}

pub struct SilentBuildProgress;

impl BuildProgress for SilentBuildProgress {}

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

pub fn prepare_build(
    plan: &BuildPlan,
    options: &PrepareBuildOptions,
    progress: &mut dyn BuildProgress,
) -> Result<PreparedBuild> {
    let paths = BuildPaths::create(plan, options)?;
    fs::create_dir_all(&paths.logs_dir).into_diagnostic()?;

    let python_spec = conda_python_spec(plan);
    let inner_env_path = plan.inner_env_path_for(&paths.conda_prefix);
    let conda_python = conda_python_path(&paths.conda_prefix);
    let launcher_relpath = PathBuf::from("bin").join(format!("pybin-{}", plan.entrypoint_name));

    progress.on_layout_ready(
        plan,
        &paths.work_dir,
        &paths.logs_dir,
        &paths.conda_prefix,
        &inner_env_path,
    );

    info!(
        project = %plan.project_root.display(),
        work_dir = %paths.work_dir.display(),
        conda_prefix = %paths.conda_prefix.display(),
        inner_env = %inner_env_path.display(),
        "preparing staged build",
    );

    run_phase(progress, BuildPhase::CreateCondaPrefix, || {
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
        )
    })?;

    run_phase(progress, BuildPhase::SyncUvProject, || {
        install_project(
            plan,
            &paths.logs_dir,
            &paths.conda_prefix,
            &conda_python,
            &inner_env_path,
        )
    })?;

    run_phase(progress, BuildPhase::PackCondaPrefix, || {
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
        )
    })?;

    run_phase(progress, BuildPhase::UnpackPackedPrefix, || {
        unpack_tarball(&paths.packed_env_path, &paths.stage_dir)
    })?;
    run_phase(progress, BuildPhase::PruneStagedRuntime, || {
        prune_staged_runtime(&paths.stage_dir, &paths.logs_dir)
    })?;
    run_phase(progress, BuildPhase::StageProjectSource, || {
        stage_source_overlay(plan, &paths.stage_dir)
    })?;
    run_phase(progress, BuildPhase::WriteLauncher, || {
        write_launcher(plan, &paths.stage_dir, &launcher_relpath)
    })?;

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

pub fn run_phase<T>(
    progress: &mut dyn BuildProgress,
    phase: BuildPhase,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    progress.on_phase_start(phase);
    let started = Instant::now();
    match action() {
        Ok(value) => {
            progress.on_phase_complete(phase, started.elapsed());
            Ok(value)
        }
        Err(error) => {
            progress.on_phase_failed(phase, started.elapsed(), &error);
            Err(error)
        }
    }
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
    } else if let Some(spec) = normalize_poetry_python_request(value) {
        format!("python{spec}")
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

fn normalize_poetry_python_request(value: &str) -> Option<String> {
    if let Some(version) = value.strip_prefix('^') {
        return expand_caret_requirement(version.trim());
    }

    if let Some(version) = value.strip_prefix('~') {
        return expand_tilde_requirement(version.trim());
    }

    None
}

fn expand_caret_requirement(version: &str) -> Option<String> {
    let segments = parse_version_segments(version)?;
    let upper = if segments.first().copied().unwrap_or(0) > 0 {
        vec![segments[0] + 1]
    } else if segments.get(1).copied().unwrap_or(0) > 0 {
        vec![0, segments[1] + 1]
    } else {
        vec![0, 0, segments.get(2).copied().unwrap_or(0) + 1]
    };

    Some(format!(
        ">={},<{}",
        format_version_segments(&segments),
        format_version_segments(&upper)
    ))
}

fn expand_tilde_requirement(version: &str) -> Option<String> {
    let segments = parse_version_segments(version)?;
    let upper = if segments.len() >= 2 {
        vec![segments[0], segments[1] + 1]
    } else {
        vec![segments[0] + 1]
    };

    Some(format!(
        ">={},<{}",
        format_version_segments(&segments),
        format_version_segments(&upper)
    ))
}

fn parse_version_segments(version: &str) -> Option<Vec<u64>> {
    let mut segments = Vec::new();
    for segment in version.split('.') {
        if segment.is_empty() {
            return None;
        }
        segments.push(segment.parse().ok()?);
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

fn format_version_segments(segments: &[u64]) -> String {
    segments
        .iter()
        .map(|segment| segment.to_string())
        .collect::<Vec<_>>()
        .join(".")
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

    let (entry_module, entry_attr) = parse_entrypoint_target(&plan.entrypoint_target)?;
    let inner_env = plan.inner_env_relative_path.to_string_lossy();
    let script = format!(
        "#!/usr/bin/env bash\n\
set -euo pipefail\n\
SCRIPT_DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
ROOT_DIR=\"$(cd \"$SCRIPT_DIR/..\" && pwd)\"\n\
export PATH=\"$ROOT_DIR/{inner_env}/bin:$ROOT_DIR/bin:$PATH\"\n\
if [ -d \"$ROOT_DIR/pybin-src\" ]; then\n\
  export PYTHONPATH=\"$ROOT_DIR/pybin-src${{PYTHONPATH:+:$PYTHONPATH}}\"\n\
fi\n\
PYBIN_ENTRYPOINT='import importlib\n\
import sys\n\
from functools import reduce\n\
script_name, module_name, attr_path = sys.argv[1:4]; sys.argv = [script_name] + sys.argv[4:]; obj = reduce(getattr, attr_path.split(\".\"), importlib.import_module(module_name)); raise SystemExit(obj())'\n\
exec \"$ROOT_DIR/{inner_env}/bin/python\" -c \"$PYBIN_ENTRYPOINT\" \"{entrypoint_name}\" \"{entry_module}\" \"{entry_attr}\" \"$@\"\n",
        entrypoint_name = plan.entrypoint_name,
        entry_module = entry_module,
        entry_attr = entry_attr,
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

fn stage_source_overlay(plan: &BuildPlan, stage_dir: &Path) -> Result<()> {
    let Some(source_overlay) = &plan.source_overlay else {
        return Ok(());
    };

    let source_path = plan.project_root.join(&source_overlay.relative_source_path);
    let destination = stage_dir
        .join("pybin-src")
        .join(&source_overlay.module_root);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }

    if source_path.is_dir() {
        copy_dir_all(&source_path, &destination)
    } else {
        let file_destination = if source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "py")
        {
            stage_dir.join("pybin-src").join(
                source_path
                    .file_name()
                    .ok_or_else(|| miette!("source overlay path had no filename"))?,
            )
        } else {
            destination
        };
        if let Some(parent) = file_destination.parent() {
            fs::create_dir_all(parent).into_diagnostic()?;
        }
        fs::copy(&source_path, &file_destination).into_diagnostic()?;
        Ok(())
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).into_diagnostic()?;
    for entry in fs::read_dir(source).into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let entry_type = entry.file_type().into_diagnostic()?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry_type.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).into_diagnostic()?;
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct RuntimePruneSummary {
    removed_entries: u64,
    removed_bytes: u64,
    removed_paths: Vec<String>,
}

impl RuntimePruneSummary {
    fn note_removed_path(&mut self, path: &Path, bytes: u64, reason: &str) {
        self.removed_entries += 1;
        self.removed_bytes += bytes;
        self.removed_paths.push(format!(
            "{reason}: {} ({})",
            path.display(),
            format_bytes(bytes)
        ));
    }
}

fn prune_staged_runtime(stage_dir: &Path, logs_dir: &Path) -> Result<()> {
    let mut summary = RuntimePruneSummary::default();
    walk_and_prune(stage_dir, &mut summary)?;
    write_prune_log(logs_dir, &summary)?;

    info!(
        removed_entries = summary.removed_entries,
        removed_bytes = summary.removed_bytes,
        "pruned non-runtime files from staged tree",
    );

    Ok(())
}

fn remove_path_if_present(
    path: &Path,
    reason: &str,
    summary: &mut RuntimePruneSummary,
) -> Result<()> {
    let Some((bytes, is_dir)) = path_size_and_kind(path)? else {
        return Ok(());
    };

    if is_dir {
        fs::remove_dir_all(path).into_diagnostic()?;
    } else {
        fs::remove_file(path).into_diagnostic()?;
    }
    summary.note_removed_path(path, bytes, reason);
    Ok(())
}

fn walk_and_prune(path: &Path, summary: &mut RuntimePruneSummary) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path).into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let child_path = entry.path();
        let file_type = entry.file_type().into_diagnostic()?;

        if file_type.is_dir() {
            if entry.file_name() == "__pycache__" {
                remove_path_if_present(&child_path, "__pycache__", summary)?;
                continue;
            }

            walk_and_prune(&child_path, summary)?;
            continue;
        }

        if should_prune_file(&child_path) {
            let reason = child_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!("*.{ext}"))
                .unwrap_or_else(|| "file".to_string());
            remove_path_if_present(&child_path, &reason, summary)?;
        }
    }

    Ok(())
}

fn should_prune_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("pyc" | "pyo")
    )
}

fn path_size_and_kind(path: &Path) -> Result<Option<(u64, bool)>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };

    if metadata.is_dir() {
        Ok(Some((directory_size(path)?, true)))
    } else {
        Ok(Some((metadata.len(), false)))
    }
}

fn directory_size(path: &Path) -> Result<u64> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path).into_diagnostic()? {
        let entry = entry.into_diagnostic()?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child).into_diagnostic()?;
        if metadata.is_dir() {
            total = total.saturating_add(directory_size(&child)?);
        } else {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn write_prune_log(logs_dir: &Path, summary: &RuntimePruneSummary) -> Result<()> {
    let log_path = logs_dir.join("prune-runtime.log");
    let mut body = String::new();
    body.push_str(&format!(
        "removed entries: {}\nremoved bytes: {} ({})\n",
        summary.removed_entries,
        summary.removed_bytes,
        format_bytes(summary.removed_bytes),
    ));

    if summary.removed_paths.is_empty() {
        body.push_str("\nremoved paths:\n  <none>\n");
    } else {
        body.push_str("\nremoved paths:\n");
        for entry in &summary.removed_paths {
            body.push_str(&format!("  {entry}\n"));
        }
    }

    fs::write(log_path, body).into_diagnostic()?;
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;

    if bytes as f64 >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn parse_entrypoint_target(target: &str) -> Result<(&str, &str)> {
    let (module, attr) = target.split_once(':').ok_or_else(|| {
        miette!("entrypoint target `{target}` is not a valid `module:function` reference")
    })?;

    if module.is_empty() || attr.is_empty() {
        return Err(miette!(
            "entrypoint target `{target}` is not a valid `module:function` reference"
        ));
    }

    Ok((module, attr))
}

fn install_project(
    plan: &BuildPlan,
    logs_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
) -> Result<()> {
    match &plan.install_strategy {
        InstallStrategy::UvSync { frozen } => install_with_uv_sync(
            plan,
            logs_dir,
            conda_prefix,
            conda_python,
            inner_env_path,
            *frozen,
        ),
        InstallStrategy::UvPipInstallProject => {
            install_with_uv_pip_project(plan, logs_dir, conda_prefix, conda_python, inner_env_path)
        }
        InstallStrategy::UvPipInstallRequirements { relative_path } => {
            install_with_uv_pip_requirements(
                plan,
                logs_dir,
                conda_prefix,
                conda_python,
                inner_env_path,
                relative_path,
            )
        }
        InstallStrategy::CustomCommand { command } => install_with_custom_command(
            plan,
            logs_dir,
            conda_prefix,
            conda_python,
            inner_env_path,
            command,
        ),
    }
}

fn install_with_uv_sync(
    plan: &BuildPlan,
    logs_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
    frozen: bool,
) -> Result<()> {
    let mut uv_sync_args = vec![
        OsString::from("run"),
        OsString::from("-p"),
        conda_prefix.as_os_str().to_os_string(),
        OsString::from("uv"),
        OsString::from("sync"),
        OsString::from("--no-editable"),
        OsString::from("--link-mode"),
        OsString::from("copy"),
        OsString::from("--python"),
        conda_python.as_os_str().to_os_string(),
    ];
    if frozen {
        uv_sync_args.push(OsString::from("--frozen"));
    }

    run_logged(
        "uv-sync",
        logs_dir,
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
    )
}

fn install_with_uv_pip_project(
    plan: &BuildPlan,
    logs_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
) -> Result<()> {
    create_inner_uv_env(
        logs_dir,
        &plan.project_root,
        conda_prefix,
        conda_python,
        inner_env_path,
    )?;
    let inner_python = conda_python_path(inner_env_path);

    run_logged(
        "uv-pip-install-project",
        logs_dir,
        &plan.project_root,
        "conda",
        &[
            OsString::from("run"),
            OsString::from("-p"),
            conda_prefix.as_os_str().to_os_string(),
            OsString::from("uv"),
            OsString::from("pip"),
            OsString::from("install"),
            OsString::from("--python"),
            inner_python.as_os_str().to_os_string(),
            OsString::from("--link-mode"),
            OsString::from("copy"),
            OsString::from("."),
        ],
        &[("UV_LINK_MODE", OsString::from("copy"))],
    )
}

fn install_with_uv_pip_requirements(
    plan: &BuildPlan,
    logs_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
    relative_path: &Path,
) -> Result<()> {
    create_inner_uv_env(
        logs_dir,
        &plan.project_root,
        conda_prefix,
        conda_python,
        inner_env_path,
    )?;
    let inner_python = conda_python_path(inner_env_path);

    run_logged(
        "uv-pip-install-requirements",
        logs_dir,
        &plan.project_root,
        "conda",
        &[
            OsString::from("run"),
            OsString::from("-p"),
            conda_prefix.as_os_str().to_os_string(),
            OsString::from("uv"),
            OsString::from("pip"),
            OsString::from("install"),
            OsString::from("--python"),
            inner_python.as_os_str().to_os_string(),
            OsString::from("--link-mode"),
            OsString::from("copy"),
            OsString::from("-r"),
            relative_path.as_os_str().to_os_string(),
        ],
        &[("UV_LINK_MODE", OsString::from("copy"))],
    )
}

fn create_inner_uv_env(
    logs_dir: &Path,
    current_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
) -> Result<()> {
    run_logged(
        "uv-venv",
        logs_dir,
        current_dir,
        "conda",
        &[
            OsString::from("run"),
            OsString::from("-p"),
            conda_prefix.as_os_str().to_os_string(),
            OsString::from("uv"),
            OsString::from("venv"),
            OsString::from("--python"),
            conda_python.as_os_str().to_os_string(),
            inner_env_path.as_os_str().to_os_string(),
        ],
        &[],
    )
}

fn install_with_custom_command(
    plan: &BuildPlan,
    logs_dir: &Path,
    conda_prefix: &Path,
    conda_python: &Path,
    inner_env_path: &Path,
    command: &str,
) -> Result<()> {
    create_inner_uv_env(
        logs_dir,
        &plan.project_root,
        conda_prefix,
        conda_python,
        inner_env_path,
    )?;
    let inner_python = conda_python_path(inner_env_path);
    let (shell_program, shell_flag) = shell_command_parts();

    run_logged(
        "custom-install-command",
        logs_dir,
        &plan.project_root,
        "conda",
        &[
            OsString::from("run"),
            OsString::from("-p"),
            conda_prefix.as_os_str().to_os_string(),
            OsString::from(shell_program),
            OsString::from(shell_flag),
            OsString::from(command),
        ],
        &[
            (
                "PYBIN_PROJECT_ROOT",
                plan.project_root.as_os_str().to_os_string(),
            ),
            (
                "PYBIN_CONDA_PREFIX",
                conda_prefix.as_os_str().to_os_string(),
            ),
            (
                "PYBIN_CONDA_PYTHON",
                conda_python.as_os_str().to_os_string(),
            ),
            ("PYBIN_UV_ENV", inner_env_path.as_os_str().to_os_string()),
            (
                "PYBIN_INNER_PYTHON",
                inner_python.as_os_str().to_os_string(),
            ),
            (
                "UV_PROJECT_ENVIRONMENT",
                inner_env_path.as_os_str().to_os_string(),
            ),
            ("UV_LINK_MODE", OsString::from("copy")),
        ],
    )
}

fn shell_command_parts() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-lc")
    }
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
    use crate::{
        plan::InstallStrategy,
        project::{ProjectMetadataSource, PythonRequest, PythonRequestSource},
    };

    #[test]
    fn formats_exact_python_versions_for_conda() {
        let plan = BuildPlan {
            project_root: PathBuf::from("."),
            package_name: "demo".to_string(),
            python_request: Some(PythonRequest {
                value: "3.12".to_string(),
                source: PythonRequestSource::DotPythonVersion,
            }),
            metadata_source: ProjectMetadataSource::Pep621Project,
            install_strategy: InstallStrategy::UvSync { frozen: true },
            entrypoint_name: "demo".to_string(),
            entrypoint_target: "demo.cli:main".to_string(),
            source_overlay: None,
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
            metadata_source: ProjectMetadataSource::Pep621Project,
            install_strategy: InstallStrategy::UvPipInstallProject,
            entrypoint_name: "demo".to_string(),
            entrypoint_target: "demo.cli:main".to_string(),
            source_overlay: None,
            uv_lock_present: false,
            inner_env_relative_path: PathBuf::from("uv-env"),
        };

        assert_eq!(conda_python_spec(&plan), "python>=3.12,<3.13");
    }

    #[test]
    fn normalizes_poetry_caret_python_requirements_for_conda() {
        let plan = BuildPlan {
            project_root: PathBuf::from("."),
            package_name: "demo".to_string(),
            python_request: Some(PythonRequest {
                value: "^3.7".to_string(),
                source: PythonRequestSource::PoetryDependency,
            }),
            metadata_source: ProjectMetadataSource::Poetry,
            install_strategy: InstallStrategy::UvPipInstallProject,
            entrypoint_name: "demo".to_string(),
            entrypoint_target: "demo.cli:main".to_string(),
            source_overlay: None,
            uv_lock_present: false,
            inner_env_relative_path: PathBuf::from("uv-env"),
        };

        assert_eq!(conda_python_spec(&plan), "python>=3.7,<4");
    }
}
