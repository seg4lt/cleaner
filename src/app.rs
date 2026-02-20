use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::io::{self, stdout};
use std::path::PathBuf;

use crate::{
    deletion,
    git::{find_git_repos, get_untracked_files},
    tree::Tree,
    tree_widget::{HelpWidget, ResultsWidget, TreeWidget, TreeWidgetState},
};

pub struct App {
    folder: PathBuf,
    tree: Tree,
    state: TreeWidgetState,
    show_results: bool,
    deletion_results: Option<(Vec<String>, Vec<(String, String)>)>,
}

impl App {
    pub fn new(folder: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Find all git repositories
        let repos = find_git_repos(&folder);

        if repos.is_empty() {
            return Err("No git repositories found in the specified folder".into());
        }

        // Get untracked files for each repo
        let mut repo_data = Vec::new();
        for repo in &repos {
            let untracked = get_untracked_files(repo);
            if !untracked.is_empty() {
                repo_data.push((repo.clone(), untracked));
            }
        }

        if repo_data.is_empty() {
            return Err("No untracked files found in any repository".into());
        }

        // Build the tree
        let tree = Tree::build(repo_data);

        let mut state = TreeWidgetState::new();

        // Select the first visible node
        let visible = tree.flatten_visible();
        if !visible.is_empty() {
            let first_id = visible[0].id.clone();
            state.selected_index = 0;
        }

        Ok(Self {
            folder,
            tree,
            state,
            show_results: false,
            deletion_results: None,
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Setup terminal
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal);

        // Restore terminal
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;

        result
    }

    fn run_app(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let repo_count = self.tree.roots.len();
        let title = format!(
            "Git Cleaner - {} repo{} found",
            repo_count,
            if repo_count == 1 { "" } else { "s" }
        );

        loop {
            // Draw UI
            terminal.draw(|frame| {
                let area = frame.area();

                if self.show_results {
                    // Show results screen
                    if let Some((ref deleted, ref failed)) = self.deletion_results {
                        let results_widget = ResultsWidget {
                            deleted: deleted.clone(),
                            failed: failed.clone(),
                        };
                        frame.render_widget(results_widget, area);
                    }
                } else {
                    // Show main tree view
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(0), Constraint::Length(1)])
                        .split(area);

                    // Render tree
                    let tree_widget = TreeWidget::new(&self.tree, title.clone());
                    frame.render_stateful_widget(tree_widget, chunks[0], &mut self.state);

                    // Render help
                    frame.render_widget(HelpWidget, chunks[1]);
                }
            })?;

            // Handle events
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        if self.show_results {
                            // Any key exits results screen
                            return Ok(());
                        } else if self.state.show_confirmation {
                            // Handle confirmation dialog
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    self.execute_deletion();
                                    self.state.show_confirmation = false;
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    self.state.show_confirmation = false;
                                }
                                _ => {}
                            }
                        } else {
                            // Handle normal navigation
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    return Ok(());
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    let visible_count = self.tree.flatten_visible().len();
                                    self.state.select_next(visible_count);
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    self.state.select_previous();
                                }
                                KeyCode::Char('h') | KeyCode::Left => {
                                    self.collapse_current();
                                }
                                KeyCode::Char('l') | KeyCode::Right => {
                                    self.expand_current();
                                }
                                KeyCode::Char(' ') => {
                                    self.toggle_mark_current();
                                }
                                KeyCode::Enter => {
                                    self.show_delete_confirmation();
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    fn collapse_current(&mut self) {
        let visible = self.tree.flatten_visible();
        if let Some(node) = visible.get(self.state.selected_index) {
            let node_id = node.id.clone();
            let repo_path = node.repo_path.clone();
            if let Some(node_mut) = self.tree.get_node_mut_in_repo(&repo_path, &node_id) {
                if node_mut.is_dir && node_mut.is_expanded {
                    node_mut.is_expanded = false;
                }
            }
        }
    }

    fn expand_current(&mut self) {
        let visible = self.tree.flatten_visible();
        if let Some(node) = visible.get(self.state.selected_index) {
            let node_id = node.id.clone();
            let repo_path = node.repo_path.clone();
            if let Some(node_mut) = self.tree.get_node_mut_in_repo(&repo_path, &node_id) {
                if node_mut.is_dir && !node_mut.is_expanded {
                    node_mut.is_expanded = true;
                }
            }
        }
    }

    fn toggle_mark_current(&mut self) {
        let visible = self.tree.flatten_visible();
        if let Some(node) = visible.get(self.state.selected_index) {
            let node_id = node.id.clone();
            let repo_path = node.repo_path.clone();
            if let Some(node_mut) = self.tree.get_node_mut_in_repo(&repo_path, &node_id) {
                node_mut.toggle_marked();
            }
        }
    }

    fn select_parent(&mut self, node_id: &str) {
        // Find the parent by looking at the visible nodes
        let visible = self.tree.flatten_visible();
        for (i, node) in visible.iter().enumerate() {
            if node.id == node_id {
                // Find the previous node with lower depth
                for j in (0..i).rev() {
                    if visible[j].depth < node.depth {
                        self.state.selected_index = j;
                        return;
                    }
                }
                break;
            }
        }
    }

    fn show_delete_confirmation(&mut self) {
        let marked = self.tree.get_marked_nodes();
        if marked.is_empty() {
            return;
        }

        let count = marked.len();
        let message = if count == 1 {
            format!("Delete 1 item?")
        } else {
            format!("Delete {} items?", count)
        };

        self.state.confirmation_message = message;
        self.state.show_confirmation = true;
    }

    fn execute_deletion(&mut self) {
        let marked = self.tree.get_marked_nodes();
        if marked.is_empty() {
            return;
        }

        // Collect paths to delete
        let paths: Vec<PathBuf> = marked.iter().map(|node| node.full_path()).collect();

        // Delete in parallel
        let (deleted, failed) = deletion::delete_paths(paths);

        self.deletion_results = Some((deleted, failed));
        self.show_results = true;
    }
}
