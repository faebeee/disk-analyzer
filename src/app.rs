//! Application state: the scanned tree plus navigation (zoom/selection)
//! state, independent of any rendering concerns.

use crate::model::Node;
use crate::scanner::ScanProgress;
use std::path::{Path, PathBuf};

pub enum Mode {
    Scanning,
    Browsing,
    ConfirmDelete,
    Error(String),
}

/// Total/available space of the filesystem/mount that the scan root lives on.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskSpace {
    pub total: u64,
    pub available: u64,
}

impl DiskSpace {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used() as f64 / self.total as f64
        }
    }
}

pub struct App {
    pub root: Node,
    /// Chain of child-indices from `root` down to the current zoom root.
    pub zoom_path: Vec<usize>,
    /// Chain of child-indices from the current zoom root down to the current selection.
    pub selection: Vec<usize>,
    pub mode: Mode,
    pub progress: ScanProgress,
    pub scan_errors: u64,
    pub status: Option<String>,
    pub disk_space: DiskSpace,
}

impl App {
    pub fn new(root: Node) -> Self {
        let disk_space = disk_space_for(&root.path);
        App {
            root,
            zoom_path: Vec::new(),
            selection: Vec::new(),
            mode: Mode::Browsing,
            progress: ScanProgress {
                scanned_entries: 0,
                errors: 0,
            },
            scan_errors: 0,
            status: None,
            disk_space,
        }
    }

    /// The node currently at the center of the sunburst (the "zoomed" root).
    pub fn zoom_root(&self) -> &Node {
        let mut node = &self.root;
        for &idx in &self.zoom_path {
            match node.children.get(idx) {
                Some(child) => node = child,
                None => break,
            }
        }
        node
    }

    pub fn zoom_root_path(&self) -> PathBuf {
        self.zoom_root().path.clone()
    }

    /// Resolve the currently selected node (relative to zoom root), if any.
    pub fn selected_node(&self) -> Option<&Node> {
        if self.selection.is_empty() {
            return None;
        }
        let mut node = self.zoom_root();
        for &idx in &self.selection {
            node = node.children.get(idx)?;
        }
        Some(node)
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_node().map(|n| n.path.clone())
    }

    /// Breadcrumb string from filesystem root down to current zoom root.
    pub fn breadcrumb(&self) -> String {
        self.zoom_root().path.to_string_lossy().to_string()
    }

    // --- Navigation ---

    pub fn select_first_if_none(&mut self) {
        if self.selection.is_empty() && !self.zoom_root().children.is_empty() {
            self.selection.push(0);
        }
    }

    pub fn move_sibling(&mut self, delta: i32) {
        self.select_first_if_none();
        if self.selection.is_empty() {
            return;
        }
        let parent = self.parent_of_selection();
        let len = parent.children.len();
        if len == 0 {
            return;
        }
        let last = self.selection.last_mut().unwrap();
        let cur = *last as i32;
        let new_idx = ((cur + delta).rem_euclid(len as i32)) as usize;
        *last = new_idx;
    }

    fn parent_of_selection(&self) -> &Node {
        let mut node = self.zoom_root();
        for &idx in &self.selection[..self.selection.len() - 1] {
            if let Some(child) = node.children.get(idx) {
                node = child;
            }
        }
        node
    }

    /// Zoom into the currently selected directory, making it the new center.
    pub fn zoom_in(&mut self) {
        if let Some(node) = self.selected_node() {
            if node.is_dir && !node.children.is_empty() {
                self.zoom_path.extend(self.selection.iter().copied());
                self.selection.clear();
            }
        }
    }

    /// Zoom out one level (parent of current zoom root becomes the center).
    pub fn zoom_out(&mut self) {
        if self.zoom_path.pop().is_some() {
            self.selection.clear();
        }
    }

    // --- Mutation after delete ---

    /// Replace the node at `path` in the tree with `fresh` (result of a
    /// rescan), and recompute ancestor sizes. If fresh has size 0 and no
    /// children and doesn't exist anymore, the caller should instead call
    /// `remove_path`.
    pub fn refresh_path(&mut self, path: &Path, fresh: Node) {
        if let Some(node) = self.root.find_mut(path) {
            *node = fresh;
        }
        self.root.recompute();
        self.clamp_selection();
    }

    pub fn remove_path(&mut self, path: &Path) {
        remove_from(&mut self.root, path);
        self.root.recompute();
        self.clamp_selection();
    }

    /// Re-query total/available space for the mount containing the scan root.
    pub fn refresh_disk_space(&mut self) {
        self.disk_space = disk_space_for(&self.root.path);
    }

    fn clamp_selection(&mut self) {
        // Walk down and truncate/clamp selection indices to valid ranges.
        let mut node = self.zoom_root_or_none();
        if node.is_none() {
            self.zoom_path.clear();
            node = Some(&self.root);
        }
        let mut node = node.unwrap();
        let mut new_selection = Vec::new();
        for &idx in &self.selection {
            if node.children.is_empty() {
                break;
            }
            let clamped = idx.min(node.children.len() - 1);
            new_selection.push(clamped);
            node = &node.children[clamped];
        }
        self.selection = new_selection;
    }

    fn zoom_root_or_none(&self) -> Option<&Node> {
        let mut node = &self.root;
        for &idx in &self.zoom_path {
            node = node.children.get(idx)?;
        }
        Some(node)
    }
}

/// Query total/available space for the mount containing `path`. Falls back
/// to zeroed values if the underlying syscall fails (e.g. unsupported
/// platform or invalid path).
fn disk_space_for(path: &Path) -> DiskSpace {
    let total = fs4::total_space(path).unwrap_or(0);
    let available = fs4::available_space(path).unwrap_or(0);
    DiskSpace { total, available }
}

fn remove_from(node: &mut Node, path: &Path) -> bool {
    if node.remove_child(path) {
        return true;
    }
    for c in &mut node.children {
        if remove_from(c, path) {
            return true;
        }
    }
    false
}
