use pybin::{
    build::{PrepareBuildOptions, SilentBuildProgress, prepare_build},
    packer::{PackOptions, pack_directory},
    plan::BuildPlan,
    project::load_project_metadata,
    sfx::PayloadCompression,
};
use std::{fs, path::PathBuf, process::Command};
use tempfile::tempdir;

#[cfg(unix)]
#[test]
#[ignore = "slow end-to-end smoke; requires conda, uv, and networked package resolution"]
fn builds_and_runs_the_demo_fixture_as_a_single_binary() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/demo-app");
    let temp = tempdir().expect("tempdir");
    let work_dir = temp.path().join("work");
    let output = temp.path().join("demo-sfx");
    let cache_dir = temp.path().join("cache with spaces");

    let metadata = load_project_metadata(&fixture, None).expect("fixture metadata");
    let plan = BuildPlan::resolve(metadata, None).expect("fixture plan");
    let mut progress = SilentBuildProgress;
    let prepared = prepare_build(
        &plan,
        &PrepareBuildOptions {
            work_dir: Some(work_dir),
        },
        &mut progress,
    )
    .expect("prepared build");

    let exec_relpath = prepared.launcher_relpath.to_string_lossy().to_string();
    let stub_path = PathBuf::from(env!("CARGO_BIN_EXE_pybin"));
    let manifest = pack_directory(
        &prepared.stage_dir,
        &exec_relpath,
        &output,
        &PackOptions {
            stub_path: Some(stub_path),
            unique_id: true,
            payload_compression: PayloadCompression::Zstd,
        },
    )
    .expect("packed output");

    assert!(!manifest.build_uid.is_empty(), "expected a unique build id");

    let first = Command::new(&output)
        .env("PYBIN_CACHE_DIR", &cache_dir)
        .args(["smoke", "one"])
        .output()
        .expect("run packed fixture");
    assert!(
        first.status.success(),
        "first run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        "demo-args:smoke,one\n"
    );

    fs::remove_dir_all(&cache_dir).expect("remove cache dir");

    let second = Command::new(&output)
        .env("PYBIN_CACHE_DIR", &cache_dir)
        .arg("two")
        .output()
        .expect("rerun packed fixture");
    assert!(
        second.status.success(),
        "second run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&second.stdout), "demo-args:two\n");
}
