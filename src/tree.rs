use std::path::{Path, PathBuf};

/// A node in the tree structure
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,        // Unique identifier (full path relative to repo root)
    pub label: String,     // Display name (just the file/directory name)
    pub is_dir: bool,      // Is this a directory?
    pub is_expanded: bool, // Is this directory expanded?
    pub is_selected: bool, // Is this node currently selected (for navigation)?
    pub is_marked: bool,   // Is this node marked for deletion?
    pub depth: usize,      // Depth in the tree (for indentation)
    pub children: Vec<TreeNode>,
    pub repo_path: PathBuf, // Path to the git repository root
}

impl TreeNode {
    pub fn new(id: String, label: String, is_dir: bool, depth: usize, repo_path: PathBuf) -> Self {
        Self {
            id,
            label,
            is_dir,
            is_expanded: true, // Directories start expanded
            is_selected: false,
            is_marked: false,
            depth,
            children: Vec::new(),
            repo_path,
        }
    }

    /// Toggle expansion state for directories
    pub fn toggle_expansion(&mut self) {
        if self.is_dir {
            self.is_expanded = !self.is_expanded;
        }
    }

    /// Toggle marked state
    pub fn toggle_marked(&mut self) {
        self.is_marked = !self.is_marked;
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

            tree.roots.push(root);
        }

        tree
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
    if node.is_marked {
        marked.push(node);
    }

    for child in &node.children {
        collect_marked(child, marked);
    }
}

fn clear_selections_recursive(node: &mut TreeNode) {
    node.is_selected = false;
    for child in &mut node.children {
        clear_selections_recursive(child);
    }
}
