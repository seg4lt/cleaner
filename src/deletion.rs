use rayon::prelude::*;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoCommandKind {
    Clean,
    Delete,
}

impl RepoCommandKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepoCommand {
    pub repo_path: PathBuf,
    pub kind: RepoCommandKind,
}

impl RepoCommand {
    pub fn display_label(&self) -> String {
        format!("[{}] {}", self.kind.label(), self.repo_path.display())
    }
}

pub fn run_repo_command(command: &RepoCommand) -> Result<String, String> {
    let display = command.display_label();
    let result = match command.kind {
        RepoCommandKind::Clean => clean_repo(&command.repo_path),
        RepoCommandKind::Delete => delete_repo_dir(&command.repo_path),
    };

    result.map(|_| display)
}

pub fn run_repo_commands(commands: Vec<RepoCommand>) -> (Vec<String>, Vec<(String, String)>) {
    let results: Vec<_> = commands
        .into_par_iter()
        .map(|command| {
            let label = command.display_label();
            match run_repo_command(&command) {
                Ok(done) => Ok(done),
                Err(e) => Err((label, e)),
            }
        })
        .collect();

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for result in results {
        match result {
            Ok(item) => succeeded.push(item),
            Err((item, error)) => failed.push((item, error)),
        }
    }

    (succeeded, failed)
}

/// Run `git clean -fxd` in multiple repositories in parallel.
/// Returns (successfully_cleaned_repos, failed_with_error)
pub fn clean_repos(repos: Vec<PathBuf>) -> (Vec<String>, Vec<(String, String)>) {
    let results: Vec<_> = repos
        .into_par_iter()
        .map(|repo_path| {
            let repo_str = repo_path.to_string_lossy().to_string();
            match clean_repo(&repo_path) {
                Ok(()) => Ok(repo_str),
                Err(e) => Err((repo_str, e)),
            }
        })
        .collect();

    let mut cleaned = Vec::new();
    let mut failed = Vec::new();

    for result in results {
        match result {
            Ok(repo) => cleaned.push(repo),
            Err((repo, error)) => failed.push((repo, error)),
        }
    }

    (cleaned, failed)
}

fn clean_repo(repo_path: &PathBuf) -> Result<(), String> {
    let output = Command::new("git")
        .args(["clean", "-fxd"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git clean: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("git clean exited with status {}", output.status))
        } else {
            Err(stderr)
        }
    }
}

fn delete_repo_dir(repo_path: &PathBuf) -> Result<(), String> {
    let output = Command::new("rm")
        .arg("-rf")
        .arg(repo_path)
        .output()
        .map_err(|e| format!("Failed to run rm -rf: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("rm -rf exited with status {}", output.status))
        } else {
            Err(stderr)
        }
    }
}

/// Delete multiple paths in parallel
/// Returns (successfully_deleted, failed_with_error)
pub fn delete_paths(
    repo_path: &PathBuf,
    paths: Vec<PathBuf>,
) -> (Vec<String>, Vec<(String, String)>) {
    let results: Vec<_> = paths
        .into_par_iter()
        .map(|path| {
            let path_str = path.to_string_lossy().to_string();
            match delete_path(repo_path, &path) {
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

fn delete_path(repo_path: &PathBuf, path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Err("Path does not exist".to_string());
    }

    // SAFETY: Verify the file is still untracked before deleting
    let relative_path = match path.strip_prefix(repo_path) {
        Ok(p) => p,
        Err(_) => return Err("Path is not within repository".to_string()),
    };

    if !is_untracked(repo_path, relative_path) {
        return Err("Path is tracked by git - refusing to delete".to_string());
    }

    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| format!("Failed to remove directory: {}", e))
    } else {
        std::fs::remove_file(path).map_err(|e| format!("Failed to remove file: {}", e))
    }
}

/// Check if a path is untracked by git
fn is_untracked(repo_path: &PathBuf, relative_path: &std::path::Path) -> bool {
    let output = Command::new("git")
        .args(&["status", "--porcelain", "-uall"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let path_str = relative_path.to_string_lossy();

            // Check if the path appears as untracked (starts with ??)
            for line in stdout.lines() {
                if line.starts_with("?? ") {
                    let untracked_path = &line[3..];
                    if untracked_path.starts_with(path_str.as_ref())
                        || path_str.starts_with(untracked_path)
                    {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}
