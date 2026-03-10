use std::{
    env, io,
    path::Path,
    process::{Command, Stdio},
};

#[cfg(target_family = "unix")]
use std::{fs, fs::Permissions, os::unix::fs::PermissionsExt};

pub fn execute(target: &Path) -> io::Result<i32> {
    let args: Vec<String> = env::args().skip(1).collect();
    do_execute(target, &args)
}

#[cfg(target_family = "unix")]
fn ensure_executable(target: &Path) {
    let perms = Permissions::from_mode(0o770);
    fs::set_permissions(target, perms).ok();
}

#[cfg(target_family = "unix")]
fn do_execute(target: &Path, args: &[String]) -> io::Result<i32> {
    ensure_executable(target);

    Ok(Command::new(target)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?
        .wait()?
        .code()
        .unwrap_or(1))
}

#[cfg(target_family = "windows")]
fn do_execute(target: &Path, args: &[String]) -> io::Result<i32> {
    Ok(Command::new(target)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?
        .wait()?
        .code()
        .unwrap_or(1))
}
