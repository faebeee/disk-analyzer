//! The `Node` tree: an in-memory representation of a scanned filesystem
//! subtree, with sizes rolled up from children to parents.

use std::path::PathBuf;

/// A node in the filesystem size-tree.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Total size in bytes (for dirs: sum of all descendants).
    pub size: u64,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new_file(path: PathBuf, size: u64) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        Node {
            name,
            path,
            is_dir: false,
            size,
            children: Vec::new(),
        }
    }

    pub fn new_dir(path: PathBuf, children: Vec<Node>) -> Self {
        let size = children.iter().map(|c| c.size).sum();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let mut children = children;
        children.sort_by(|a, b| b.size.cmp(&a.size));
        Node {
            name,
            path,
            is_dir: true,
            size,
            children,
        }
    }

    /// Recompute this node's size and re-sort children by size descending.
    pub fn recompute(&mut self) {
        for c in &mut self.children {
            if c.is_dir {
                c.recompute();
            }
        }
        self.children.sort_by(|a, b| b.size.cmp(&a.size));
        if self.is_dir {
            self.size = self.children.iter().map(|c| c.size).sum();
        }
    }

    /// Remove a direct child by path, returns true if removed.
    pub fn remove_child(&mut self, path: &std::path::Path) -> bool {
        let before = self.children.len();
        self.children.retain(|c| c.path != path);
        self.children.len() != before
    }

    /// Find a mutable reference to the node matching `path`, searching this subtree.
    pub fn find_mut(&mut self, path: &std::path::Path) -> Option<&mut Node> {
        if self.path == path {
            return Some(self);
        }
        for c in &mut self.children {
            if let Some(found) = c.find_mut(path) {
                return Some(found);
            }
        }
        None
    }
}
