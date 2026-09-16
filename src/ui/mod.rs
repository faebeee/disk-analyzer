//! Rendering layer, built with `ratatui`. Draws the tree-list browser, the
//! selected-entry info panel, the keybinding footer, and modal dialogs
//! (scanning/error screens, delete confirmation) — all as a pure function
//! of [`App`] state.

use crate::app::{App, Mode};
use humansize::{format_size, BINARY};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    match &app.mode {
        Mode::Scanning => {
            draw_scanning(frame, app, size);
            return;
        }
        Mode::Error(msg) => {
            draw_error(frame, msg, size);
            return;
        }
        _ => {}
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(3)])
        .split(size);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(chunks[0]);

    draw_treelist(frame, app, main_chunks[0]);
    draw_info_panel(frame, app, main_chunks[1]);
    draw_footer(frame, app, chunks[1]);

    if matches!(app.mode, Mode::ConfirmDelete) {
        draw_confirm_modal(frame, app, size);
    }
}

fn draw_scanning(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Disk Analyzer — scanning… ")
        .borders(Borders::ALL);
    let text = vec![
        Line::from(format!("Entries scanned: {}", app.progress.scanned_entries)),
        Line::from(format!("Errors (skipped): {}", app.progress.errors)),
        Line::from(""),
        Line::from("Please wait, building size tree..."),
    ];
    let para = Paragraph::new(text).block(block).alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn draw_error(frame: &mut Frame, msg: &str, area: Rect) {
    let block = Block::default().title(" Error ").borders(Borders::ALL);
    let para = Paragraph::new(msg.to_string())
        .block(block)
        .style(Style::default().fg(Color::Red))
        .alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn draw_treelist(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(format!(" {} ", app.breadcrumb()))
        .borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let root = app.zoom_root();
    let total = root.size.max(1);
    let selected_idx = app.selection.first().copied();

    // Reserve space for " 100.0% 999.9 GiB  " prefix, leave the rest for the bar + name.
    let stats_width: u16 = 20;
    let bar_width: usize = inner
        .width
        .saturating_sub(stats_width)
        .clamp(4, 30) as usize;

    let items: Vec<ListItem> = root
        .children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let pct = child.size as f64 / total as f64;
            let filled = ((pct * bar_width as f64).round() as usize).min(bar_width);
            let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(bar_width - filled));
            let size_str = format_size(child.size, BINARY);
            let suffix = if child.is_dir { "/" } else { "" };
            let line = format!(
                "{:>6.1}% {:>10} {} {}{}",
                pct * 100.0,
                size_str,
                bar,
                child.name,
                suffix
            );

            let is_selected = Some(i) == selected_idx;
            let style = if is_selected {
                Style::default()
                    .bg(Color::White)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if child.is_dir {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(selected_idx);

    let list = List::new(items);
    frame.render_stateful_widget(list, inner, &mut state);
}

fn draw_info_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().title(" Selected ").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    let root_size = app.zoom_root().size.max(1);

    if let Some(node) = app.selected_node() {
        let pct = (node.size as f64 / root_size as f64) * 100.0;
        lines.push(Line::from(vec![Span::styled(
            node.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(format!(
            "Type: {}",
            if node.is_dir { "folder" } else { "file" }
        )));
        lines.push(Line::from(format!(
            "Size: {}",
            format_size(node.size, BINARY)
        )));
        lines.push(Line::from(format!("% of view: {:.1}%", pct)));
        if node.is_dir {
            lines.push(Line::from(format!("Items: {}", node.children.len())));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(node.path.to_string_lossy().to_string()));
    } else {
        lines.push(Line::from("No selection"));
        lines.push(Line::from(format!(
            "Total size: {}",
            format_size(app.zoom_root().size, BINARY)
        )));
        lines.push(Line::from(format!(
            "Items: {}",
            app.zoom_root().children.len()
        )));
    }

    if let Some(status) = &app.status {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            status.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    if app.scan_errors > 0 {
        lines.push(Line::from(Span::styled(
            format!("{} items skipped (permission errors)", app.scan_errors),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}

fn draw_footer(frame: &mut Frame, _app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let text = "↑/↓ move  Enter/→ zoom in  Backspace/←/Esc zoom out  x delete  r refresh  q quit";
    let para = Paragraph::new(text).block(block);
    frame.render_widget(para, area);
}

fn draw_confirm_modal(frame: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(50, 20, area);
    frame.render_widget(Clear, popup);
    let name = app
        .selected_node()
        .map(|n| n.path.to_string_lossy().to_string())
        .unwrap_or_default();
    let block = Block::default()
        .title(" Confirm delete ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Red));
    let text = vec![
        Line::from(format!("Delete: {}", name)),
        Line::from(""),
        Line::from("This cannot be undone."),
        Line::from(""),
        Line::from("Press 'y' to confirm, 'n' or Esc to cancel."),
    ];
    let para = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    frame.render_widget(para, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::model::Node;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn file(name: &str, size: u64) -> Node {
        Node::new_file(PathBuf::from(name), size)
    }

    #[test]
    fn draws_treelist_without_panicking_and_prints_it() {
        let root = Node::new_dir(
            PathBuf::from("root"),
            vec![
                Node::new_dir(
                    PathBuf::from("big_folder"),
                    vec![file("video.mp4", 8_000_000_000), file("notes.txt", 200)],
                ),
                Node::new_dir(
                    PathBuf::from("photos"),
                    vec![file("a.jpg", 3_000_000_000), file("b.jpg", 2_500_000_000)],
                ),
                file("readme.md", 1_200),
            ],
        );
        let app = App::new(root);

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        println!("{out}");
        assert!(out.contains("big_folder"));
        assert!(out.contains("photos"));
    }
}
