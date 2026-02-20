use std::path::{Path, PathBuf};
use std::process::Command;

/// Find all git repository roots within the given folder
pub fn find_git_repos(folder: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    find_git_repos_recursive(folder, &mut repos);
    repos
}

fn find_git_repos_recursive(folder: &Path, repos: &mut Vec<PathBuf>) {
    // Check if current folder is a git repo
    let git_dir = folder.join(".git");
    if git_dir.exists() {
        repos.push(folder.to_path_buf());
        // Don't recurse into subdirectories of a git repo
        // (submodules will be handled separately if needed)
        return;
    }

    // Recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories
                if let Some(name) = path.file_name() {
                    if let Some(name_str) = name.to_str() {
                        if name_str.starts_with('.') && name_str != ".git" {
                            continue;
                        }
                    }
                }
                find_git_repos_recursive(&path, repos);
            }
        }
    }
}

/// Get untracked files for a git repository
/// Returns list of (path, is_directory) tuples
pub fn get_untracked_files(repo_path: &Path) -> Vec<(PathBuf, bool)> {
    let output = Command::new("git")
        .args(&["status", "--porcelain", "-uall"])
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
        // Lines starting with "??" are untracked
        if line.starts_with("?? ") {
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
