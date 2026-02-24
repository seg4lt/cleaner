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
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::{
    deletion,
    git::find_git_repos,
    tree::{GlobalTreeEntry, NodeAction, NodeKind, Tree},
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

enum ScanWorkerResult {
    TreeReady {
        generation: u64,
        tree: Tree,
        repo_paths: Vec<PathBuf>,
        global_paths: Vec<PathBuf>,
    },
}

enum SizeWorkerResult {
    RepoEstimate {
        generation: u64,
        estimate: deletion::RepoSavingsEstimate,
    },
    GlobalEstimate {
        generation: u64,
        path: PathBuf,
        size: Option<u64>,
    },
    Progress {
        generation: u64,
        completed: usize,
        total: usize,
    },
    Finished {
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy)]
struct SizeProgress {
    completed: usize,
    total: usize,
}

pub struct App {
    folder: PathBuf,
    tree: Tree,
    state: TreeWidgetState,
    show_results: bool,
    deletion_results: Option<(Vec<String>, Vec<(String, String)>)>,
    run_mode: RunMode,
    confirmation_flow: Option<ConfirmationFlow>,
    loading: bool, // command execution only
    is_scanning_tree: bool,
    is_estimating_sizes: bool,
    size_progress: Option<SizeProgress>,
    status_note: Option<String>,
    scan_generation: u64,
    run_receiver: Option<Receiver<RunWorkerResult>>,
    scan_receiver: Option<Receiver<ScanWorkerResult>>,
    size_receiver: Option<Receiver<SizeWorkerResult>>,
    last_spinner_tick: Instant,
}

impl App {
    pub fn new(folder: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let mut app = Self {
            folder,
            tree: Tree::new(),
            state: TreeWidgetState::new(),
            show_results: false,
            deletion_results: None,
            run_mode: RunMode::Bulk,
            confirmation_flow: None,
            loading: false,
            is_scanning_tree: false,
            is_estimating_sizes: false,
            size_progress: None,
            status_note: None,
            scan_generation: 0,
            run_receiver: None,
            scan_receiver: None,
            size_receiver: None,
            last_spinner_tick: Instant::now(),
        };

        app.start_scan();
        Ok(app)
    }

    fn build_fast_tree(repos: Vec<PathBuf>, globals: &[deletion::GlobalCleanupTarget]) -> Tree {
        let global_entries = globals
            .iter()
            .map(|target| GlobalTreeEntry {
                label: target.label.clone(),
                path: target.path.clone(),
                is_dir: target.is_dir,
                size: target.size,
            })
            .collect();

        Tree::build(repos, global_entries)
    }

    fn spawn_scan_worker(folder: PathBuf, generation: u64) -> Receiver<ScanWorkerResult> {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let repos = find_git_repos(&folder);
            let globals = deletion::discover_global_cleanup_targets();
            let global_paths = globals.iter().map(|g| g.path.clone()).collect::<Vec<_>>();
            let tree = Self::build_fast_tree(repos.clone(), &globals);
            let _ = tx.send(ScanWorkerResult::TreeReady {
                generation,
                tree,
                repo_paths: repos,
                global_paths,
            });
        });
        rx
    }

    fn spawn_size_worker(
        generation: u64,
        repo_paths: Vec<PathBuf>,
        global_paths: Vec<PathBuf>,
    ) -> Receiver<SizeWorkerResult> {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let total = repo_paths.len() + global_paths.len();
            let mut completed = 0usize;
            let _ = tx.send(SizeWorkerResult::Progress {
                generation,
                completed,
                total,
            });

            for repo_path in repo_paths {
                let estimate = deletion::estimate_repo_savings(&repo_path);
                if tx
                    .send(SizeWorkerResult::RepoEstimate {
                        generation,
                        estimate,
                    })
                    .is_err()
                {
                    return;
                }
                completed += 1;
                if tx
                    .send(SizeWorkerResult::Progress {
                        generation,
                        completed,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
            }

            for path in global_paths {
                let size = deletion::estimate_path_size(&path);
                if tx
                    .send(SizeWorkerResult::GlobalEstimate {
                        generation,
                        path,
                        size,
                    })
                    .is_err()
                {
                    return;
                }
                completed += 1;
                if tx
                    .send(SizeWorkerResult::Progress {
                        generation,
                        completed,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
            }

            let _ = tx.send(SizeWorkerResult::Finished { generation });
        });
        rx
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
            self.poll_scan_completion();
            self.poll_size_completion();

            let repo_count = self.tree.repo_count();
            let global_count = self.tree.global_target_count();
            let mut title = format!(
                "Cleaner - {} repo{} | {} global target{} | mode: {}",
                repo_count,
                if repo_count == 1 { "" } else { "s" },
                global_count,
                if global_count == 1 { "" } else { "s" },
                self.run_mode.label()
            );
            if let Some(status) = self.status_line() {
                title.push_str(" | ");
                title.push_str(&status);
            }

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
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                self.start_refresh();
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
            if let Some(node_mut) = self.tree.get_node_mut_by_id(&node_id) {
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
            if let Some(node_mut) = self.tree.get_node_mut_by_id(&node_id) {
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
            if let Some(node_mut) = self.tree.get_node_mut_by_id(&node_id) {
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
                let flow = ConfirmEachFlow {
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
            match (node.kind, node.action) {
                (NodeKind::Repo, NodeAction::Clean) => {
                    commands.push(deletion::RepoCommand::Repo {
                        repo_path: node.path.clone(),
                        kind: deletion::RepoCommandKind::Clean,
                    });
                }
                (NodeKind::Repo, NodeAction::Delete) => {
                    commands.push(deletion::RepoCommand::Repo {
                        repo_path: node.path.clone(),
                        kind: deletion::RepoCommandKind::Delete,
                    });
                }
                (NodeKind::GlobalPath, NodeAction::Delete) => {
                    commands.push(deletion::RepoCommand::GlobalDelete {
                        label: node.label.clone(),
                        path: node.path.clone(),
                    });
                }
                _ => {}
            }
        }

        commands.sort_by(|a, b| a.display_label().cmp(&b.display_label()));
        commands
    }

    fn start_refresh(&mut self) {
        if self.loading {
            return;
        }
        self.start_scan();
    }

    fn start_scan(&mut self) {
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;

        self.is_scanning_tree = true;
        self.is_estimating_sizes = false;
        self.size_progress = None;
        self.status_note = Some("scanning repositories...".to_string());
        self.scan_receiver = Some(Self::spawn_scan_worker(self.folder.clone(), generation));
        self.size_receiver = None;
    }

    fn start_size_estimation(
        &mut self,
        generation: u64,
        repo_paths: Vec<PathBuf>,
        global_paths: Vec<PathBuf>,
    ) {
        let total = repo_paths.len() + global_paths.len();
        self.is_estimating_sizes = total > 0;
        self.size_progress = Some(SizeProgress {
            completed: 0,
            total,
        });
        if total == 0 {
            self.is_estimating_sizes = false;
            if self.tree.flatten_visible().is_empty() {
                self.status_note =
                    Some("no git repositories or global cleanup targets found".into());
            } else {
                self.status_note = None;
            }
            self.size_receiver = None;
            return;
        }

        self.status_note = Some(format!("estimating sizes 0/{}", total));
        self.size_receiver = Some(Self::spawn_size_worker(
            generation,
            repo_paths,
            global_paths,
        ));
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
                    vec![(
                        "worker".to_string(),
                        "Background worker disconnected".to_string(),
                    )],
                ));
                self.show_results = true;
            }
        }
    }

    fn poll_scan_completion(&mut self) {
        let Some(rx) = &self.scan_receiver else {
            return;
        };

        match rx.try_recv() {
            Ok(ScanWorkerResult::TreeReady {
                generation,
                tree,
                repo_paths,
                global_paths,
            }) => {
                self.scan_receiver = None;
                if generation != self.scan_generation {
                    return;
                }

                self.is_scanning_tree = false;
                self.tree = tree;
                let visible_count = self.tree.flatten_visible().len();
                if visible_count == 0 {
                    self.state.selected_index = 0;
                    self.status_note =
                        Some("no git repositories or global cleanup targets found".into());
                } else if self.state.selected_index >= visible_count {
                    self.state.selected_index = visible_count - 1;
                    self.status_note = None;
                } else {
                    self.status_note = None;
                }

                self.start_size_estimation(generation, repo_paths, global_paths);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.scan_receiver = None;
                self.is_scanning_tree = false;
                if self.scan_generation > 0 {
                    self.status_note = Some("scan worker disconnected".to_string());
                }
            }
        }
    }

    fn poll_size_completion(&mut self) {
        let Some(rx) = &self.size_receiver else {
            return;
        };

        loop {
            match rx.try_recv() {
                Ok(SizeWorkerResult::RepoEstimate {
                    generation,
                    estimate,
                }) => {
                    if generation != self.scan_generation {
                        continue;
                    }
                    let _ = self.tree.update_repo_estimate(
                        &estimate.repo_path,
                        estimate.clean_size,
                        estimate.delete_size,
                    );
                }
                Ok(SizeWorkerResult::GlobalEstimate {
                    generation,
                    path,
                    size,
                }) => {
                    if generation != self.scan_generation {
                        continue;
                    }
                    let _ = self.tree.update_global_size(&path, size);
                }
                Ok(SizeWorkerResult::Progress {
                    generation,
                    completed,
                    total,
                }) => {
                    if generation != self.scan_generation {
                        continue;
                    }
                    self.is_estimating_sizes = completed < total;
                    self.size_progress = Some(SizeProgress { completed, total });
                    self.status_note = if total > 0 && completed < total {
                        Some(format!("estimating sizes {}/{}", completed, total))
                    } else if self.is_scanning_tree {
                        Some("scanning repositories...".to_string())
                    } else {
                        None
                    };
                }
                Ok(SizeWorkerResult::Finished { generation }) => {
                    if generation != self.scan_generation {
                        continue;
                    }
                    self.is_estimating_sizes = false;
                    if let Some(progress) = self.size_progress {
                        self.size_progress = Some(SizeProgress {
                            completed: progress.total,
                            total: progress.total,
                        });
                    }
                    if self.tree.flatten_visible().is_empty() {
                        self.status_note =
                            Some("no git repositories or global cleanup targets found".to_string());
                    } else {
                        self.status_note = None;
                    }
                    self.size_receiver = None;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.is_estimating_sizes = false;
                    self.size_receiver = None;
                    if self.scan_generation > 0 && !self.tree.flatten_visible().is_empty() {
                        self.status_note = Some("size worker disconnected".to_string());
                    }
                    break;
                }
            }
        }
    }

    fn status_line(&self) -> Option<String> {
        if self.is_scanning_tree {
            return Some("scanning repositories...".to_string());
        }

        if self.is_estimating_sizes {
            if let Some(progress) = self.size_progress {
                return Some(format!(
                    "estimating sizes {}/{}",
                    progress.completed, progress.total
                ));
            }
            return Some("estimating sizes...".to_string());
        }

        self.status_note.clone()
    }
}
