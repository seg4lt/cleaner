use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Find all git repository roots within the given folder
pub fn find_git_repos(folder: &Path) -> Vec<PathBuf> {
    let repos = Mutex::new(Vec::new());
    find_git_repos_parallel(folder, &repos);
    let mut repos = repos.into_inner().unwrap();
    repos.sort();
    repos
}

fn find_git_repos_parallel(folder: &Path, repos: &Mutex<Vec<PathBuf>>) {
    // Check if current folder is a git repo
    let git_dir = folder.join(".git");
    if git_dir.exists() {
        repos.lock().unwrap().push(folder.to_path_buf());

        // Optimization: once we're inside a repo, only recurse into declared submodules.
        // This avoids scanning the entire repo tree while still discovering nested git repos
        // that are intentionally part of the repository.
        let submodule_dirs = find_submodule_dirs(folder);
        submodule_dirs.par_iter().for_each(|submodule_dir| {
            find_git_repos_parallel(submodule_dir, repos);
        });
        return;
    }

    // Read directory entries
    let entries: Vec<_> = match std::fs::read_dir(folder) {
        Ok(entries) => entries.flatten().collect(),
        Err(_) => return,
    };

    // Process subdirectories in parallel
    entries
        .par_iter()
        .filter(|entry| entry.path().is_dir())
        .for_each(|entry| {
            let path = entry.path();

            // Skip hidden directories (including .git internals)
            if let Some(name) = path.file_name() {
                if let Some(name_str) = name.to_str() {
                    if name_str.starts_with('.') {
                        return;
                    }
                }
            }

            find_git_repos_parallel(&path, repos);
        });
}

fn find_submodule_dirs(repo_root: &Path) -> Vec<PathBuf> {
    let gitmodules = repo_root.join(".gitmodules");
    let contents = match std::fs::read_to_string(&gitmodules) {
        Ok(contents) => contents,
        Err(_) => return Vec::new(),
    };

    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (key, value) = trimmed.split_once('=')?;
            if key.trim() != "path" {
                return None;
            }

            let submodule_path = value.trim();
            if submodule_path.is_empty() {
                return None;
            }

            let path = repo_root.join(submodule_path);
            if path.is_dir() {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

/// Get untracked files for a git repository
/// Returns list of (path, is_directory) tuples
pub fn get_untracked_files(repo_path: &Path) -> Vec<(PathBuf, bool)> {
    // Get both untracked and ignored files in one command
    // Using --porcelain without -uall to get only top-level items
    let output = Command::new("git")
        .args(&["status", "--porcelain", "--ignored"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_untracked_files(repo_path, &stdout)
        }
        _ => Vec::new(),
    }
}

fn parse_untracked_files(repo_path: &Path, git_output: &str) -> Vec<(PathBuf, bool)> {
    let mut untracked = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for line in git_output.lines() {
        // Lines starting with "??" are untracked, "!!" are ignored
        if line.starts_with("?? ") || line.starts_with("!! ") {
            let path_str = &line[3..];
            let full_path = repo_path.join(path_str);

            // Check if it's a directory
            let is_dir = full_path.is_dir();

            if is_dir {
                // For directories, add only the top-level directory
                // and skip any files within it
                let path = PathBuf::from(path_str);
                if let Some(first_component) = path.components().next() {
                    let first_path = PathBuf::from(first_component.as_os_str());
                    if seen_dirs.insert(first_path.clone()) {
                        untracked.push((first_path, true));
                    }
                }
            } else {
                // For files, check if they're in an already-tracked directory
                let path = PathBuf::from(path_str);

                // Check if any parent directory is already in our untracked list
                let mut in_untracked_dir = false;
                for (untracked_path, is_untracked_dir) in &untracked {
                    if *is_untracked_dir {
                        if path.starts_with(untracked_path) {
                            in_untracked_dir = true;
                            break;
                        }
                    }
                }

                if !in_untracked_dir {
                    untracked.push((path, false));
                }
            }
        }
    }

    untracked
}
