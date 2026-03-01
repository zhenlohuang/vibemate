use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::model_router::RequestLog;

pub fn render(frame: &mut Frame, area: Rect, logs: &[RequestLog], scroll: usize) {
    let block = Block::default().borders(Borders::ALL).title("Request Logs");
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);

    let available_rows = inner.height.saturating_sub(2) as usize;
    let max_top = logs.len().saturating_sub(available_rows);
    let top = max_top.saturating_sub(scroll.min(max_top));

    let rows = logs.iter().skip(top).take(available_rows).map(|log| {
        let model = if log.original_model.is_empty() {
            log.routed_model.clone()
        } else if log.routed_model.is_empty() || log.original_model == log.routed_model {
            log.original_model.clone()
        } else {
            format!("{} -> {}", log.original_model, log.routed_model)
        };
        let error = if log.error_summary.is_empty() {
            "-".to_string()
        } else {
            log.error_summary.clone()
        };
        Row::new(vec![
            Cell::from(log.timestamp.clone()),
            Cell::from(log.request_id.clone()),
            Cell::from(log.path.clone()),
            Cell::from(model),
            Cell::from(log.provider.clone()),
            Cell::from(log.status.to_string()),
            Cell::from(format!("{}ms", log.latency_ms)),
            Cell::from(error),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(29),
            Constraint::Length(8),
            Constraint::Length(18),
            Constraint::Length(64),
            Constraint::Length(16),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec![
            "Time", "RID", "Path", "Model", "Provider", "Code", "Latency", "Err",
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
