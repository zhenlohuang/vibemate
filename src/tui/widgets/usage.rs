use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::agent::{UsageInfo, UsageWindow};
use crate::cli::usage::{derive_display_name, should_display_window};
use chrono::{DateTime, Datelike, Local, Timelike};

/// Count visible rows for one agent: 4 lines per window + 1 separator if both groups present.
fn card_rows(item: &UsageInfo) -> usize {
    let (normal, extra) = split_windows(item);
    let total = normal.len() + extra.len();
    if total == 0 {
        return 0;
    }
    total * 4
}

fn split_windows(item: &UsageInfo) -> (Vec<&UsageWindow>, Vec<&UsageWindow>) {
    let visible: Vec<_> = item
        .windows
        .iter()
        .filter(|w| should_display_window(w))
        .collect();
    let normal: Vec<_> = visible.iter().filter(|w| !w.is_extra).copied().collect();
    let extra: Vec<_> = visible.iter().filter(|w| w.is_extra).copied().collect();
    (normal, extra)
}

/// Compute the height needed: card border (2) + max card rows.
/// Returns at least 3 so the "no data" message is visible.
pub fn needed_height(usage_items: &[UsageInfo]) -> u16 {
    let max_rows = usage_items.iter().map(card_rows).max().unwrap_or(0);
    if max_rows == 0 {
        return 3;
    }
    (2 + max_rows as u16).max(3)
}

pub fn render(frame: &mut Frame, area: Rect, usage_items: &[UsageInfo]) {
    if usage_items.is_empty() {
        let empty = Paragraph::new("No usage data yet. Press r to refresh.");
        frame.render_widget(empty, area);
        return;
    }

    let constraints: Vec<Constraint> = (0..usage_items.len())
        .map(|_| Constraint::Ratio(1, usage_items.len() as u32))
        .collect();
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (index, item) in usage_items.iter().enumerate() {
        let card_area = cards[index];
        let agent_title = if item.display_name.trim().is_empty() {
            item.agent_name.clone()
        } else {
            item.display_name.clone()
        };
        let title = item
            .plan
            .as_ref()
            .map(|plan| format!("{agent_title} ({plan})"))
            .unwrap_or(agent_title);

        let card_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(Style::default().bg(Color::Rgb(35, 40, 55)));
        frame.render_widget(card_block.clone(), card_area);
        let card_inner = card_block.inner(card_area);

        let (normal, extra) = split_windows(item);

        if normal.is_empty() && extra.is_empty() {
            frame.render_widget(Paragraph::new("No windows reported"), card_inner);
            continue;
        }

        // Build row constraints: 4 lines per window
        let mut row_constraints = Vec::new();
        for _ in &normal {
            row_constraints.push(Constraint::Length(1)); // title
            row_constraints.push(Constraint::Length(1)); // bar + percent
            row_constraints.push(Constraint::Length(1)); // reset text
            row_constraints.push(Constraint::Length(1)); // spacer
        }
        for _ in &extra {
            row_constraints.push(Constraint::Length(1));
            row_constraints.push(Constraint::Length(1));
            row_constraints.push(Constraint::Length(1));
            row_constraints.push(Constraint::Length(1));
        }
        row_constraints.push(Constraint::Min(0)); // absorb remaining space

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(card_inner);

        let mut row_idx = 0;

        // Render normal quotas
        for window in &normal {
            render_window(frame, &rows, row_idx, &item.agent_name, window);
            row_idx += 4;
        }

        // Render extra quotas
        for window in &extra {
            render_window(frame, &rows, row_idx, &item.agent_name, window);
            row_idx += 4;
        }
    }
}

fn render_window(
    frame: &mut Frame,
    rows: &[Rect],
    base: usize,
    agent_name: &str,
    window: &UsageWindow,
) {
    let percent = window.utilization_pct.clamp(0.0, 100.0).round() as u16;
    let title_text = derive_display_name(agent_name, window);

    // Line 1: Bold title
    let name_line = Paragraph::new(Line::from(Span::styled(
        title_text,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(name_line, rows[base]);

    // Line 2: Left bar + right percentage text
    let bar_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(2),
            Constraint::Length(10),
        ])
        .split(rows[base + 1]);

    let gauge = Gauge::default()
        .style(Style::default().bg(Color::Rgb(86, 90, 123)))
        .gauge_style(
            Style::default()
                .fg(Color::Rgb(161, 167, 229))
                .bg(Color::Rgb(86, 90, 123)),
        )
        .percent(percent)
        .label("");
    frame.render_widget(gauge, bar_row[0]);

    let percent_label = Paragraph::new(Line::from(Span::styled(
        format!("{percent}% used"),
        Style::default().fg(Color::White),
    )));
    frame.render_widget(percent_label, bar_row[2]);

    // Line 3: Human-readable reset time in muted gray
    let reset_text = format_reset_text(window.resets_at.as_deref());
    let reset_line = Paragraph::new(Span::styled(
        reset_text,
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(reset_line, rows[base + 2]);

    // Line 4: spacer (empty)
}

fn format_reset_text(reset: Option<&str>) -> String {
    let Some(reset_raw) = reset else {
        return String::new();
    };

    let parsed = DateTime::parse_from_rfc3339(reset_raw);
    let Ok(parsed) = parsed else {
        return format!("Resets {reset_raw}");
    };

    let local_dt = parsed.with_timezone(&Local);
    let now = Local::now();

    let time_text = format_clock_time(local_dt);
    let offset_text = format_utc_offset(local_dt);

    if local_dt.date_naive() == now.date_naive() {
        return format!("Resets {time_text} ({offset_text})");
    }
    if local_dt.year() == now.year() {
        return format!(
            "Resets {} at {} ({offset_text})",
            local_dt.format("%b %-d"),
            time_text
        );
    }

    format!(
        "Resets {} {} ({offset_text})",
        local_dt.format("%Y-%m-%d"),
        time_text
    )
}

fn format_clock_time(dt: DateTime<Local>) -> String {
    if dt.minute() == 0 {
        dt.format("%-I%P").to_string()
    } else {
        dt.format("%-I:%M%P").to_string()
    }
}

fn format_utc_offset(dt: DateTime<Local>) -> String {
    let total_seconds = dt.offset().local_minus_utc();
    let sign = if total_seconds >= 0 { '+' } else { '-' };
    let abs = total_seconds.abs();
    let hours = abs / 3600;
    let minutes = (abs % 3600) / 60;
    format!("UTC{sign}{hours:02}:{minutes:02}")
}
