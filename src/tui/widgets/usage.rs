use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::oauth::UsageInfo;

pub fn render(frame: &mut Frame, area: Rect, usage_items: &[UsageInfo]) {
    let base_block = Block::default().borders(Borders::ALL).title("Quotas");
    frame.render_widget(base_block.clone(), area);
    let inner = base_block.inner(area);

    if usage_items.is_empty() {
        let empty = Paragraph::new("No usage data yet. Press r to refresh.");
        frame.render_widget(empty, inner);
        return;
    }

    let constraints: Vec<Constraint> = (0..usage_items.len())
        .map(|_| Constraint::Ratio(1, usage_items.len() as u32))
        .collect();
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    for (index, item) in usage_items.iter().enumerate() {
        let card_area = cards[index];
        let title = item
            .plan
            .as_ref()
            .map(|plan| format!("{} ({plan})", item.agent_name))
            .unwrap_or_else(|| item.agent_name.clone());

        let card_block = Block::default().borders(Borders::ALL).title(title);
        frame.render_widget(card_block.clone(), card_area);
        let card_inner = card_block.inner(card_area);

        if item.windows.is_empty() {
            frame.render_widget(Paragraph::new("No windows reported"), card_inner);
            continue;
        }

        let mut row_constraints = Vec::with_capacity(item.windows.len() + 1);
        row_constraints.push(Constraint::Length(1));
        row_constraints.extend((0..item.windows.len()).map(|_| Constraint::Length(2)));

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(card_inner);

        frame.render_widget(Paragraph::new(""), rows[0]);

        for (window_index, window) in item.windows.iter().enumerate() {
            let percent = window.utilization_pct.clamp(0.0, 100.0) as u16;
            let label = if let Some(reset) = &window.resets_at {
                format!("{} {:>3}% (reset {reset})", window.name, percent)
            } else {
                format!("{} {:>3}%", window.name, percent)
            };

            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(Color::Cyan))
                .percent(percent)
                .label(label);

            frame.render_widget(gauge, rows[window_index + 1]);
        }
    }
}
