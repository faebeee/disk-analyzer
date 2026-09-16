//! Translates raw keyboard input into [`App`] navigation calls or
//! high-level [`Action`]s handled by the main loop.

use crate::app::{App, Mode};
use crossterm::event::{KeyCode, KeyEvent};

pub enum Action {
    None,
    Quit,
    DeleteConfirmed,
    Refresh,
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match app.mode {
        Mode::ConfirmDelete => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.mode = Mode::Browsing;
                return Action::DeleteConfirmed;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.mode = Mode::Browsing;
                app.status = Some("Delete cancelled".to_string());
            }
            _ => {}
        },
        Mode::Browsing => match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Action::Quit,
            KeyCode::Up => app.move_sibling(-1),
            KeyCode::Down => app.move_sibling(1),
            KeyCode::Enter | KeyCode::Right => app.zoom_in(),
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left => app.zoom_out(),
            KeyCode::Char('x') | KeyCode::Delete => {
                if app.selected_node().is_some() {
                    app.mode = Mode::ConfirmDelete;
                } else {
                    app.status = Some("Nothing selected".to_string());
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => return Action::Refresh,
            _ => {}
        },
        Mode::Scanning | Mode::Error(_) => {
            if let KeyCode::Char('q') = key.code {
                return Action::Quit;
            }
        }
    }
    Action::None
}
