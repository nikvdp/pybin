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
    pub metadata_source: ProjectMetadataSource,
}

pub fn supported_project_markers(project_root: &Path) -> Vec<PathBuf> {
    ["pyproject.toml", "setup.py", "requirements.txt"]
        .into_iter()
        .map(|name| project_root.join(name))
        .filter(|path| path.is_file())
        .collect()
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
    PoetryDependency,
    SetupPyPythonRequires,
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectMetadataSource {
    Pep621Project,
    Poetry,
    SetupPy,
    RequirementsTxt,
}

impl ProjectMetadataSource {
    pub const fn description(self) -> &'static str {
        match self {
            Self::Pep621Project => "[project] in pyproject.toml",
            Self::Poetry => "[tool.poetry] in pyproject.toml",
            Self::SetupPy => "setup.py fallback",
            Self::RequirementsTxt => "requirements.txt fallback",
        }
    }
}

#[derive(Debug, Deserialize)]
struct PyProjectToml {
    project: Option<ProjectSection>,
    tool: Option<ToolSection>,
}

#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: String,
    #[serde(rename = "requires-python")]
    requires_python: Option<String>,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ToolSection {
    poetry: Option<PoetrySection>,
}

#[derive(Debug, Deserialize)]
struct PoetrySection {
    name: String,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
    dependencies: Option<BTreeMap<String, toml::Value>>,
}

pub fn load_project_metadata(
    project_root: impl AsRef<Path>,
    python_override: Option<&str>,
) -> Result<ProjectMetadata> {
    let project_root = project_root.as_ref();
    let manifest_path = project_root.join("pyproject.toml");
    let manifest = if manifest_path.is_file() {
        Some(
            toml::from_str::<PyProjectToml>(&fs::read_to_string(&manifest_path).into_diagnostic()?)
                .into_diagnostic()?,
        )
    } else {
        None
    };
    let uv_lock_present = project_root.join("uv.lock").is_file();

    if let Some(project) = manifest
        .as_ref()
        .and_then(|manifest| manifest.project.as_ref())
    {
        return Ok(ProjectMetadata {
            project_root: project_root.to_path_buf(),
            package_name: project.name.clone(),
            project_scripts: project.scripts.clone(),
            python_request: resolve_python_request(
                project_root,
                python_override,
                project
                    .requires_python
                    .clone()
                    .map(|value| ManifestPythonRequest {
                        value,
                        source: PythonRequestSource::RequiresPython,
                    }),
            )?,
            uv_lock_present,
            metadata_source: ProjectMetadataSource::Pep621Project,
        });
    }

    if let Some(poetry) = manifest
        .as_ref()
        .and_then(|manifest| manifest.tool.as_ref())
        .and_then(|tool| tool.poetry.as_ref())
    {
        return Ok(ProjectMetadata {
            project_root: project_root.to_path_buf(),
            package_name: poetry.name.clone(),
            project_scripts: poetry.scripts.clone(),
            python_request: resolve_python_request(
                project_root,
                python_override,
                poetry_python_requirement(&poetry.dependencies),
            )?,
            uv_lock_present,
            metadata_source: ProjectMetadataSource::Poetry,
        });
    }

    if let Some(setup_metadata) = load_setup_py_metadata(project_root)? {
        return Ok(ProjectMetadata {
            project_root: project_root.to_path_buf(),
            package_name: setup_metadata.name,
            project_scripts: setup_metadata.scripts,
            python_request: resolve_python_request(
                project_root,
                python_override,
                setup_metadata.python_requires,
            )?,
            uv_lock_present,
            metadata_source: ProjectMetadataSource::SetupPy,
        });
    }

    if project_root.join("requirements.txt").is_file() {
        let package_name = project_root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("python-app")
            .to_string();

        return Ok(ProjectMetadata {
            project_root: project_root.to_path_buf(),
            package_name,
            project_scripts: BTreeMap::new(),
            python_request: resolve_python_request(project_root, python_override, None)?,
            uv_lock_present,
            metadata_source: ProjectMetadataSource::RequirementsTxt,
        });
    }

    Err(miette!(
        "project root `{}` is missing supported project metadata; expected `[project]`, `[tool.poetry]`, `setup.py`, or `requirements.txt`",
        manifest_path.display()
    ))
}

fn poetry_python_requirement(
    dependencies: &Option<BTreeMap<String, toml::Value>>,
) -> Option<ManifestPythonRequest> {
    dependencies
        .as_ref()
        .and_then(|deps| deps.get("python"))
        .and_then(toml_value_to_string)
        .map(|value| ManifestPythonRequest {
            value,
            source: PythonRequestSource::PoetryDependency,
        })
}

fn toml_value_to_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

#[derive(Debug)]
struct SetupPyMetadata {
    name: String,
    python_requires: Option<ManifestPythonRequest>,
    scripts: BTreeMap<String, String>,
}

fn load_setup_py_metadata(project_root: &Path) -> Result<Option<SetupPyMetadata>> {
    let setup_path = project_root.join("setup.py");
    if !setup_path.is_file() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&setup_path).into_diagnostic()?;
    let name = extract_setup_keyword_string(&contents, "name");
    let python_requires = extract_setup_keyword_string(&contents, "python_requires").map(|value| {
        ManifestPythonRequest {
            value,
            source: PythonRequestSource::SetupPyPythonRequires,
        }
    });
    let scripts = extract_console_scripts(&contents);

    let Some(name) = name else {
        return Ok(None);
    };

    Ok(Some(SetupPyMetadata {
        name,
        python_requires,
        scripts,
    }))
}

fn extract_setup_keyword_string(contents: &str, key: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{key}={quote}");
        if let Some(start) = contents.find(&pattern) {
            let value_start = start + pattern.len();
            if let Some(end) = contents[value_start..].find(quote) {
                return Some(contents[value_start..value_start + end].to_string());
            }
        }
    }

    None
}

fn extract_console_scripts(contents: &str) -> BTreeMap<String, String> {
    let Some(console_scripts_start) = contents.find("console_scripts") else {
        return BTreeMap::new();
    };
    let Some(list_start_offset) = contents[console_scripts_start..].find('[') else {
        return BTreeMap::new();
    };
    let list_start = console_scripts_start + list_start_offset + 1;
    let Some(list_end_offset) = contents[list_start..].find(']') else {
        return BTreeMap::new();
    };
    let list_body = &contents[list_start..list_start + list_end_offset];

    let mut scripts = BTreeMap::new();
    for raw_entry in list_body.split(',') {
        let entry = raw_entry.trim().trim_matches('"').trim_matches('\'');
        let Some((name, target)) = entry.split_once('=') else {
            continue;
        };
        let script_name = name.trim();
        let script_target = target.trim();
        if !script_name.is_empty() && !script_target.is_empty() {
            scripts.insert(script_name.to_string(), script_target.to_string());
        }
    }

    scripts
}

#[derive(Debug)]
struct ManifestPythonRequest {
    value: String,
    source: PythonRequestSource,
}

fn resolve_python_request(
    project_root: &Path,
    python_override: Option<&str>,
    manifest_python_request: Option<ManifestPythonRequest>,
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

    Ok(manifest_python_request.map(|request| PythonRequest {
        value: request.value,
        source: request.source,
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
