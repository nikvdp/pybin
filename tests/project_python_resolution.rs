use pybin::project::{ProjectMetadataSource, PythonRequestSource, load_project_metadata};
use std::fs;
use tempfile::tempdir;

#[test]
fn prefers_dot_venv_python_version_when_no_python_version_file_exists() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"
[project]
name = "demo-app"
version = "0.1.0"
scripts = { demo = "demo.cli:main" }
requires-python = ">=3.11"
"#,
    )
    .expect("write pyproject");
    fs::create_dir_all(dir.path().join(".venv")).expect("create .venv");
    fs::write(
        dir.path().join(".venv").join("pyvenv.cfg"),
        "home = /tmp/python\nversion_info = 3.12.8\n",
    )
    .expect("write pyvenv");

    let metadata = load_project_metadata(dir.path(), None).expect("metadata");
    let request = metadata.python_request.expect("python request");

    assert_eq!(request.value, "3.12.8");
    assert!(matches!(request.source, PythonRequestSource::DotVenv));
}

#[test]
fn loads_legacy_poetry_metadata_when_project_table_is_absent() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"
[tool.poetry]
name = "legacy-poetry-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
click = "^8.0"

[tool.poetry.scripts]
legacy-poetry-app = "legacy.cli:main"
"#,
    )
    .expect("write pyproject");

    let metadata = load_project_metadata(dir.path(), None).expect("metadata");

    assert_eq!(metadata.package_name, "legacy-poetry-app");
    assert_eq!(
        metadata
            .project_scripts
            .get("legacy-poetry-app")
            .map(String::as_str),
        Some("legacy.cli:main")
    );
    assert!(matches!(
        metadata.metadata_source,
        ProjectMetadataSource::Poetry
    ));
    let request = metadata.python_request.expect("python request");
    assert_eq!(request.value, "^3.11");
    assert!(matches!(
        request.source,
        PythonRequestSource::PoetryDependency
    ));
}

#[test]
fn falls_back_to_setup_py_when_pyproject_lacks_supported_metadata() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("pyproject.toml"),
        r#"
[build-system]
requires = ["setuptools"]
build-backend = "setuptools.build_meta"
"#,
    )
    .expect("write pyproject");
    fs::write(
        dir.path().join("setup.py"),
        r#"
from setuptools import setup

setup(
    name="legacy-setup-app",
    python_requires=">=3.9",
    entry_points={"console_scripts": ["legacy-setup = legacy.cli:main"]},
)
"#,
    )
    .expect("write setup.py");

    let metadata = load_project_metadata(dir.path(), None).expect("metadata");

    assert_eq!(metadata.package_name, "legacy-setup-app");
    assert_eq!(
        metadata
            .project_scripts
            .get("legacy-setup")
            .map(String::as_str),
        Some("legacy.cli:main")
    );
    assert!(matches!(
        metadata.metadata_source,
        ProjectMetadataSource::SetupPy
    ));
    let request = metadata.python_request.expect("python request");
    assert_eq!(request.value, ">=3.9");
    assert!(matches!(
        request.source,
        PythonRequestSource::SetupPyPythonRequires
    ));
}
