//! Parallel filesystem scanning: walks a directory tree using `rayon`,
//! building a [`Node`] tree while reporting progress and skipping
//! unreadable entries and symlinks.

use crate::model::Node;
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

/// Progress update sent from the scanner thread to the UI thread.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub scanned_entries: u64,
    pub errors: u64,
}

/// Result of a completed scan.
pub struct ScanResult {
    pub root: Node,
    pub errors: u64,
}

static SCANNED: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

/// Scan the given path recursively, building a size-tree. Sends periodic
/// progress updates on `progress_tx`. Symlinks are not followed (to avoid
/// cycles / double counting) and unreadable entries are skipped and counted
/// as errors instead of aborting the whole scan.
pub fn scan(root_path: &Path, progress_tx: Sender<ScanProgress>) -> ScanResult {
    SCANNED.store(0, Ordering::Relaxed);
    ERRORS.store(0, Ordering::Relaxed);

    // Spawn a lightweight ticker thread that reports progress every so often
    // while the parallel scan runs. We do this by polling the atomics.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let ticker_progress_tx = progress_tx.clone();
    let ticker = std::thread::spawn(move || loop {
        let scanned = SCANNED.load(Ordering::Relaxed);
        let errors = ERRORS.load(Ordering::Relaxed);
        let _ = ticker_progress_tx.send(ScanProgress {
            scanned_entries: scanned,
            errors,
        });
        if done_rx.recv_timeout(std::time::Duration::from_millis(100)).is_ok() {
            break;
        }
    });

    let root = scan_dir(root_path);

    let _ = done_tx.send(());
    let _ = ticker.join();

    let errors = ERRORS.load(Ordering::Relaxed);
    // final progress update
    let _ = progress_tx.send(ScanProgress {
        scanned_entries: SCANNED.load(Ordering::Relaxed),
        errors,
    });

    ScanResult { root, errors }
}

fn scan_dir(path: &Path) -> Node {
    let entries: Vec<_> = match fs::read_dir(path) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Node::new_dir(path.to_path_buf(), Vec::new());
        }
    };

    let children: Vec<Node> = entries
        .into_par_iter()
        .filter_map(|entry| scan_entry(&entry))
        .collect();

    Node::new_dir(path.to_path_buf(), children)
}

fn scan_entry(entry: &fs::DirEntry) -> Option<Node> {
    let path = entry.path();
    let meta = match entry.metadata() {
        // metadata() on DirEntry does not follow symlinks on unix
        Ok(m) => m,
        Err(_) => {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    };

    SCANNED.fetch_add(1, Ordering::Relaxed);

    if meta.is_symlink() {
        // Don't follow symlinks: avoid cycles and double counting.
        return None;
    }

    if meta.is_dir() {
        Some(scan_dir(&path))
    } else {
        Some(Node::new_file(path, meta.len()))
    }
}

/// Rescan a single path (e.g. after deleting something) and return the
/// refreshed Node, updating the error counter passed in.
pub fn rescan_path(path: &Path) -> Node {
    if path.is_dir() {
        scan_dir(path)
    } else {
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Node::new_file(path.to_path_buf(), size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn scans_nested_dirs_and_sums_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.txt"), b"hello").unwrap(); // 5 bytes
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.txt"), b"world!").unwrap(); // 6 bytes

        let (tx, _rx) = channel();
        let result = scan(root, tx);
        assert_eq!(result.root.size, 11);
        assert_eq!(result.root.children.len(), 2);
        let sub_node = result
            .root
            .children
            .iter()
            .find(|c| c.name == "sub")
            .unwrap();
        assert_eq!(sub_node.size, 6);
    }

    #[test]
    fn skips_unreadable_dirs_without_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let locked = root.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("secret.txt"), b"secret").unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&locked, perms.clone()).unwrap();

        let (tx, _rx) = channel();
        let result = scan(root, tx);

        // restore perms so tempdir cleanup can remove it
        perms.set_mode(0o755);
        fs::set_permissions(&locked, perms).unwrap();

        assert!(result.errors >= 1);
    }
}
