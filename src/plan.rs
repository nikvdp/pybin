use crate::project::{ProjectMetadata, ProjectMetadataSource, PythonRequest};
use miette::{Result, miette};
use std::path::{Path, PathBuf};

pub const DEFAULT_INNER_ENV_NAME: &str = "uv-env";

#[derive(Debug, Clone)]
pub struct BuildPlan {
    pub project_root: PathBuf,
    pub package_name: String,
    pub python_request: Option<PythonRequest>,
    pub metadata_source: ProjectMetadataSource,
    pub install_strategy: InstallStrategy,
    pub entrypoint_name: String,
    pub entrypoint_target: String,
    pub source_overlay: Option<SourceOverlay>,
    pub uv_lock_present: bool,
    pub inner_env_relative_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum InstallStrategy {
    UvSync { frozen: bool },
    UvPipInstallProject,
    UvPipInstallRequirements { relative_path: PathBuf },
    CustomCommand { command: String },
}

#[derive(Debug, Clone)]
pub struct SourceOverlay {
    pub module_root: String,
    pub relative_source_path: PathBuf,
}

impl InstallStrategy {
    pub fn description(&self) -> String {
        match self {
            Self::UvSync { frozen: true } => "uv sync --frozen".to_string(),
            Self::UvSync { frozen: false } => "uv sync".to_string(),
            Self::UvPipInstallProject => "uv pip install .".to_string(),
            Self::UvPipInstallRequirements { relative_path } => {
                format!("uv pip install -r {}", relative_path.display())
            }
            Self::CustomCommand { command } => format!("custom install command: {command}"),
        }
    }
}

impl BuildPlan {
    pub fn resolve(
        metadata: ProjectMetadata,
        entrypoint_override: Option<&str>,
        install_command_override: Option<&str>,
    ) -> Result<Self> {
        let (entrypoint_name, entrypoint_target, source_overlay) =
            select_entrypoint(&metadata, entrypoint_override)?;
        let install_strategy = select_install_strategy(&metadata, install_command_override);

        Ok(Self {
            project_root: metadata.project_root,
            package_name: metadata.package_name,
            python_request: metadata.python_request,
            metadata_source: metadata.metadata_source,
            install_strategy,
            entrypoint_name,
            entrypoint_target,
            source_overlay,
            uv_lock_present: metadata.uv_lock_present,
            inner_env_relative_path: PathBuf::from(DEFAULT_INNER_ENV_NAME),
        })
    }

    pub fn inner_env_path_for<P: AsRef<Path>>(&self, conda_prefix: P) -> PathBuf {
        conda_prefix.as_ref().join(&self.inner_env_relative_path)
    }
}

fn select_install_strategy(
    metadata: &ProjectMetadata,
    install_command_override: Option<&str>,
) -> InstallStrategy {
    if let Some(command) = install_command_override {
        return InstallStrategy::CustomCommand {
            command: command.to_string(),
        };
    }

    if metadata.uv_lock_present {
        return InstallStrategy::UvSync { frozen: true };
    }

    if metadata.project_root.join("pyproject.toml").is_file()
        || metadata.project_root.join("setup.py").is_file()
        || metadata.project_root.join("setup.cfg").is_file()
    {
        return InstallStrategy::UvPipInstallProject;
    }

    if metadata.project_root.join("requirements.txt").is_file() {
        return InstallStrategy::UvPipInstallRequirements {
            relative_path: PathBuf::from("requirements.txt"),
        };
    }

    InstallStrategy::UvPipInstallProject
}

fn select_entrypoint(
    metadata: &ProjectMetadata,
    entrypoint_override: Option<&str>,
) -> Result<(String, String, Option<SourceOverlay>)> {
    if let Some(name) = entrypoint_override {
        if let Some(target) = metadata.project_scripts.get(name) {
            return Ok((name.to_string(), target.clone(), None));
        }

        if let Some((entrypoint_name, entrypoint_target)) = parse_explicit_entrypoint(name) {
            let source_overlay = resolve_source_overlay(metadata, &entrypoint_target)?;
            return Ok((entrypoint_name, entrypoint_target, source_overlay));
        }

        return Err(miette!(
            "requested entrypoint `{name}` was not found in project metadata; use `--entrypoint name=module:function` for explicit entrypoints"
        ));
    }

    match metadata.project_scripts.len() {
        0 => Err(miette!(
            "project is not packable yet because no entrypoint was declared; pass `--entrypoint name=module:function` for metadata-less projects"
        )),
        1 => metadata
            .project_scripts
            .iter()
            .next()
            .map(|(name, target)| (name.clone(), target.clone(), None))
            .ok_or_else(|| miette!("failed to resolve the only project script")),
        _ => Err(miette!(
            "project defines multiple scripts; rerun with `--entrypoint <name>` to choose one"
        )),
    }
}

fn parse_explicit_entrypoint(value: &str) -> Option<(String, String)> {
    let (name, target) = value.split_once('=')?;
    let name = name.trim();
    let target = target.trim();
    if name.is_empty() || target.is_empty() || !target.contains(':') {
        return None;
    }

    Some((name.to_string(), target.to_string()))
}

fn resolve_source_overlay(
    metadata: &ProjectMetadata,
    entrypoint_target: &str,
) -> Result<Option<SourceOverlay>> {
    if !matches!(
        metadata.metadata_source,
        ProjectMetadataSource::RequirementsTxt
    ) {
        return Ok(None);
    }

    let module_root = entrypoint_target
        .split(':')
        .next()
        .and_then(|module| module.split('.').next())
        .filter(|module| !module.is_empty())
        .ok_or_else(|| {
            miette!("explicit entrypoint `{entrypoint_target}` is not a valid `module:function` reference")
        })?;

    for relative_path in [
        PathBuf::from(module_root),
        PathBuf::from("src").join(module_root),
        PathBuf::from(format!("{module_root}.py")),
        PathBuf::from("src").join(format!("{module_root}.py")),
    ] {
        if metadata.project_root.join(&relative_path).exists() {
            return Ok(Some(SourceOverlay {
                module_root: module_root.to_string(),
                relative_source_path: relative_path,
            }));
        }
    }

    Err(miette!(
        "explicit entrypoint `{entrypoint_target}` requires local source `{module_root}` but no matching file or package was found under `{}`",
        metadata.project_root.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::load_project_metadata;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_single_script_with_python_version_file() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[project]
name = "demo-app"
version = "0.1.0"
scripts = { demo = "demo.cli:main" }
"#,
        )
        .expect("write pyproject");
        fs::write(dir.path().join(".python-version"), "3.12.7\n").expect("write .python-version");

        let metadata = load_project_metadata(dir.path(), None).expect("metadata");
        let plan = BuildPlan::resolve(metadata, None, None).expect("plan");

        assert_eq!(plan.package_name, "demo-app");
        assert_eq!(plan.entrypoint_name, "demo");
        assert_eq!(plan.entrypoint_target, "demo.cli:main");
        assert_eq!(
            plan.python_request
                .as_ref()
                .map(|request| request.value.as_str()),
            Some("3.12.7")
        );
        assert_eq!(
            plan.inner_env_relative_path,
            PathBuf::from(DEFAULT_INNER_ENV_NAME)
        );
    }

    #[test]
    fn requires_entrypoint_override_for_multi_script_projects() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[project]
name = "demo-app"
version = "0.1.0"
scripts = { demo = "demo.cli:main", admin = "demo.admin:main" }
"#,
        )
        .expect("write pyproject");

        let metadata = load_project_metadata(dir.path(), None).expect("metadata");
        let error = BuildPlan::resolve(metadata, None, None).expect_err("should fail");

        assert!(error.to_string().contains("--entrypoint"));
    }

    #[test]
    fn accepts_explicit_entrypoint_override() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[project]
name = "demo-app"
version = "0.1.0"
scripts = { demo = "demo.cli:main", admin = "demo.admin:main" }
requires-python = ">=3.12,<3.13"
"#,
        )
        .expect("write pyproject");

        let metadata = load_project_metadata(dir.path(), None).expect("metadata");
        let plan = BuildPlan::resolve(metadata, Some("admin"), None).expect("plan");

        assert_eq!(plan.entrypoint_name, "admin");
        assert_eq!(plan.entrypoint_target, "demo.admin:main");
        assert_eq!(
            plan.python_request
                .as_ref()
                .map(|request| request.value.as_str()),
            Some(">=3.12,<3.13")
        );
    }

    #[test]
    fn selects_uv_sync_for_locked_projects() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[project]
name = "demo-app"
version = "0.1.0"
scripts = { demo = "demo.cli:main" }
"#,
        )
        .expect("write pyproject");
        fs::write(dir.path().join("uv.lock"), "version = 1\n").expect("write uv.lock");

        let metadata = load_project_metadata(dir.path(), None).expect("metadata");
        let plan = BuildPlan::resolve(metadata, None, None).expect("plan");

        assert!(matches!(
            plan.install_strategy,
            InstallStrategy::UvSync { frozen: true }
        ));
    }

    #[test]
    fn selects_uv_pip_install_project_for_unlocked_pyproject_projects() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"
[tool.poetry]
name = "legacy-poetry-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"

[tool.poetry.scripts]
legacy-poetry-app = "legacy.cli:main"
"#,
        )
        .expect("write pyproject");

        let metadata = load_project_metadata(dir.path(), None).expect("metadata");
        let plan = BuildPlan::resolve(metadata, None, None).expect("plan");

        assert!(matches!(
            plan.install_strategy,
            InstallStrategy::UvPipInstallProject
        ));
    }

    #[test]
    fn accepts_explicit_entrypoint_for_requirements_only_projects() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("requirements.txt"), "click>=8,<9\n")
            .expect("write requirements");
        fs::create_dir_all(dir.path().join("req_app")).expect("create package dir");
        fs::write(dir.path().join("req_app").join("__init__.py"), "").expect("write init");
        fs::write(
            dir.path().join("req_app").join("cli.py"),
            "def main():\n    return 0\n",
        )
        .expect("write cli");

        let metadata = load_project_metadata(dir.path(), Some("3.12")).expect("metadata");
        let plan =
            BuildPlan::resolve(metadata, Some("req-app=req_app.cli:main"), None).expect("plan");

        assert_eq!(plan.entrypoint_name, "req-app");
        assert_eq!(plan.entrypoint_target, "req_app.cli:main");
        assert!(matches!(
            plan.install_strategy,
            InstallStrategy::UvPipInstallRequirements { .. }
        ));
        let overlay = plan.source_overlay.expect("source overlay");
        assert_eq!(overlay.module_root, "req_app");
        assert_eq!(overlay.relative_source_path, PathBuf::from("req_app"));
    }
}
