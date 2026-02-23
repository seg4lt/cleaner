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

/// A node in the tree structure
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,        // Unique identifier (full path relative to repo root)
    pub label: String,     // Display name (just the file/directory name)
    pub is_dir: bool,      // Is this a directory?
    pub is_expanded: bool, // Is this directory expanded?
    pub is_selected: bool, // Is this node currently selected (for navigation)?
    pub action: NodeAction, // Action to run for this node
    pub depth: usize,      // Depth in the tree (for indentation)
    pub children: Vec<TreeNode>,
    pub repo_path: PathBuf, // Path to the git repository root
    pub size: Option<u64>,  // Size in bytes (None if not calculated yet)
}

impl TreeNode {
    pub fn new(id: String, label: String, is_dir: bool, depth: usize, repo_path: PathBuf) -> Self {
        Self {
            id,
            label,
            is_dir,
            is_expanded: true, // Directories start expanded
            is_selected: false,
            action: NodeAction::None,
            depth,
            children: Vec::new(),
            repo_path,
            size: None,
        }
    }

    /// Get size as human readable string
    pub fn size_str(&self) -> String {
        match self.size {
            Some(size) => format_size(size),
            None => String::new(),
        }
    }

    /// Calculate size for this node (file size or total directory size)
    pub fn calculate_size(&mut self) {
        let full_path = self.full_path();
        self.size = Some(calculate_path_size(&full_path));
    }

    /// Toggle expansion state for directories
    pub fn toggle_expansion(&mut self) {
        if self.is_dir {
            self.is_expanded = !self.is_expanded;
        }
    }

    /// Cycle action state (none -> clean -> delete -> none)
    pub fn cycle_action(&mut self) {
        // Top-level repo rows (depth 0) and regular nodes can be marked
        if self.depth == 0 || !self.id.is_empty() {
            self.action = self.action.cycle();
        }
    }

    /// Check if this node can be marked for deletion
    pub fn can_be_marked(&self) -> bool {
        self.depth == 0 || !self.id.is_empty()
    }

    /// Get the full absolute path
    pub fn full_path(&self) -> PathBuf {
        self.repo_path.join(&self.id)
    }
}

/// The tree structure containing all git repositories and their untracked files
#[derive(Debug)]
pub struct Tree {
    pub roots: Vec<TreeNode>,
}

impl Tree {
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    /// Build the tree from git repositories and their untracked files
    pub fn build(repo_data: Vec<(PathBuf, Vec<(PathBuf, bool)>)>) -> Self {
        let mut tree = Self::new();
        for (repo_path, untracked_files) in repo_data {
            tree.add_repo(repo_path, untracked_files);
        }
        tree
    }

    /// Add a single repository to the tree
    pub fn add_repo(&mut self, repo_path: PathBuf, untracked_files: Vec<(PathBuf, bool)>) {
        // Create a root node for this repository
        let repo_name = repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut root = TreeNode::new("".to_string(), repo_name, true, 0, repo_path.clone());

        // Build the tree structure from untracked files
        for (path, is_dir) in untracked_files {
            add_path_to_tree(&mut root, &path, is_dir, &repo_path);
        }

        // Sort children: directories first, then alphabetically
        sort_children(&mut root);

        // Flatten directories that only have single directory children
        flatten_empty_directories(&mut root);

        // Recalculate depths after flattening
        recalculate_depths(&mut root, 0);

        // Calculate sizes for all nodes
        calculate_sizes_recursive(&mut root);

        self.roots.push(root);
    }

    /// Flatten the tree into a list of visible nodes for rendering
    pub fn flatten_visible(&self) -> Vec<&TreeNode> {
        let mut result = Vec::new();
        for root in &self.roots {
            flatten_node(root, &mut result);
        }
        result
    }

    /// Get mutable reference to a node by its ID
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TreeNode> {
        for root in &mut self.roots {
            if let Some(node) = find_node_mut(root, id) {
                return Some(node);
            }
        }
        None
    }

    /// Get mutable reference to a node by repository and node ID
    pub fn get_node_mut_in_repo(&mut self, repo_path: &Path, id: &str) -> Option<&mut TreeNode> {
        for root in &mut self.roots {
            if root.repo_path == repo_path {
                return find_node_mut(root, id);
            }
        }
        None
    }

    /// Get all marked nodes for deletion
    pub fn get_marked_nodes(&self) -> Vec<&TreeNode> {
        let mut marked = Vec::new();
        for root in &self.roots {
            collect_marked(root, &mut marked);
        }
        marked
    }

    /// Clear all selections
    pub fn clear_selections(&mut self) {
        for root in &mut self.roots {
            clear_selections_recursive(root);
        }
    }

    /// Select a specific node by ID
    pub fn select_node(&mut self, id: &str) {
        self.clear_selections();
        if let Some(node) = self.get_node_mut(id) {
            node.is_selected = true;
        }
    }
}

fn add_path_to_tree(parent: &mut TreeNode, path: &Path, is_dir: bool, repo_path: &Path) {
    let components: Vec<_> = path.components().collect();

    if components.is_empty() {
        return;
    }

    // Build the tree structure recursively
    insert_path_components(parent, &components, 0, is_dir, repo_path);
}

fn insert_path_components(
    parent: &mut TreeNode,
    components: &[std::path::Component],
    depth: usize,
    final_is_dir: bool,
    repo_path: &Path,
) {
    if depth >= components.len() {
        return;
    }

    let component = &components[depth];
    let name = component.as_os_str().to_string_lossy().to_string();

    // Build the full path up to this component
    let partial_path: PathBuf = components.iter().take(depth + 1).collect();
    let path_str = partial_path.to_string_lossy().to_string();

    // Check if this component already exists as a child
    let child_index = parent.children.iter().position(|child| child.label == name);

    if let Some(index) = child_index {
        // Component exists, recurse into it
        if depth + 1 < components.len() {
            insert_path_components(
                &mut parent.children[index],
                components,
                depth + 1,
                final_is_dir,
                repo_path,
            );
        }
    } else {
        // Component doesn't exist, create it
        let is_last = depth == components.len() - 1;
        let is_dir = if is_last { final_is_dir } else { true };

        let mut new_node = TreeNode::new(
            path_str.clone(),
            name,
            is_dir,
            parent.depth + 1,
            repo_path.to_path_buf(),
        );

        // If there are more components, recurse
        if !is_last {
            insert_path_components(
                &mut new_node,
                components,
                depth + 1,
                final_is_dir,
                repo_path,
            );
        }

        parent.children.push(new_node);
    }
}

fn sort_children(node: &mut TreeNode) {
    // Sort: directories first, then by name
    node.children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.label.cmp(&b.label),
    });

    // Recursively sort children
    for child in &mut node.children {
        sort_children(child);
    }
}

/// Flatten directories that only have a single directory child (no files)
/// This creates paths like "a/b/c" instead of nested directories when there are no files at intermediate levels
fn flatten_empty_directories(node: &mut TreeNode) {
    // First recursively flatten children
    for child in &mut node.children {
        flatten_empty_directories(child);
    }

    // Check if this directory has only one child and that child is a directory
    if node.is_dir && node.children.len() == 1 {
        let child = &node.children[0];
        if child.is_dir {
            // Merge the child into this node
            let child_label = child.label.clone();
            let child_id = child.id.clone();
            let grand_children = child.children.clone();

            // Update this node's label to include the child
            node.label = format!("{}/{}", node.label, child_label);
            node.id = child_id;
            node.children = grand_children;

            // Recursively flatten again in case there are more single-dir chains
            flatten_empty_directories(node);
        }
    }
}

/// Recalculate depths for all nodes after flattening
fn recalculate_depths(node: &mut TreeNode, new_depth: usize) {
    node.depth = new_depth;
    for child in &mut node.children {
        recalculate_depths(child, new_depth + 1);
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

/// Format bytes to human readable string
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

/// Calculate total size of a path (file or directory)
fn calculate_path_size(path: &Path) -> u64 {
    if path.is_file() {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    } else if path.is_dir() {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                total += calculate_path_size(&entry.path());
            }
        }
        total
    } else {
        0
    }
}

fn clear_selections_recursive(node: &mut TreeNode) {
    node.is_selected = false;
    for child in &mut node.children {
        clear_selections_recursive(child);
    }
}

/// Calculate sizes for all nodes recursively
fn calculate_sizes_recursive(node: &mut TreeNode) {
    // First calculate sizes for all children
    for child in &mut node.children {
        calculate_sizes_recursive(child);
    }

    // Calculate this node's size
    if node.is_dir {
        if node.id.is_empty() {
            // Repo root should reflect the total size of displayed cleanup items, not the whole repo.
            let total: u64 = node.children.iter().filter_map(|c| c.size).sum();
            node.size = Some(total);
        } else {
            // Directory nodes may be represented without expanded children in the tree, so
            // compute their actual on-disk size directly.
            node.size = Some(calculate_path_size(&node.full_path()));
        }
    } else {
        // For files, get the file size
        let full_path = node.full_path();
        node.size = std::fs::metadata(&full_path).ok().map(|m| m.len());
    }
}
