use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::proxy::RequestLog;

pub fn render(frame: &mut Frame, area: Rect, logs: &[RequestLog], scroll: usize) {
    let block = Block::default().borders(Borders::ALL).title("Request Logs");
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let available_rows = inner.height.saturating_sub(2) as usize;
    let start = scroll.min(logs.len().saturating_sub(1));

    let rows = logs.iter().skip(start).take(available_rows).map(|log| {
        Row::new(vec![
            Cell::from(log.timestamp.clone()),
            Cell::from(log.method.clone()),
            Cell::from(log.path.clone()),
            Cell::from(log.model.clone()),
            Cell::from(log.provider.clone()),
            Cell::from(log.status.to_string()),
            Cell::from(format!("{}ms", log.latency_ms)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(20),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec![
            "Time", "Method", "Path", "Model", "Provider", "Code", "Latency",
        ])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1);

    frame.render_widget(table, inner);
}
