use ratatui::prelude::*;
use ratatui::symbols;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use crate::agent::{UsageInfo, UsageWindow};
use crate::cli::usage::{derive_display_name, should_display_window};
use chrono::{DateTime, Datelike, Local, Timelike};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageRenderMeta {
    pub max_scroll: usize,
    pub page_step: usize,
}

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
        .filter(|w| {
            should_display_window(w) && !derive_display_name(&item.agent_name, w).trim().is_empty()
        })
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

pub fn render(
    frame: &mut Frame,
    area: Rect,
    usage_items: &[UsageInfo],
    scroll: usize,
    selected_card: Option<usize>,
) -> UsageRenderMeta {
    render_with_empty_message(
        frame,
        area,
        usage_items,
        scroll,
        selected_card,
        "No usage data yet. Press r to refresh.",
    )
}

pub fn render_static_lines(
    usage_items: &[UsageInfo],
    width: u16,
    empty_message: &str,
) -> Vec<String> {
    let Some(buffer) = render_static_buffer(usage_items, width, empty_message) else {
        return Vec::new();
    };
    buffer_to_lines(&buffer)
}

pub fn render_static_buffer(
    usage_items: &[UsageInfo],
    width: u16,
    empty_message: &str,
) -> Option<Buffer> {
    let width = width.max(1);
    let height = needed_height(usage_items).max(1);

    let backend = TestBackend::new(width, height);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(_) => return None,
    };

    if terminal
        .draw(|frame| {
            let area = frame.area();
            let _ = render_with_empty_message(frame, area, usage_items, 0, None, empty_message);
        })
        .is_err()
    {
        return None;
    }

    Some(terminal.backend().buffer().clone())
}

fn render_with_empty_message(
    frame: &mut Frame,
    area: Rect,
    usage_items: &[UsageInfo],
    scroll: usize,
    selected_card: Option<usize>,
    empty_message: &str,
) -> UsageRenderMeta {
    if usage_items.is_empty() {
        let empty = Paragraph::new(empty_message);
        frame.render_widget(empty, area);
        return UsageRenderMeta {
            max_scroll: 0,
            page_step: 1,
        };
    }

    let constraints: Vec<Constraint> = (0..usage_items.len())
        .map(|_| Constraint::Ratio(1, usage_items.len() as u32))
        .collect();
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    let mut selected_max_scroll = 0usize;
    let mut selected_page_step = 1usize;

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

        let border_style = if selected_card == Some(index) {
            Style::default()
                .fg(Color::Rgb(161, 167, 229))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let card_block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title)
            .style(Style::default().bg(Color::Rgb(35, 40, 55)));
        frame.render_widget(card_block.clone(), card_area);
        let card_inner = card_block.inner(card_area);

        let (normal, extra) = split_windows(item);

        if normal.is_empty() && extra.is_empty() {
            frame.render_widget(Paragraph::new("No windows reported"), card_inner);
            continue;
        }

        let mut windows_to_render: Vec<&UsageWindow> =
            Vec::with_capacity(normal.len().saturating_add(extra.len()));
        windows_to_render.extend(normal.iter().copied());
        windows_to_render.extend(extra.iter().copied());

        if card_inner.height < 4 {
            if selected_card == Some(index) {
                selected_max_scroll = windows_to_render.len();
            }
            frame.render_widget(Paragraph::new("Increase terminal height"), card_inner);
            continue;
        }

        let available_rows = card_inner.height as usize;
        let visible_capacity = available_rows / 4;
        if visible_capacity == 0 {
            if selected_card == Some(index) {
                selected_max_scroll = windows_to_render.len();
            }
            frame.render_widget(Paragraph::new("Increase terminal height"), card_inner);
            continue;
        }

        let card_max_scroll = windows_to_render.len().saturating_sub(visible_capacity);
        let card_page_step = (visible_capacity / 2).max(1);
        if selected_card == Some(index) {
            selected_max_scroll = card_max_scroll;
            selected_page_step = card_page_step;
        }

        let effective_scroll = if selected_card == Some(index) {
            scroll
        } else {
            0
        };
        let start = effective_scroll.min(card_max_scroll);
        let mut windows_to_render: Vec<&UsageWindow> = windows_to_render
            .iter()
            .skip(start)
            .take(visible_capacity)
            .copied()
            .collect();

        let above_count = start;
        let mut below_count = normal
            .len()
            .saturating_add(extra.len())
            .saturating_sub(start.saturating_add(windows_to_render.len()));
        let mut show_truncated_note = above_count > 0 || below_count > 0;
        if show_truncated_note {
            let required_rows = windows_to_render.len().saturating_mul(4);
            if required_rows.saturating_add(1) > available_rows && !windows_to_render.is_empty() {
                windows_to_render.pop();
                below_count = below_count.saturating_add(1);
            }
            show_truncated_note = above_count > 0 || below_count > 0;
        }

        // Build row constraints: 4 lines per visible window.
        let mut row_constraints = Vec::new();
        for _ in &windows_to_render {
            row_constraints.push(Constraint::Length(1)); // title
            row_constraints.push(Constraint::Length(1)); // bar + percent
            row_constraints.push(Constraint::Length(1)); // reset text
            row_constraints.push(Constraint::Length(1)); // spacer
        }
        if show_truncated_note {
            row_constraints.push(Constraint::Length(1));
        }
        row_constraints.push(Constraint::Min(0)); // absorb remaining space

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(card_inner);

        let mut row_idx = 0;

        for window in &windows_to_render {
            render_window(frame, &rows, row_idx, &item.agent_name, window);
            row_idx += 4;
        }

        if show_truncated_note {
            let note_text = match (above_count, below_count) {
                (above, below) if above > 0 && below > 0 => {
                    format!("... {above} above, {below} more quota(s)")
                }
                (above, 0) if above > 0 => format!("... {above} above"),
                (0, below) if below > 0 => format!("... {below} more quota(s)"),
                _ => String::new(),
            };
            if !note_text.is_empty() {
                let note = Paragraph::new(Span::styled(
                    note_text,
                    Style::default().fg(Color::DarkGray),
                ));
                frame.render_widget(note, rows[row_idx]);
            }
        }
    }

    UsageRenderMeta {
        max_scroll: if selected_card.is_some() {
            selected_max_scroll
        } else {
            0
        },
        page_step: selected_page_step.max(1),
    }
}

fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
    let area = *buffer.area();
    let mut lines = Vec::with_capacity(area.height as usize);

    for y in 0..area.height {
        let mut line = String::with_capacity(area.width as usize);
        for x in 0..area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    lines
}

fn render_progress_bar(frame: &mut Frame, area: Rect, percent: u16) {
    if area.is_empty() {
        return;
    }

    let fill_bg = Color::Rgb(86, 90, 123);
    let fill_fg = Color::Rgb(161, 167, 229);
    let gauge = Gauge::default()
        .style(Style::default().bg(fill_bg))
        .gauge_style(Style::default().fg(fill_fg).bg(fill_bg))
        .percent(percent)
        .label("");
    frame.render_widget(gauge, area);

    // ratatui::Gauge leaves one center cell for label even when label is empty.
    // Patch that single cell only when 100% to keep bar continuous while preserving Gauge style.
    if percent == 100 {
        let center_x = area.left() + area.width / 2;
        let buffer = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            buffer[(center_x, y)]
                .set_symbol(symbols::block::FULL)
                .set_fg(fill_fg)
                .set_bg(fill_bg);
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
    let base_title = derive_display_name(agent_name, window);
    let title_text = if window.is_extra {
        format!("Extra: {base_title}")
    } else {
        base_title
    };

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

    render_progress_bar(frame, bar_row[0], percent);

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

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use crate::agent::{UsageInfo, UsageWindow};

    use super::{UsageRenderMeta, render, render_static_lines};

    fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
        let area = *buffer.area();
        let mut lines = Vec::with_capacity(area.height as usize);
        for y in 0..area.height {
            let mut line = String::with_capacity(area.width as usize);
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    fn render_lines_with_scroll(
        usage: &[UsageInfo],
        width: u16,
        height: u16,
        scroll: usize,
        selected_card: Option<usize>,
    ) -> (Vec<String>, UsageRenderMeta) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal should be created");
        let mut meta = UsageRenderMeta::default();
        terminal
            .draw(|frame| {
                meta = render(frame, frame.area(), usage, scroll, selected_card);
            })
            .expect("render should succeed");
        (buffer_to_lines(terminal.backend().buffer()), meta)
    }

    #[test]
    fn static_render_includes_agent_title_and_percent() {
        let usage = vec![UsageInfo {
            agent_name: "codex".to_string(),
            display_name: "Codex".to_string(),
            plan: Some("plus".to_string()),
            windows: vec![UsageWindow {
                name: "five-hour".to_string(),
                utilization_pct: 60.0,
                resets_at: Some("2026-02-28T23:00:00Z".to_string()),
                is_extra: false,
                source_limit_name: None,
            }],
            extra_usage: None,
        }];

        let output = render_static_lines(&usage, 100, "No usage data available.").join("\n");
        assert!(output.contains("Codex (plus)"));
        assert!(output.contains("Session"));
        assert!(output.contains("60% used"));
    }

    #[test]
    fn static_render_uses_custom_empty_message() {
        let output = render_static_lines(&[], 80, "No usage data available.").join("\n");
        assert!(output.contains("No usage data available."));
    }

    #[test]
    fn static_render_full_bar_has_no_center_gap() {
        let usage = vec![UsageInfo {
            agent_name: "claude".to_string(),
            display_name: "Claude".to_string(),
            plan: None,
            windows: vec![UsageWindow {
                name: "five-hour".to_string(),
                utilization_pct: 100.0,
                resets_at: Some("2026-02-28T18:00:00Z".to_string()),
                is_extra: false,
                source_limit_name: None,
            }],
            extra_usage: None,
        }];

        let output = render_static_lines(&usage, 100, "No usage data available.").join("\n");
        assert!(!output.contains("█ █"));
    }

    #[test]
    fn static_render_prefixes_extra_quota_with_extra_label() {
        let usage = vec![UsageInfo {
            agent_name: "claude".to_string(),
            display_name: "Claude".to_string(),
            plan: None,
            windows: vec![UsageWindow {
                name: "sonnet-4".to_string(),
                utilization_pct: 10.0,
                resets_at: None,
                is_extra: true,
                source_limit_name: None,
            }],
            extra_usage: None,
        }];

        let output = render_static_lines(&usage, 100, "No usage data available.").join("\n");
        assert!(output.contains("Extra: sonnet-4"));
    }

    #[test]
    fn static_render_skips_window_with_empty_display_name() {
        let usage = vec![UsageInfo {
            agent_name: "gemini".to_string(),
            display_name: "Gemini".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "gemini-2.5-pro".to_string(),
                    utilization_pct: 10.0,
                    resets_at: Some("2026-03-06T00:00:00Z".to_string()),
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "".to_string(),
                    utilization_pct: 20.0,
                    resets_at: Some("2026-03-06T00:00:00Z".to_string()),
                    is_extra: false,
                    source_limit_name: None,
                },
            ],
            extra_usage: None,
        }];

        let output = render_static_lines(&usage, 100, "No usage data available.").join("\n");
        assert!(output.contains("Gemini 2.5 Pro"));
        assert_eq!(output.matches("% used").count(), 1);
    }

    #[test]
    fn render_with_scroll_moves_usage_window_slice() {
        let windows = (1..=5)
            .map(|index| UsageWindow {
                name: format!("quota-{index}"),
                utilization_pct: (index * 10) as f64,
                resets_at: Some("2026-03-06T00:00:00Z".to_string()),
                is_extra: false,
                source_limit_name: None,
            })
            .collect::<Vec<_>>();

        let usage = vec![UsageInfo {
            agent_name: "test-agent".to_string(),
            display_name: "Test".to_string(),
            plan: None,
            windows,
            extra_usage: None,
        }];

        let (lines0, meta0) = render_lines_with_scroll(&usage, 100, 16, 0, Some(0));
        let output0 = lines0.join("\n");
        assert!(output0.contains("quota-1"));
        assert!(output0.contains("quota-2"));
        assert!(output0.contains("quota-3"));
        assert!(!output0.contains("quota-4"));
        assert_eq!(meta0.max_scroll, 2);

        let (lines1, meta1) = render_lines_with_scroll(&usage, 100, 16, 1, Some(0));
        let output1 = lines1.join("\n");
        assert!(!output1.contains("quota-1"));
        assert!(output1.contains("quota-2"));
        assert!(output1.contains("quota-3"));
        assert!(output1.contains("quota-4"));
        assert_eq!(meta1.max_scroll, 2);
    }

    #[test]
    fn unselected_card_does_not_apply_scroll_offset() {
        let windows = (1..=5)
            .map(|index| UsageWindow {
                name: format!("quota-{index}"),
                utilization_pct: (index * 10) as f64,
                resets_at: Some("2026-03-06T00:00:00Z".to_string()),
                is_extra: false,
                source_limit_name: None,
            })
            .collect::<Vec<_>>();
        let usage = vec![UsageInfo {
            agent_name: "test-agent".to_string(),
            display_name: "Test".to_string(),
            plan: None,
            windows,
            extra_usage: None,
        }];

        let (lines0, meta0) = render_lines_with_scroll(&usage, 100, 16, 0, None);
        let (lines1, meta1) = render_lines_with_scroll(&usage, 100, 16, 1, None);
        assert_eq!(lines0, lines1);
        assert_eq!(meta0.max_scroll, 0);
        assert_eq!(meta1.max_scroll, 0);
    }
}
