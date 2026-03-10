use pybin::packer::{PackOptions, pack_directory};
use std::{fs, path::PathBuf, process::Command};
use tempfile::tempdir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
#[test]
fn packed_runner_executes_a_staged_script() {
    let temp = tempdir().expect("tempdir");
    let stage = temp.path().join("stage");
    let bin_dir = stage.join("bin");
    let cache_dir = temp.path().join("cache");
    let output = temp.path().join("hello-sfx");
    let script = bin_dir.join("hello");
    let runner_path = PathBuf::from(env!("CARGO_BIN_EXE_pybin-runner"));

    fs::create_dir_all(&bin_dir).expect("create staged bin dir");
    fs::write(&script, "#!/bin/sh\necho runner-ok\n").expect("write script");

    let mut perms = fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("set executable bit");

    let manifest = pack_directory(
        &stage,
        "bin/hello",
        &output,
        &PackOptions {
            runner_path,
            unique_id: true,
        },
    )
    .expect("pack staged dir");

    assert!(
        !manifest.build_uid.is_empty(),
        "expected a non-empty build uid"
    );

    let result = Command::new(&output)
        .env("WARP_CACHE_DIR", &cache_dir)
        .output()
        .expect("run packed output");

    let cache_packages_dir = cache_dir.join("packages");
    let cache_entry_names: Vec<String> = if cache_packages_dir.exists() {
        fs::read_dir(&cache_packages_dir)
            .expect("packages read_dir")
            .map(|entry| {
                entry
                    .expect("packages entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    } else {
        Vec::new()
    };

    assert!(
        result.status.success(),
        "packed binary failed\nstatus: {:?}\ncache package entries: {:?}\nstdout:\n{}\nstderr:\n{}",
        result.status.code(),
        cache_entry_names,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "runner-ok\n");
    assert_eq!(cache_entry_names.len(), 1, "expected one extracted package");
    assert_eq!(
        cache_entry_names[0],
        format!(
            "{}.{}",
            output.file_name().unwrap().to_string_lossy(),
            manifest.build_uid
        ),
    );
}
