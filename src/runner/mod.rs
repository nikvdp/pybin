pub mod executor;
pub mod extractor;

use crate::sfx;
use dirs::data_local_dir;
use miette::{IntoDiagnostic, Result, miette};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

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
    if let Ok(root) = env::var("WARP_CACHE_DIR") {
        return Ok(PathBuf::from(root).join("packages").join(target));
    }

    let root = data_local_dir()
        .ok_or_else(|| miette!("no local data directory was available for cache extraction"))?;
    Ok(root.join("warp").join("packages").join(target))
}

fn extract(executable: &Path, bundle: &sfx::BundleMetadata, cache_path: &Path) -> Result<()> {
    fs::remove_dir_all(cache_path).ok();
    extractor::extract_to(executable, bundle, cache_path).into_diagnostic()?;
    Ok(())
}
