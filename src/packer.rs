use crate::sfx;
use flate2::{Compression, write::GzEncoder};
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use std::{
    env,
    fs::{self, File},
    io::{self, Write, copy},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tar::Builder;
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub stub_path: Option<PathBuf>,
    pub unique_id: bool,
}

#[derive(Debug, Clone)]
pub struct PackManifest {
    pub build_uid: String,
}

pub fn pack_directory(
    input_dir: impl AsRef<Path>,
    exec_relpath: &str,
    output_path: impl AsRef<Path>,
    options: &PackOptions,
) -> Result<PackManifest> {
    let input_dir = input_dir.as_ref();
    let output_path = output_path.as_ref();

    if !input_dir.is_dir() {
        return Err(miette!(
            "input directory `{}` does not exist or is not a directory",
            input_dir.display()
        ));
    }

    let exec_path = input_dir.join(exec_relpath);
    if !exec_path.is_file() {
        return Err(miette!(
            "target executable `{}` was not found inside the staged directory",
            exec_path.display()
        ));
    }

    let build_uid = if options.unique_id {
        generate_uid()
    } else {
        String::new()
    };

    let stub_path = options.stub_path.clone().unwrap_or(current_stub_path()?);
    let runner_bytes = fs::read(&stub_path).into_diagnostic().wrap_err_with(|| {
        format!(
            "failed to read the SFX stub executable at `{}`",
            stub_path.display()
        )
    })?;
    let tarball = create_tgz(input_dir)?;
    let metadata = sfx::encode_metadata(exec_relpath, &build_uid);
    let payload_len = tarball.as_file().metadata().into_diagnostic()?.len();
    let footer = sfx::footer_bytes(payload_len, metadata.len() as u32);

    let mut output = create_output_file(output_path).into_diagnostic()?;
    output.write_all(&runner_bytes).into_diagnostic()?;

    let mut tarball_file = File::open(tarball.path()).into_diagnostic()?;
    copy(&mut tarball_file, &mut output).into_diagnostic()?;
    output.write_all(&metadata).into_diagnostic()?;
    output.write_all(&footer).into_diagnostic()?;
    drop(output);

    Ok(PackManifest { build_uid })
}

fn current_stub_path() -> Result<std::path::PathBuf> {
    env::current_exe()
        .into_diagnostic()
        .wrap_err("could not determine the current pybin executable for SFX packing")
}

fn create_tgz(input_dir: &Path) -> Result<NamedTempFile> {
    let tarball = NamedTempFile::new().into_diagnostic()?;
    let gz = GzEncoder::new(tarball.reopen().into_diagnostic()?, Compression::best());
    let mut tar = Builder::new(gz);
    tar.follow_symlinks(false);
    tar.append_dir_all(".", input_dir).into_diagnostic()?;
    tar.finish().into_diagnostic()?;
    Ok(tarball)
}

fn create_output_file(path: &Path) -> io::Result<File> {
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o755)
            .open(path)
    }

    #[cfg(target_family = "windows")]
    {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
    }
}

fn generate_uid() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("{timestamp:x}{pid:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_ids() {
        let first = generate_uid();
        let second = generate_uid();
        assert_ne!(first, second);
    }
}
