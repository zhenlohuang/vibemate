use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::{ActivePage, App};
use crate::tui::widgets::{logs, status, usage};

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(7),
            Constraint::Length(2),
        ])
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
        ActivePage::Router => {
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Min(5)])
                .split(chunks[1]);
            status::render(frame, body[0], app);
            let log_items: Vec<_> = app.logs.iter().cloned().collect();
            logs::render(frame, body[1], &log_items, app.log_scroll);
        }
    }

    let footer = Paragraph::new(footer_line(app))
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

fn tab_line(app: &App) -> Line<'static> {
    let selected = match app.active_page {
        ActivePage::Usage => 0,
        ActivePage::Router => 1,
    };
    let tab_names = ["Usage", "API Proxy"];

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

    Line::from(spans)
}

fn footer_line(app: &App) -> Line<'static> {
    let mut spans = vec![
        Span::styled("Esc:quit", Style::default().fg(Color::White)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("Tab:switch page", Style::default().fg(Color::White)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("j/k:scroll", Style::default().fg(Color::White)),
    ];

    if let Some(message) = &app.status_message {
        spans.push(Span::raw("  |  "));
        spans.push(Span::styled(
            message.clone(),
            Style::default().fg(Color::Gray),
        ));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use super::render;
    use crate::tui::app::App;

    fn render_lines(app: &App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal should be created");
        terminal
            .draw(|frame| render(frame, app))
            .expect("render should succeed");
        buffer_to_lines(terminal.backend().buffer())
    }

    fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
        let area = *buffer.area();
        let mut lines = Vec::with_capacity(area.height as usize);
        for y in 0..area.height {
            let mut line = String::with_capacity(area.width as usize);
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line);
        }
        lines
    }

    #[test]
    fn footer_text_is_visible_with_top_border() {
        let app = App::new("http://127.0.0.1:12345".to_string());
        let output = render_lines(&app, 120, 20).join("\n");
        assert!(output.contains("Esc:quit"));
        assert!(output.contains("Tab:switch page"));
    }

    #[test]
    fn footer_keeps_separator_line() {
        let app = App::new("http://127.0.0.1:12345".to_string());
        let lines = render_lines(&app, 120, 20);
        let footer_border_row = &lines[18];
        assert!(footer_border_row.contains('─'));
    }
}
