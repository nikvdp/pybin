use pybin::project::{PythonRequestSource, load_project_metadata};
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
