pub mod executor;
pub mod extractor;

use crate::sfx;
use dirs::data_local_dir;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use miette::{IntoDiagnostic, Result, miette};
use std::{
    env, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    time::Duration,
};

const DEFAULT_CACHE_NAMESPACE: &str = "pybin";

pub fn run() -> Result<()> {
    let self_path = env::current_exe().into_diagnostic()?;
    let bundle = sfx::read_bundle(&self_path)?;
    let self_file_name = self_path
        .file_name()
        .ok_or_else(|| miette!("runner could not determine its own filename"))?;
    let cache_folder_name = if bundle.build_uid.is_empty() {
        self_file_name.to_string_lossy().to_string()
    } else {
        format!("{}.{}", self_file_name.to_string_lossy(), bundle.build_uid)
    };
    let cache_path = cache_path(&cache_folder_name)?;
    let target_path = cache_path.join(&bundle.exec_relpath);

    match fs::metadata(&cache_path) {
        Ok(cache) => {
            if cache.modified().into_diagnostic()?
                < fs::metadata(&self_path)
                    .into_diagnostic()?
                    .modified()
                    .into_diagnostic()?
            {
                extract(&self_path, &bundle, &cache_path)?;
            }
        }
        Err(_) => extract(&self_path, &bundle, &cache_path)?,
    }

    let exit_code = executor::execute(&target_path).into_diagnostic()?;
    process::exit(exit_code);
}

fn cache_path(target: &str) -> Result<PathBuf> {
    if let Ok(root) = env::var("PYBIN_CACHE_DIR") {
        return Ok(PathBuf::from(root).join("packages").join(target));
    }

    if let Ok(root) = env::var("WARP_CACHE_DIR") {
        return Ok(PathBuf::from(root).join("packages").join(target));
    }

    let root = data_local_dir()
        .ok_or_else(|| miette!("no local data directory was available for cache extraction"))?;
    Ok(root
        .join(DEFAULT_CACHE_NAMESPACE)
        .join("packages")
        .join(target))
}

fn extract(executable: &Path, bundle: &sfx::BundleMetadata, cache_path: &Path) -> Result<()> {
    let mut progress = RunnerProgress::new();
    fs::remove_dir_all(cache_path).ok();
    progress.start("Extracting packaged runtime for first use");
    extractor::extract_to(executable, bundle, cache_path).into_diagnostic()?;
    progress.finish("Extracted packaged runtime for first use");
    progress.start("Finalizing packaged runtime for first use");
    run_conda_unpack(cache_path)?;
    progress.finish("Finalized packaged runtime for first use");
    Ok(())
}

fn run_conda_unpack(cache_path: &Path) -> Result<()> {
    let unpack = cache_path.join("bin").join("conda-unpack");
    if !unpack.is_file() {
        return Ok(());
    }

    let status = Command::new(&unpack)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .into_diagnostic()?;

    if status.success() {
        return Ok(());
    }

    Err(miette!(
        "conda-unpack failed inside `{}`",
        cache_path.display()
    ))
}

struct RunnerProgress {
    enabled: bool,
    spinner: Option<ProgressBar>,
}

impl RunnerProgress {
    fn new() -> Self {
        Self {
            enabled: progress_enabled(),
            spinner: None,
        }
    }

    fn start(&mut self, message: &str) {
        if self.enabled {
            let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            spinner.set_style(
                ProgressStyle::with_template("{msg} {spinner}").expect("runner spinner template"),
            );
            spinner.enable_steady_tick(Duration::from_millis(100));
            spinner.set_message(format!("{message} (one-time startup step)"));
            self.spinner = Some(spinner);
        }
    }

    fn finish(&mut self, message: &str) {
        if let Some(spinner) = self.spinner.take() {
            spinner.println(format!("{message} (later launches reuse the cache)"));
            spinner.finish_and_clear();
        }
    }
}

fn progress_enabled() -> bool {
    if env::var_os("PYBIN_NO_PROGRESS").is_some() {
        return false;
    }

    std::io::stderr().is_terminal()
        && std::io::stdout().is_terminal()
        && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cache_path_uses_visible_pybin_namespace() {
        let path = cache_path("demo-package").expect("default cache path");
        assert!(path.ends_with("pybin/packages/demo-package"));
        assert!(!path.to_string_lossy().contains(".pybin"));
    }
}
