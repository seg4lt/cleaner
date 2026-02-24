use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Section,
    Repo,
    GlobalPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeAction {
    None,
    Clean,
    Delete,
}

impl NodeAction {
    pub fn cycle(self) -> Self {
        match self {
            Self::None => Self::Clean,
            Self::Clean => Self::Delete,
            Self::Delete => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepoEstimateStatus {
    pub clean_done: bool,
    pub delete_done: bool,
}

/// A node in the tree structure.
/// The current app uses a repo-only list, but we keep a small generic node shape
/// so the existing tree widget can continue to render it.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub action: NodeAction,
    pub depth: usize,
    pub children: Vec<TreeNode>,
    pub path: PathBuf,
    pub size: Option<u64>,
    pub size_done: bool,
    pub clean_size: Option<u64>,
    pub delete_size: Option<u64>,
    pub repo_estimate_status: Option<RepoEstimateStatus>,
}

impl TreeNode {
    pub fn new(
        id: String,
        label: String,
        kind: NodeKind,
        is_dir: bool,
        depth: usize,
        path: PathBuf,
    ) -> Self {
        Self {
            id,
            label,
            kind,
            is_dir,
            is_expanded: true,
            action: NodeAction::None,
            depth,
            children: Vec::new(),
            path,
            size: None,
            size_done: false,
            clean_size: None,
            delete_size: None,
            repo_estimate_status: None,
        }
    }

    pub fn global_size_display(&self) -> Option<String> {
        if self.kind != NodeKind::GlobalPath {
            return None;
        }

        Some(match (self.size_done, self.size) {
            (false, _) => "...".to_string(),
            (true, Some(size)) => format_size(size),
            (true, None) => "?".to_string(),
        })
    }

    pub fn repo_sizes_display(&self) -> Option<String> {
        if self.kind != NodeKind::Repo {
            return None;
        }

        let status = self.repo_estimate_status.unwrap_or_default();
        let clean = field_display(self.clean_size, status.clean_done);
        let delete = field_display(self.delete_size, status.delete_done);

        Some(format!("clean: {}, delete: {}", clean, delete))
    }

    pub fn cycle_action(&mut self) {
        match self.kind {
            NodeKind::Section => {}
            NodeKind::Repo => {
                self.action = self.action.cycle();
            }
            NodeKind::GlobalPath => {
                self.action = match self.action {
                    NodeAction::None => NodeAction::Delete,
                    NodeAction::Delete => NodeAction::None,
                    NodeAction::Clean => NodeAction::Delete,
                };
            }
        }
    }

    pub fn can_be_marked(&self) -> bool {
        self.kind != NodeKind::Section
    }
}

#[derive(Debug, Clone)]
pub struct GlobalTreeEntry {
    pub label: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug)]
pub struct Tree {
    pub roots: Vec<TreeNode>,
}

impl Tree {
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    pub fn build(repo_paths: Vec<PathBuf>, global_entries: Vec<GlobalTreeEntry>) -> Self {
        let mut tree = Self::new();
        if !repo_paths.is_empty() {
            let mut repo_section = TreeNode::new(
                "repos".to_string(),
                "Repositories".to_string(),
                NodeKind::Section,
                true,
                0,
                PathBuf::new(),
            );

            for repo_path in repo_paths {
                let repo_name = repo_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let id = format!("repos/{}", repo_path.to_string_lossy());
                repo_section.children.push(TreeNode::new(
                    id,
                    repo_name,
                    NodeKind::Repo,
                    true,
                    1,
                    repo_path,
                ));
                if let Some(repo_node) = repo_section.children.last_mut() {
                    repo_node.repo_estimate_status = Some(RepoEstimateStatus::default());
                }
            }

            tree.roots.push(repo_section);
        }

        if !global_entries.is_empty() {
            let mut global_section = TreeNode::new(
                "globals".to_string(),
                "Global Dev Files (macOS)".to_string(),
                NodeKind::Section,
                true,
                0,
                PathBuf::new(),
            );

            for entry in global_entries {
                let id = format!("globals/{}", entry.path.to_string_lossy());
                let mut node = TreeNode::new(
                    id,
                    entry.label,
                    NodeKind::GlobalPath,
                    entry.is_dir,
                    1,
                    entry.path,
                );
                node.size = entry.size;
                node.size_done = entry.size.is_some();
                global_section.children.push(node);
            }

            tree.roots.push(global_section);
        }
        tree
    }

    pub fn flatten_visible(&self) -> Vec<&TreeNode> {
        let mut result = Vec::new();
        for root in &self.roots {
            flatten_node(root, &mut result);
        }
        result
    }

    pub fn get_node_mut_by_id(&mut self, id: &str) -> Option<&mut TreeNode> {
        for root in &mut self.roots {
            if let Some(node) = find_node_mut(root, id) {
                return Some(node);
            }
        }
        None
    }

    pub fn get_marked_nodes(&self) -> Vec<&TreeNode> {
        let mut marked = Vec::new();
        for root in &self.roots {
            collect_marked(root, &mut marked);
        }
        marked
    }

    pub fn get_repo_node_mut_by_path(&mut self, repo_path: &Path) -> Option<&mut TreeNode> {
        self.roots
            .iter_mut()
            .flat_map(|root| root.children.iter_mut())
            .find(|node| node.kind == NodeKind::Repo && node.path == repo_path)
    }

    pub fn get_global_node_mut_by_path(&mut self, target_path: &Path) -> Option<&mut TreeNode> {
        self.roots
            .iter_mut()
            .flat_map(|root| root.children.iter_mut())
            .find(|node| node.kind == NodeKind::GlobalPath && node.path == target_path)
    }

    pub fn update_repo_estimate(
        &mut self,
        repo_path: &Path,
        clean_size: Option<u64>,
        delete_size: Option<u64>,
    ) -> bool {
        let Some(node) = self.get_repo_node_mut_by_path(repo_path) else {
            return false;
        };

        node.clean_size = clean_size;
        node.delete_size = delete_size;
        let mut status = node.repo_estimate_status.unwrap_or_default();
        status.clean_done = true;
        status.delete_done = true;
        node.repo_estimate_status = Some(status);
        true
    }

    pub fn update_global_size(&mut self, target_path: &Path, size: Option<u64>) -> bool {
        let Some(node) = self.get_global_node_mut_by_path(target_path) else {
            return false;
        };

        node.size = size;
        node.size_done = true;
        true
    }

    pub fn repo_count(&self) -> usize {
        self.roots
            .iter()
            .filter(|n| n.kind == NodeKind::Section && n.id == "repos")
            .map(|n| n.children.len())
            .sum()
    }

    pub fn global_target_count(&self) -> usize {
        self.roots
            .iter()
            .filter(|n| n.kind == NodeKind::Section && n.id == "globals")
            .map(|n| n.children.len())
            .sum()
    }
}

fn flatten_node<'a>(node: &'a TreeNode, result: &mut Vec<&'a TreeNode>) {
    result.push(node);

    if node.is_dir && node.is_expanded {
        for child in &node.children {
            flatten_node(child, result);
        }
    }
}

fn find_node_mut<'a>(node: &'a mut TreeNode, id: &str) -> Option<&'a mut TreeNode> {
    if node.id == id {
        return Some(node);
    }

    for child in &mut node.children {
        if let Some(found) = find_node_mut(child, id) {
            return Some(found);
        }
    }

    None
}

fn collect_marked<'a>(node: &'a TreeNode, marked: &mut Vec<&'a TreeNode>) {
    if node.can_be_marked() && node.action != NodeAction::None {
        marked.push(node);
    }

    for child in &node.children {
        collect_marked(child, marked);
    }
}

fn field_display(value: Option<u64>, done: bool) -> String {
    if !done {
        "...".to_string()
    } else {
        match value {
            Some(bytes) => format_size(bytes),
            None => "?".to_string(),
        }
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}
