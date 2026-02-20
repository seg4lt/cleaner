use rayon::prelude::*;
use std::path::PathBuf;

/// Delete multiple paths in parallel
/// Returns (successfully_deleted, failed_with_error)
pub fn delete_paths(paths: Vec<PathBuf>) -> (Vec<String>, Vec<(String, String)>) {
    let results: Vec<_> = paths
        .into_par_iter()
        .map(|path| {
            let path_str = path.to_string_lossy().to_string();
            match delete_path(&path) {
                Ok(()) => Ok(path_str),
                Err(e) => Err((path_str, e)),
            }
        })
        .collect();

    let mut deleted = Vec::new();
    let mut failed = Vec::new();

    for result in results {
        match result {
            Ok(path) => deleted.push(path),
            Err((path, error)) => failed.push((path, error)),
        }
    }

    (deleted, failed)
}

fn delete_path(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Err("Path does not exist".to_string());
    }

    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| format!("Failed to remove directory: {}", e))
    } else {
        std::fs::remove_file(path).map_err(|e| format!("Failed to remove file: {}", e))
    }
}
