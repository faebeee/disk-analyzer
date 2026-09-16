//! `disk-analyzer` — an interactive terminal UI for finding what's eating
//! your disk space.
//!
//! The program scans a directory tree in parallel on a background thread
//! (see [`scanner`]), builds an in-memory size tree (see [`model`]), and
//! then presents it as an interactive, ncdu-style tree list (see [`ui`])
//! that supports zooming into folders and deleting files/directories.
//!
//! See `README.md` for a full description of the keybindings and
//! architecture.

mod app;
mod events;
mod model;
mod scanner;
mod ui;

use app::{App, Mode};
use clap::Parser;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use scanner::ScanProgress;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "disk-analyzer",
    about = "Visualize folder sizes as an interactive terminal sunburst chart to find what's eating your disk space."
)]
struct Cli {
    /// Directory to scan (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let scan_root = cli.path.canonicalize().unwrap_or(cli.path.clone());

    if !scan_root.exists() {
        eprintln!("Path does not exist: {}", scan_root.display());
        std::process::exit(1);
    }

    // Kick off scanning in a background thread.
    let (progress_tx, progress_rx) = mpsc::channel::<ScanProgress>();
    let (result_tx, result_rx) = mpsc::channel::<scanner::ScanResult>();
    let scan_path = scan_root.clone();
    std::thread::spawn(move || {
        let result = scanner::scan(&scan_path, progress_tx);
        let _ = result_tx.send(result);
    });

    // Terminal setup.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_app(&mut terminal, progress_rx, result_rx);

    // Terminal teardown (always run, even on error).
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    progress_rx: mpsc::Receiver<ScanProgress>,
    result_rx: mpsc::Receiver<scanner::ScanResult>,
) -> anyhow::Result<()> {
    let mut app: Option<App> = None;
    let mut placeholder = App::new(model::Node::new_dir(PathBuf::from("."), Vec::new()));
    placeholder.mode = Mode::Scanning;

    loop {
        // Drain progress updates.
        if let Some(a) = app.as_mut() {
            if matches!(a.mode, Mode::Scanning) {
                while let Ok(p) = progress_rx.try_recv() {
                    a.progress = p;
                }
            }
        } else {
            while let Ok(p) = progress_rx.try_recv() {
                placeholder.progress = p;
            }
        }

        // Check if scan finished.
        if app.is_none() {
            match result_rx.try_recv() {
                Ok(result) => {
                    let mut new_app = App::new(result.root);
                    new_app.scan_errors = result.errors;
                    app = Some(new_app);
                }
                Err(TryRecvError::Disconnected) => {
                    placeholder.mode = Mode::Error("Scan thread crashed".to_string());
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let current: &App = app.as_ref().unwrap_or(&placeholder);
        terminal.draw(|f| ui::draw(f, current))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == crossterm::event::KeyEventKind::Press {
                    if let Some(a) = app.as_mut() {
                        match events::handle_key(a, key) {
                            events::Action::Quit => break,
                            events::Action::DeleteConfirmed => {
                                perform_delete(a);
                            }
                            events::Action::Refresh => {
                                perform_refresh(a);
                            }
                            events::Action::None => {}
                        }
                    } else if let crossterm::event::KeyCode::Char('q') = key.code {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn perform_delete(app: &mut App) {
    let Some(path) = app.selected_path() else {
        return;
    };
    let node = app.selected_node().cloned();
    let is_dir = node.map(|n| n.is_dir).unwrap_or(false);

    let result = if is_dir {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };

    match result {
        Ok(()) => {
            app.remove_path(&path);
            app.refresh_disk_space();
            app.status = Some(format!("Deleted {}", path.display()));
        }
        Err(e) => {
            app.status = Some(format!("Failed to delete {}: {}", path.display(), e));
        }
    }
}

fn perform_refresh(app: &mut App) {
    let path = app.zoom_root_path();
    let fresh = scanner::rescan_path(&path);
    app.refresh_path(&path, fresh);
    app.refresh_disk_space();
    app.status = Some(format!("Refreshed {}", path.display()));
}
