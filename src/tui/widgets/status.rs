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

    let text = Line::from(vec![
        Span::raw("API Proxy: "),
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

    let widget = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(widget, area);
}
