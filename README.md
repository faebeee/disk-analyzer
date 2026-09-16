# disk-analyzer

A fast, interactive terminal (TUI) tool for finding what's eating your disk
space. It recursively scans a directory in parallel, then lets you browse the
results as a sorted, ncdu-style tree list — drilling into folders, comparing
sizes at a glance via inline bar charts, and deleting files or folders
directly from the UI.

```
┌ /home/fabs/Work/disk-analyzer ────────────────────────────────────┐┌ Selected ──────────────────┐
│  59.3%   7.45 GiB [##################------------] big_folder/    ││big_folder                   │
│  40.7%   5.12 GiB [############------------------] photos/        ││Type: folder                 │
│   0.0%   1.17 KiB [------------------------------] readme.md      ││Size: 7.45 GiB               │
│                                                                    ││% of view: 59.3%             │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
│↑/↓ move  Enter/→ zoom in  Backspace/←/Esc zoom out  x delete  q quit                              │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Features

- **Parallel scanning** — walks the filesystem using [`rayon`](https://crates.io/crates/rayon)
  so large directory trees scan quickly on multi-core machines.
- **Live progress** — while scanning, the UI shows a running count of scanned
  entries and skipped (unreadable) entries.
- **Tree-list browser** — each directory level is shown as a flat, size-sorted
  list (largest first) with a percentage, human-readable size, and a
  proportional bar, similar to [`ncdu`](https://dev.yorhel.nl/ncdu) / WinDirStat.
- **Zoom navigation** — zoom into a folder to make it the new root of the
  view, and zoom back out; a breadcrumb in the panel title always shows the
  current path.
- **Safe deletion** — delete the selected file or folder (recursively) after
  an explicit confirmation prompt; the in-memory tree updates immediately
  without a full rescan.
- **Symlink-safe** — symlinks are never followed, avoiding cycles and
  double-counting of sizes.
- **Resilient to permission errors** — unreadable files/directories are
  skipped and counted instead of aborting the whole scan.

## Installation

### Prerequisites

- Rust toolchain (edition 2024, so a recent stable release — see
  [rustup.rs](https://rustup.rs) if you don't have one installed).

### Build from source

```sh
git clone <this-repo-url>
cd disk-analyzer
cargo build --release
```

The compiled binary will be at `target/release/disk-analyzer`. Optionally
install it onto your `PATH`:

```sh
cargo install --path .
```

## Usage

```sh
disk-analyzer [PATH]
```

- `PATH` — the directory to scan. Defaults to the current directory (`.`) if
  omitted.

Examples:

```sh
# Scan the current directory
disk-analyzer

# Scan your home directory
disk-analyzer ~

# Scan a specific path
disk-analyzer /var/log
```

The scan runs in the background; the UI displays a progress screen until it
completes, then switches to the interactive browser.

### Keybindings

| Key                        | Action                                   |
| -------------------------- | ----------------------------------------- |
| `↑` / `↓`                  | Move selection up/down within the list    |
| `Enter` / `→`               | Zoom into the selected folder              |
| `Backspace` / `←` / `Esc`   | Zoom out to the parent folder              |
| `x` / `Delete`              | Delete the selected file or folder         |
| `y`                         | Confirm deletion (while prompted)          |
| `n` / `Esc`                 | Cancel deletion (while prompted)           |
| `q`                         | Quit                                       |

Deleting a folder removes it (and everything inside it) recursively from
disk — use with care. You'll always be shown a confirmation prompt with the
full path before anything is deleted.

## How it works

### Scanning (`src/scanner.rs`)

`scan()` recursively walks the target directory. Each directory's entries are
processed in parallel via `rayon`'s `into_par_iter`, and a background
"ticker" thread periodically reports progress (entries scanned, errors so
far) back to the UI over an `mpsc` channel so the progress screen can update
smoothly. Symlinks are detected via `DirEntry::metadata()` (which does not
follow symlinks on Unix) and skipped entirely. Unreadable files/directories
increment an error counter rather than failing the scan.

### Data model (`src/model.rs`)

The scan result is a `Node` tree:

```rust
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,        // for dirs: sum of all descendant sizes
    pub children: Vec<Node>, // always kept sorted by size, descending
}
```

Directories' children are kept sorted largest-first so the tree-list view
never needs to re-sort at render time.

### Application state (`src/app.rs`)

`App` tracks:

- `root` — the full scanned tree.
- `zoom_path` — a chain of child indices from `root` down to the directory
  currently "zoomed" into (i.e. the root of the current view).
- `selection` — the index of the currently highlighted child within the
  zoomed directory.
- `mode` — `Scanning`, `Browsing`, `ConfirmDelete`, or `Error`.

Navigation (`move_sibling`, `zoom_in`, `zoom_out`) and post-delete tree
mutation (`remove_path`, `clamp_selection`) all operate on these index
chains, so the UI layer stays a thin rendering layer over `App`.

### UI (`src/ui/mod.rs`)

Built with [`ratatui`](https://ratatui.rs) on a `crossterm` backend:

- `draw_treelist` renders the current directory's children as a `List`
  widget — one row per entry with an inline bar proportional to its share of
  the zoomed directory's total size, a percentage, a human-readable size
  (via `humansize`), and the entry's name (directories suffixed with `/`).
  The selected row is highlighted and the list auto-scrolls to keep it in
  view.
- `draw_info_panel` shows details (type, size, % of the current view, child
  count, full path) for the selected entry.
- `draw_footer` shows the keybinding cheat-sheet.
- `draw_confirm_modal` shows the delete confirmation dialog.

### Event handling (`src/events.rs`)

Translates raw key events into `App` navigation calls (or an `Action::Quit` /
`Action::DeleteConfirmed` signal handled by `main.rs`).

### Entry point (`src/main.rs`)

Parses CLI args with `clap`, spawns the scanner on a background thread,
initializes the terminal (raw mode + alternate screen via `crossterm`), and
runs the main draw/poll-input loop, tearing the terminal back down on exit
(even on error, so your shell isn't left in a broken state).

## Project layout

```
src/
├── main.rs      # CLI parsing, terminal setup/teardown, main event loop
├── app.rs       # Application state and navigation logic
├── model.rs     # Node tree data structure
├── scanner.rs   # Parallel filesystem scanning
├── events.rs    # Keyboard input -> App actions
└── ui/
    └── mod.rs   # ratatui rendering (tree list, info panel, footer, modal)
```

## Development

Run the test suite:

```sh
cargo test
```

Lint:

```sh
cargo clippy
```

Run against a directory without installing:

```sh
cargo run --release -- /path/to/scan
```

### Testing notes

- `scanner` tests use `tempfile` to create real directory trees (including a
  permission-restricted directory) and assert sizes/error counts.
- `ui` tests render into a `ratatui::backend::TestBackend` and assert on the
  resulting buffer contents, so UI regressions are caught without a real
  terminal.

## Limitations / known caveats

- Symlinks are never followed, so symlinked directories' contents are not
  included in size totals.
- Hard links are counted at every location they appear, so total sizes may
  overcount space actually used on disk when hard links are present.
- Deletion is immediate after confirmation — there is no trash/recycle bin
  integration.

## License

No license file is currently included in this repository. Add one (e.g.
MIT/Apache-2.0) before distributing or open-sourcing this project.
