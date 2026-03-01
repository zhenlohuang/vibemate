use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let status = if app.router_running { "ON" } else { "OFF" };
    let color = if app.router_running {
        Color::Green
    } else {
        Color::Red
    };

    let header = Line::from(vec![
        Span::raw("API Router: "),
        Span::styled(
            app.router_addr.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  ["),
        Span::styled(
            status,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("]"),
    ]);

    let log_line = Line::from(vec![
        Span::raw("Log: "),
        Span::styled(
            app.log_source.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut lines = vec![header, log_line];
    if let Some(note) = &app.log_source_note {
        lines.push(Line::from(Span::styled(
            note.clone(),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let widget =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(widget, area);
}
