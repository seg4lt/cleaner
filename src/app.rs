use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget},
    Terminal,
};
use std::io::{self, stdout};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::{
    deletion,
    git::find_git_repos,
    tree::{NodeAction, Tree},
    tree_widget::{HelpWidget, ResultsWidget, TreeWidget, TreeWidgetState},
};

#[derive(Debug, Clone, Copy)]
enum RunMode {
    Bulk,
    ConfirmEach,
}

impl RunMode {
    fn toggle(&mut self) {
        *self = match self {
            Self::Bulk => Self::ConfirmEach,
            Self::ConfirmEach => Self::Bulk,
        };
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bulk => "bulk",
            Self::ConfirmEach => "confirm-each",
        }
    }
}

#[derive(Debug)]
struct ConfirmEachFlow {
    commands: Vec<deletion::RepoCommand>,
    index: usize,
    succeeded: Vec<String>,
    failed: Vec<(String, String)>,
}

#[derive(Debug)]
enum ConfirmationFlow {
    Bulk(Vec<deletion::RepoCommand>),
    Each(ConfirmEachFlow),
}

enum RunWorkerResult {
    Bulk {
        succeeded: Vec<String>,
        failed: Vec<(String, String)>,
    },
    Single {
        command: deletion::RepoCommand,
        result: Result<String, String>,
        flow: ConfirmEachFlow,
    },
}

pub struct App {
    folder: PathBuf,
    tree: Tree,
    state: TreeWidgetState,
    show_results: bool,
    deletion_results: Option<(Vec<String>, Vec<(String, String)>)>,
    run_mode: RunMode,
    confirmation_flow: Option<ConfirmationFlow>,
    loading: bool,
    run_receiver: Option<Receiver<RunWorkerResult>>,
    last_spinner_tick: Instant,
    repo_receiver: Option<Receiver<(PathBuf, Vec<(PathBuf, bool)>)>>,
    repos_found: usize,
}

impl App {
    pub fn new(folder: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Find all git repositories recursively
        let repos = find_git_repos(&folder);

        if repos.is_empty() {
            return Err("No git repositories found in the specified folder".into());
        }

        // Build a repo-only tree (one row per repo)
        let repo_data: Vec<_> = repos.into_iter().map(|repo| (repo, Vec::new())).collect();
        let tree = Tree::build(repo_data);

        let mut state = TreeWidgetState::new();

        // Select the first visible node
        let visible = tree.flatten_visible();
        if !visible.is_empty() {
            state.selected_index = 0;
        }

        Ok(Self {
            folder,
            tree,
            state,
            show_results: false,
            deletion_results: None,
            run_mode: RunMode::Bulk,
            confirmation_flow: None,
            loading: false,
            run_receiver: None,
            last_spinner_tick: Instant::now(),
            repo_receiver: None,
            repos_found: 0,
        })
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal);

        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;

        result
    }

    fn run_app(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            self.tick_loading();
            self.poll_run_completion();

            let repo_count = self.tree.roots.len();
            let title = format!(
                "Git Cleaner - {} repo{} found | mode: {}",
                repo_count,
                if repo_count == 1 { "" } else { "s" },
                self.run_mode.label()
            );

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
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }

                    if self.show_results {
                        // Any key closes results screen and returns to the list
                        self.show_results = false;
                        self.deletion_results = None;
                        continue;
                    } else if self.state.show_confirmation {
                        // Handle confirmation dialog
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                self.confirm_current_prompt(true);
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                self.confirm_current_prompt(false);
                            }
                            _ => {}
                        }
                    } else {
                        if self.loading {
                            continue;
                        }
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
                            KeyCode::Char('m') | KeyCode::Char('M') => {
                                self.run_mode.toggle();
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
                node_mut.cycle_action();
            }
        }
    }

    fn show_delete_confirmation(&mut self) {
        let commands = self.collect_commands();
        if commands.is_empty() {
            return;
        }

        match self.run_mode {
            RunMode::Bulk => {
                let count = commands.len();
                self.state.confirmation_message = if count == 1 {
                    "Run 1 command?".to_string()
                } else {
                    format!("Run {} commands in bulk?", count)
                };
                self.confirmation_flow = Some(ConfirmationFlow::Bulk(commands));
                self.state.show_confirmation = true;
            }
            RunMode::ConfirmEach => {
                let mut flow = ConfirmEachFlow {
                    commands,
                    index: 0,
                    succeeded: Vec::new(),
                    failed: Vec::new(),
                };
                self.state.confirmation_message = self.confirm_each_message(&flow);
                self.confirmation_flow = Some(ConfirmationFlow::Each(flow));
                self.state.show_confirmation = true;
            }
        }
    }

    fn confirm_current_prompt(&mut self, approved: bool) {
        let flow = self.confirmation_flow.take();
        match flow {
            Some(ConfirmationFlow::Bulk(commands)) => {
                self.state.show_confirmation = false;
                if approved {
                    self.start_bulk_run(commands);
                }
            }
            Some(ConfirmationFlow::Each(mut flow)) => {
                if approved {
                    if let Some(command) = flow.commands.get(flow.index).cloned() {
                        self.state.show_confirmation = false;
                        self.start_single_run(command, flow);
                        return;
                    }
                }
                flow.index += 1;
                self.advance_each_confirmation(flow);
            }
            None => {
                self.state.show_confirmation = false;
            }
        }
    }

    fn advance_each_confirmation(&mut self, flow: ConfirmEachFlow) {
        if flow.index >= flow.commands.len() {
            self.state.show_confirmation = false;
            self.finish_run(flow.succeeded, flow.failed);
            return;
        }

        self.state.confirmation_message = self.confirm_each_message(&flow);
        self.confirmation_flow = Some(ConfirmationFlow::Each(flow));
        self.state.show_confirmation = true;
    }

    fn finish_run(&mut self, succeeded: Vec<String>, failed: Vec<(String, String)>) {
        self.confirmation_flow = None;
        self.loading = false;
        self.state.loading_message = None;
        self.run_receiver = None;
        self.deletion_results = Some((succeeded, failed));
        self.show_results = true;
    }

    fn confirm_each_message(&self, flow: &ConfirmEachFlow) -> String {
        let total = flow.commands.len();
        let current_num = flow.index + 1;
        if let Some(command) = flow.commands.get(flow.index) {
            format!(
                "[{}/{}] Run {}?",
                current_num,
                total,
                command.display_label()
            )
        } else {
            "Run next command?".to_string()
        }
    }

    fn collect_commands(&self) -> Vec<deletion::RepoCommand> {
        let mut commands = Vec::new();

        for node in self.tree.get_marked_nodes() {
            let kind = match node.action {
                NodeAction::Clean => Some(deletion::RepoCommandKind::Clean),
                NodeAction::Delete => Some(deletion::RepoCommandKind::Delete),
                NodeAction::None => None,
            };

            if let Some(kind) = kind {
                commands.push(deletion::RepoCommand {
                    repo_path: node.repo_path.clone(),
                    kind,
                });
            }
        }

        commands.sort_by(|a, b| a.repo_path.cmp(&b.repo_path).then(a.kind.label().cmp(b.kind.label())));
        commands
    }

    fn start_bulk_run(&mut self, commands: Vec<deletion::RepoCommand>) {
        let count = commands.len();
        self.loading = true;
        self.state.loading_frame = 0;
        self.last_spinner_tick = Instant::now();
        self.state.loading_message = Some(if count == 1 {
            "Running 1 command...".to_string()
        } else {
            format!("Running {} commands...", count)
        });

        let (tx, rx) = channel();
        thread::spawn(move || {
            let (succeeded, failed) = deletion::run_repo_commands(commands);
            let _ = tx.send(RunWorkerResult::Bulk { succeeded, failed });
        });
        self.run_receiver = Some(rx);
    }

    fn start_single_run(&mut self, command: deletion::RepoCommand, flow: ConfirmEachFlow) {
        self.loading = true;
        self.state.loading_frame = 0;
        self.last_spinner_tick = Instant::now();
        self.state.loading_message = Some(format!("Running {}...", command.display_label()));

        let (tx, rx) = channel();
        thread::spawn(move || {
            let result = deletion::run_repo_command(&command);
            let _ = tx.send(RunWorkerResult::Single {
                command,
                result,
                flow,
            });
        });
        self.run_receiver = Some(rx);
    }

    fn tick_loading(&mut self) {
        if !self.loading {
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_spinner_tick) >= Duration::from_millis(120) {
            self.state.loading_frame = self.state.loading_frame.wrapping_add(1);
            self.last_spinner_tick = now;
        }
    }

    fn poll_run_completion(&mut self) {
        let Some(rx) = &self.run_receiver else {
            return;
        };

        match rx.try_recv() {
            Ok(RunWorkerResult::Bulk { succeeded, failed }) => {
                self.finish_run(succeeded, failed);
            }
            Ok(RunWorkerResult::Single {
                command,
                result,
                mut flow,
            }) => {
                self.loading = false;
                self.state.loading_message = None;
                self.run_receiver = None;

                match result {
                    Ok(item) => flow.succeeded.push(item),
                    Err(error) => flow.failed.push((command.display_label(), error)),
                }
                flow.index += 1;
                self.advance_each_confirmation(flow);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.loading = false;
                self.state.loading_message = None;
                self.run_receiver = None;
                self.state.show_confirmation = false;
                self.deletion_results = Some((
                    Vec::new(),
                    vec![("worker".to_string(), "Background worker disconnected".to_string())],
                ));
                self.show_results = true;
            }
        }
    }
}
