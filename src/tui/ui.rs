use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::{ActivePage, App};
use crate::tui::widgets::{logs, status, usage};

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    // Tab bar (centered, custom style similar to screenshot)
    let tab_bar = Paragraph::new(tab_line(app))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(tab_bar, chunks[0]);

    // Page body
    match app.active_page {
        ActivePage::Usage => {
            let height = usage::needed_height(&app.usage);
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(height), Constraint::Min(0)])
                .split(chunks[1]);
            usage::render(frame, body[0], &app.usage);
        }
        ActivePage::Proxy => {
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(5)])
                .split(chunks[1]);
            status::render(frame, body[0], app);
            let log_items: Vec<_> = app.logs.iter().cloned().collect();
            logs::render(frame, body[1], &log_items, app.log_scroll);
        }
    }
}

fn tab_line(app: &App) -> Line<'static> {
    let selected = match app.active_page {
        ActivePage::Usage => 0,
        ActivePage::Proxy => 1,
    };
    let tab_names = ["Usage", "Proxy"];

    let mut spans = vec![];

    for (index, name) in tab_names.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        let style = if index == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(161, 167, 229))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        spans.push(Span::styled(format!(" {name} "), style));
    }

    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        "(Tab to cycle)",
        Style::default().fg(Color::DarkGray),
    ));

    Line::from(spans)
}
