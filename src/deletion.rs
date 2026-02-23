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
