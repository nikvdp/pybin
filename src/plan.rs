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
    pub entrypoint_name: String,
    pub entrypoint_target: String,
    pub uv_lock_present: bool,
    pub inner_env_relative_path: PathBuf,
}

impl BuildPlan {
    pub fn resolve(metadata: ProjectMetadata, entrypoint_override: Option<&str>) -> Result<Self> {
        let (entrypoint_name, entrypoint_target) =
            select_entrypoint(&metadata, entrypoint_override)?;

        Ok(Self {
            project_root: metadata.project_root,
            package_name: metadata.package_name,
            python_request: metadata.python_request,
            metadata_source: metadata.metadata_source,
            entrypoint_name,
            entrypoint_target,
            uv_lock_present: metadata.uv_lock_present,
            inner_env_relative_path: PathBuf::from(DEFAULT_INNER_ENV_NAME),
        })
    }

    pub fn inner_env_path_for<P: AsRef<Path>>(&self, conda_prefix: P) -> PathBuf {
        conda_prefix.as_ref().join(&self.inner_env_relative_path)
    }
}

fn select_entrypoint(
    metadata: &ProjectMetadata,
    entrypoint_override: Option<&str>,
) -> Result<(String, String)> {
    if let Some(name) = entrypoint_override {
        if let Some(target) = metadata.project_scripts.get(name) {
            return Ok((name.to_string(), target.clone()));
        }

        return Err(miette!(
            "requested entrypoint `{name}` was not found in `[project.scripts]`"
        ));
    }

    match metadata.project_scripts.len() {
        0 => Err(miette!(
            "project is not packable yet because `[project.scripts]` is empty; pass `--entrypoint` once additional entrypoint modes exist"
        )),
        1 => metadata
            .project_scripts
            .iter()
            .next()
            .map(|(name, target)| (name.clone(), target.clone()))
            .ok_or_else(|| miette!("failed to resolve the only project script")),
        _ => Err(miette!(
            "project defines multiple scripts; rerun with `--entrypoint <name>` to choose one"
        )),
    }
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
        let plan = BuildPlan::resolve(metadata, None).expect("plan");

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
        let error = BuildPlan::resolve(metadata, None).expect_err("should fail");

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
        let plan = BuildPlan::resolve(metadata, Some("admin")).expect("plan");

        assert_eq!(plan.entrypoint_name, "admin");
        assert_eq!(plan.entrypoint_target, "demo.admin:main");
        assert_eq!(
            plan.python_request
                .as_ref()
                .map(|request| request.value.as_str()),
            Some(">=3.12,<3.13")
        );
    }
}
