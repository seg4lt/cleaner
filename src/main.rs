use clap::Parser;
use std::path::PathBuf;

mod app;
mod deletion;
mod git;
mod tree;
mod tree_widget;

#[derive(Parser)]
#[command(name = "cleaner")]
#[command(about = "A CLI tool to find and delete untracked git files")]
struct Cli {
    /// The folder to scan for git repositories
    #[arg(default_value = ".")]
    folder: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Validate folder exists
    if !cli.folder.exists() {
        eprintln!("Error: Folder '{}' does not exist", cli.folder.display());
        std::process::exit(1);
    }

    if !cli.folder.is_dir() {
        eprintln!("Error: '{}' is not a directory", cli.folder.display());
        std::process::exit(1);
    }

    // Run the TUI application
    let mut app = app::App::new(cli.folder)?;
    app.run()?;

    Ok(())
}
