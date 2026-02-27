use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::App;
use crate::tui::widgets::{logs, status, usage};

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(11),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    let header = Paragraph::new("vibemate v0.1.0")
        .block(Block::default().borders(Borders::ALL).title("Header"));
    frame.render_widget(header, chunks[0]);

    status::render(frame, chunks[1], app);
    usage::render(frame, chunks[2], &app.usage);
    let logs: Vec<_> = app.logs.iter().cloned().collect();
    logs::render(frame, chunks[3], &logs, app.log_scroll);

    let footer_text = app
        .status_message
        .clone()
        .unwrap_or_else(|| "q:quit  r:refresh  Tab:focus  j/k:scroll".to_string());

    let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, chunks[4]);
}
