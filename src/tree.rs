use std::path::{Path, PathBuf};

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

/// A node in the tree structure.
/// The current app uses a repo-only list, but we keep a small generic node shape
/// so the existing tree widget can continue to render it.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub action: NodeAction,
    pub depth: usize,
    pub children: Vec<TreeNode>,
    pub repo_path: PathBuf,
    pub size: Option<u64>,
}

impl TreeNode {
    pub fn new(id: String, label: String, is_dir: bool, depth: usize, repo_path: PathBuf) -> Self {
        Self {
            id,
            label,
            is_dir,
            is_expanded: true,
            action: NodeAction::None,
            depth,
            children: Vec::new(),
            repo_path,
            size: None,
        }
    }

    pub fn size_str(&self) -> String {
        match self.size {
            Some(size) => format_size(size),
            None => String::new(),
        }
    }

    pub fn cycle_action(&mut self) {
        if self.depth == 0 || !self.id.is_empty() {
            self.action = self.action.cycle();
        }
    }

    pub fn can_be_marked(&self) -> bool {
        self.depth == 0 || !self.id.is_empty()
    }
}

#[derive(Debug)]
pub struct Tree {
    pub roots: Vec<TreeNode>,
}

impl Tree {
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    pub fn build(repo_paths: Vec<PathBuf>) -> Self {
        let mut tree = Self::new();
        for repo_path in repo_paths {
            tree.add_repo(repo_path);
        }
        tree
    }

    pub fn add_repo(&mut self, repo_path: PathBuf) {
        let repo_name = repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let root = TreeNode::new(String::new(), repo_name, true, 0, repo_path);
        self.roots.push(root);
    }

    pub fn flatten_visible(&self) -> Vec<&TreeNode> {
        let mut result = Vec::new();
        for root in &self.roots {
            flatten_node(root, &mut result);
        }
        result
    }

    pub fn get_node_mut_in_repo(&mut self, repo_path: &Path, id: &str) -> Option<&mut TreeNode> {
        for root in &mut self.roots {
            if root.repo_path == repo_path {
                return find_node_mut(root, id);
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
    if node.action != NodeAction::None {
        marked.push(node);
    }

    for child in &node.children {
        collect_marked(child, marked);
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
