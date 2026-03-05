use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::{ActivePage, App};
use crate::tui::widgets::{logs, status, usage};

pub fn render(frame: &mut Frame, app: &mut App) {
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
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(usage::needed_height(&app.usage)),
                    Constraint::Min(0),
                ])
                .split(chunks[1]);
            let usage_meta = usage::render(
                frame,
                body[0],
                &app.usage,
                app.usage_scroll,
                app.usage_selected_card,
            );
            app.set_usage_scroll_meta(usage_meta.max_scroll, usage_meta.page_step);
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

pub fn usage_widget_area(screen: Rect, app: &App) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(7),
            Constraint::Length(2),
        ])
        .split(screen);
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(usage::needed_height(&app.usage)),
            Constraint::Min(0),
        ])
        .split(chunks[1]);
    body[0]
}

fn tab_line(app: &App) -> Line<'static> {
    let selected = match app.active_page {
        ActivePage::Usage => 0,
        ActivePage::Router => 1,
    };
    let tab_names = ["Usage", "API Router"];

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
    ];
    match app.active_page {
        ActivePage::Usage => {
            let select_hint = if app.usage_selected_card.is_some() {
                "Tab:next card  Esc:clear selection"
            } else {
                "Enter/click:select usage card"
            };
            let scroll_hint = if let Some(selected) = app.usage_selected_card {
                format!(
                    "j/k:page scroll  wheel/↑↓:line scroll (selected {}/{})",
                    selected + 1,
                    app.usage.len().max(1)
                )
            } else {
                "j/k:page scroll  wheel/↑↓:line scroll (disabled until card selected)".to_string()
            };
            spans.push(Span::styled(select_hint, Style::default().fg(Color::White)));
            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled(scroll_hint, Style::default().fg(Color::White)));
        }
        ActivePage::Router => {
            spans.push(Span::styled(
                "j/k:scroll logs",
                Style::default().fg(Color::White),
            ));
        }
    }

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

    fn render_lines(mut app: App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal should be created");
        terminal
            .draw(|frame| render(frame, &mut app))
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
        let output = render_lines(app, 120, 20).join("\n");
        assert!(output.contains("Esc:quit"));
        assert!(output.contains("Tab:switch page"));
    }

    #[test]
    fn footer_keeps_separator_line() {
        let app = App::new("http://127.0.0.1:12345".to_string());
        let lines = render_lines(app, 120, 20);
        let footer_border_row = &lines[18];
        assert!(footer_border_row.contains('─'));
    }
}
