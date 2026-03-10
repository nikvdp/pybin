use miette::{IntoDiagnostic, Result, miette};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectMetadata {
    pub project_root: PathBuf,
    pub package_name: String,
    pub project_scripts: BTreeMap<String, String>,
    pub python_request: Option<PythonRequest>,
    pub uv_lock_present: bool,
}

#[derive(Debug, Clone)]
pub struct PythonRequest {
    pub value: String,
    pub source: PythonRequestSource,
}

#[derive(Debug, Clone, Copy)]
pub enum PythonRequestSource {
    Override,
    DotPythonVersion,
    DotVenv,
    RequiresPython,
}

#[derive(Debug, Deserialize)]
struct PyProjectToml {
    project: Option<ProjectSection>,
}

#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: String,
    #[serde(rename = "requires-python")]
    requires_python: Option<String>,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

pub fn load_project_metadata(
    project_root: impl AsRef<Path>,
    python_override: Option<&str>,
) -> Result<ProjectMetadata> {
    let project_root = project_root.as_ref();
    let manifest_path = project_root.join("pyproject.toml");
    let manifest_contents = fs::read_to_string(&manifest_path).into_diagnostic()?;
    let manifest: PyProjectToml = toml::from_str(&manifest_contents).into_diagnostic()?;
    let project = manifest.project.ok_or_else(|| {
        miette!(
            "`{}` is missing a `[project]` table",
            manifest_path.display()
        )
    })?;

    Ok(ProjectMetadata {
        project_root: project_root.to_path_buf(),
        package_name: project.name,
        project_scripts: project.scripts,
        python_request: resolve_python_request(
            project_root,
            python_override,
            project.requires_python,
        )?,
        uv_lock_present: project_root.join("uv.lock").is_file(),
    })
}

fn resolve_python_request(
    project_root: &Path,
    python_override: Option<&str>,
    requires_python: Option<String>,
) -> Result<Option<PythonRequest>> {
    if let Some(value) = python_override {
        return Ok(Some(PythonRequest {
            value: value.to_string(),
            source: PythonRequestSource::Override,
        }));
    }

    if let Some(value) = read_python_version_file(project_root)? {
        return Ok(Some(PythonRequest {
            value,
            source: PythonRequestSource::DotPythonVersion,
        }));
    }

    if let Some(value) = read_project_venv_python_version(project_root)? {
        return Ok(Some(PythonRequest {
            value,
            source: PythonRequestSource::DotVenv,
        }));
    }

    Ok(requires_python.map(|value| PythonRequest {
        value,
        source: PythonRequestSource::RequiresPython,
    }))
}

fn read_python_version_file(project_root: &Path) -> Result<Option<String>> {
    let path = project_root.join(".python-version");
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).into_diagnostic()?;
    let request = contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            miette!(
                "`{}` exists but does not contain a Python request",
                path.display()
            )
        })?;

    Ok(Some(request))
}

fn read_project_venv_python_version(project_root: &Path) -> Result<Option<String>> {
    let path = project_root.join(".venv").join("pyvenv.cfg");
    if !path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).into_diagnostic()?;
    for line in contents.lines().map(str::trim) {
        if let Some(value) = line
            .strip_prefix("version_info =")
            .or_else(|| line.strip_prefix("version ="))
        {
            let version = value.trim();
            if !version.is_empty() {
                return Ok(Some(version.to_string()));
            }
        }
    }

    Err(miette!(
        "`{}` exists but does not declare a Python version",
        path.display()
    ))
}
