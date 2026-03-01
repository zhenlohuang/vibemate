use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};

use crossterm::queue;
use crossterm::style::{
    Attribute, Color as CrosstermColor, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal;
use ratatui::style::{Color as TuiColor, Modifier};
use serde::Serialize;
use serde_json::Value;

use crate::agent::auth::token::{auth_file_path, save_token};
use crate::agent::{AgentUsageCapability, UsageInfo, UsageWindow, global_agent_registry};
use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::tui::widgets;

const DEFAULT_WIDGET_WIDTH: u16 = 50;
const MIN_WIDGET_WIDTH: u16 = 30;
const MAX_WIDGET_WIDTH: u16 = 80;

#[derive(Debug, Clone, Default)]
pub struct UsageOptions {
    pub agent: Option<String>,
    pub json: bool,
    pub raw: bool,
}

#[derive(Debug, Serialize)]
struct UsageJsonOutput {
    usage: Vec<UsageJsonAgent>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UsageJsonAgent {
    agent_name: String,
    display_name: String,
    plan: Option<String>,
    quotas: Vec<UsageJsonQuota>,
    extra_quotas: Vec<UsageJsonQuota>,
}

#[derive(Debug, Serialize)]
struct UsageJsonQuota {
    quota_name: String,
    name: String,
    used_percent: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct UsageRawOutput {
    // Raw upstream usage payload keyed by agent name; no schema normalization.
    raw_usage: BTreeMap<String, Value>,
    warnings: Vec<String>,
}

pub async fn run(config: &AppConfig, options: UsageOptions) -> Result<()> {
    let registry = global_agent_registry();
    let target_agent = validate_target_agent(options.agent.as_deref())?;
    let mut usage_results = Vec::new();
    let mut raw_results = BTreeMap::new();
    let mut errors = Vec::new();
    let mut found_any_token = false;
    let client = config.system.build_http_client()?;

    for agent_impl in registry
        .iter()
        .filter(|agent| target_agent.map_or(true, |id| agent.descriptor().id == id))
    {
        let agent_id = agent_impl.descriptor().id;
        let Some(auth) = agent_impl.auth_capability() else {
            errors.push(format!("{agent_id} capability missing: auth"));
            continue;
        };
        let Some(usage_capability) = agent_impl.usage_capability() else {
            errors.push(format!("{agent_id} capability missing: usage"));
            continue;
        };

        match auth.load_saved_token().await {
            Ok(Some(mut token)) => {
                found_any_token = true;
                if let Err(err) = auth.refresh_if_needed(&mut token, &client).await {
                    errors.push(format!("{agent_id} refresh error: {err}"));
                } else {
                    match auth_file_path(agent_impl.descriptor().token_file_name) {
                        Ok(path) => {
                            if let Err(err) = save_token(&path, &token) {
                                errors.push(format!("{agent_id} token save error: {err}"));
                            }
                        }
                        Err(err) => errors.push(format!("{agent_id} token path error: {err}")),
                    }

                    if options.raw {
                        match usage_capability.get_usage_raw(&token, &client).await {
                            Ok(value) => {
                                raw_results.insert(agent_id.to_string(), value);
                            }
                            Err(err) => errors.push(format!("{agent_id} usage error: {err}")),
                        }
                    } else {
                        match usage_capability.get_usage(&token, &client).await {
                            Ok(info) => usage_results.push(info),
                            Err(err) => errors.push(format!("{agent_id} usage error: {err}")),
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(err) => errors.push(format!("{agent_id} token load error: {err}")),
        }
    }

    let has_data = if options.raw {
        !raw_results.is_empty()
    } else {
        !usage_results.is_empty()
    };

    if options.raw {
        let output = UsageRawOutput {
            raw_usage: raw_results,
            warnings: errors.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if options.json {
        let usage_json: Vec<UsageJsonAgent> =
            usage_results.iter().map(to_usage_json_agent).collect();
        let output = UsageJsonOutput {
            usage: usage_json,
            warnings: errors.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if !config.show_extra_quota() {
            filter_extra_windows(&mut usage_results);
        }
        if !usage_results.is_empty() {
            print_usage_widget(&usage_results)?;
        }
        if !errors.is_empty() {
            eprintln!("\nUsage warnings");
            eprintln!("==============");
            for error in &errors {
                eprintln!("- {error}");
            }
        }
    }

    if !found_any_token {
        if options.raw {
            return Ok(());
        }
        if options.json {
            return Ok(());
        }
        let supported_logins = target_agent
            .map(|name| format!("`vibemate login {name}`"))
            .unwrap_or_else(|| {
                registry
                    .supported_ids()
                    .into_iter()
                    .map(|name| format!("`vibemate login {name}`"))
                    .collect::<Vec<_>>()
                    .join(" or ")
            });
        println!("No login tokens found. Run {supported_logins}.");
        return Ok(());
    }

    if !has_data {
        if options.raw || options.json {
            return Ok(());
        }
        return Err(AppError::OAuth(
            "No usage data could be fetched.".to_string(),
        ));
    }

    Ok(())
}

fn validate_target_agent(agent: Option<&str>) -> Result<Option<&str>> {
    let Some(agent) = agent else {
        return Ok(None);
    };

    if global_agent_registry().get(agent).is_some() {
        return Ok(Some(agent));
    }

    let supported = global_agent_registry()
        .supported_ids()
        .into_iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(AppError::OAuth(format!(
        "Unsupported agent '{agent}'. Use {supported}"
    )))
}

fn print_usage_widget(items: &[UsageInfo]) -> Result<()> {
    let width = terminal::size()
        .map(|(width, _)| width.clamp(MIN_WIDGET_WIDTH, MAX_WIDGET_WIDTH))
        .unwrap_or(DEFAULT_WIDGET_WIDTH);
    let mut stdout = io::stdout();
    let styled_output = stdout.is_terminal();

    // For `vibemate usage`, render one card at a time so cards stack vertically.
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            queue!(stdout, Print("\r\n"))?;
        }
        let single_item = std::slice::from_ref(item);
        if styled_output {
            if let Some(buffer) =
                widgets::usage::render_static_buffer(single_item, width, "No usage data available.")
            {
                write_styled_buffer(&mut stdout, &buffer)?;
            }
        } else {
            for line in
                widgets::usage::render_static_lines(single_item, width, "No usage data available.")
            {
                queue!(stdout, Print(line), Print("\r\n"))?;
            }
        }
    }

    if styled_output {
        queue!(
            stdout,
            SetAttribute(Attribute::Reset),
            SetForegroundColor(CrosstermColor::Reset),
            SetBackgroundColor(CrosstermColor::Reset)
        )?;
    }
    stdout.flush()?;
    Ok(())
}

fn filter_extra_windows(items: &mut [UsageInfo]) {
    for item in items {
        item.windows.retain(|window| !window.is_extra);
    }
}

fn to_usage_json_agent(info: &UsageInfo) -> UsageJsonAgent {
    let mut quotas = Vec::new();
    let mut extra_quotas = Vec::new();
    for window in info
        .windows
        .iter()
        .filter(|window| should_display_window(window))
    {
        let quota = UsageJsonQuota {
            quota_name: derive_quota_name(&info.agent_name, window),
            name: derive_display_name(&info.agent_name, window),
            used_percent: window.utilization_pct,
            resets_at: window.resets_at.clone(),
        };
        if window.is_extra {
            extra_quotas.push(quota);
        } else {
            quotas.push(quota);
        }
    }

    UsageJsonAgent {
        agent_name: info.agent_name.clone(),
        display_name: info.display_name.clone(),
        plan: info.plan.clone(),
        quotas,
        extra_quotas,
    }
}

fn derive_quota_name(agent_name: &str, window: &UsageWindow) -> String {
    if let Some(agent) = lookup_usage_capability(agent_name) {
        return agent.quota_name(window);
    }

    window.name.to_string()
}

pub fn derive_display_name(agent_name: &str, window: &UsageWindow) -> String {
    if let Some(agent) = lookup_usage_capability(agent_name) {
        return agent.display_quota_name(window);
    }

    window.name.to_string()
}

fn lookup_usage_capability(agent_name: &str) -> Option<&'static dyn AgentUsageCapability> {
    global_agent_registry()
        .get(agent_name)
        .and_then(|agent| agent.usage_capability())
}

fn should_display_quota(window: &UsageWindow) -> bool {
    if !window.utilization_pct.is_finite() {
        return false;
    }
    match &window.resets_at {
        Some(value) => !value.trim().is_empty(),
        None => false,
    }
}

fn should_display_extra_quota(window: &UsageWindow) -> bool {
    window.utilization_pct.is_finite()
}

pub fn should_display_window(window: &UsageWindow) -> bool {
    if window.is_extra {
        return should_display_extra_quota(window);
    }
    should_display_quota(window)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    fg: TuiColor,
    bg: TuiColor,
    modifier: Modifier,
}

fn write_styled_buffer<W: Write>(out: &mut W, buffer: &ratatui::buffer::Buffer) -> Result<()> {
    let area = *buffer.area();
    let mut active_style: Option<CellStyle> = None;

    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            let style = CellStyle {
                fg: cell.fg,
                bg: cell.bg,
                modifier: cell.modifier,
            };
            if active_style != Some(style) {
                apply_cell_style(out, style)?;
                active_style = Some(style);
            }
            queue!(out, Print(cell.symbol()))?;
        }

        // Reset at line boundaries so terminal state stays predictable.
        queue!(
            out,
            SetAttribute(Attribute::Reset),
            SetForegroundColor(CrosstermColor::Reset),
            SetBackgroundColor(CrosstermColor::Reset),
            Print("\r\n")
        )?;
        active_style = None;
    }

    Ok(())
}

fn apply_cell_style<W: Write>(out: &mut W, style: CellStyle) -> Result<()> {
    queue!(
        out,
        SetAttribute(Attribute::Reset),
        SetForegroundColor(to_crossterm_color(style.fg)),
        SetBackgroundColor(to_crossterm_color(style.bg))
    )?;

    if style.modifier.contains(Modifier::BOLD) {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    if style.modifier.contains(Modifier::DIM) {
        queue!(out, SetAttribute(Attribute::Dim))?;
    }
    if style.modifier.contains(Modifier::ITALIC) {
        queue!(out, SetAttribute(Attribute::Italic))?;
    }
    if style.modifier.contains(Modifier::UNDERLINED) {
        queue!(out, SetAttribute(Attribute::Underlined))?;
    }
    if style.modifier.contains(Modifier::SLOW_BLINK) {
        queue!(out, SetAttribute(Attribute::SlowBlink))?;
    }
    if style.modifier.contains(Modifier::RAPID_BLINK) {
        queue!(out, SetAttribute(Attribute::RapidBlink))?;
    }
    if style.modifier.contains(Modifier::REVERSED) {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    }
    if style.modifier.contains(Modifier::HIDDEN) {
        queue!(out, SetAttribute(Attribute::Hidden))?;
    }
    if style.modifier.contains(Modifier::CROSSED_OUT) {
        queue!(out, SetAttribute(Attribute::CrossedOut))?;
    }

    Ok(())
}

fn to_crossterm_color(color: TuiColor) -> CrosstermColor {
    match color {
        TuiColor::Reset => CrosstermColor::Reset,
        TuiColor::Black => CrosstermColor::Black,
        TuiColor::Red => CrosstermColor::DarkRed,
        TuiColor::Green => CrosstermColor::DarkGreen,
        TuiColor::Yellow => CrosstermColor::DarkYellow,
        TuiColor::Blue => CrosstermColor::DarkBlue,
        TuiColor::Magenta => CrosstermColor::DarkMagenta,
        TuiColor::Cyan => CrosstermColor::DarkCyan,
        TuiColor::Gray => CrosstermColor::Grey,
        TuiColor::DarkGray => CrosstermColor::DarkGrey,
        TuiColor::LightRed => CrosstermColor::Red,
        TuiColor::LightGreen => CrosstermColor::Green,
        TuiColor::LightYellow => CrosstermColor::Yellow,
        TuiColor::LightBlue => CrosstermColor::Blue,
        TuiColor::LightMagenta => CrosstermColor::Magenta,
        TuiColor::LightCyan => CrosstermColor::Cyan,
        TuiColor::White => CrosstermColor::White,
        TuiColor::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
        TuiColor::Indexed(value) => CrosstermColor::AnsiValue(value),
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::{UsageInfo, UsageWindow};

    use super::{
        derive_display_name, derive_quota_name, filter_extra_windows, to_usage_json_agent,
        validate_target_agent,
    };

    #[test]
    fn codex_display_names_are_derived_from_codex_agent_impl() {
        assert_eq!(
            derive_display_name(
                "codex",
                &UsageWindow {
                    name: "five-hour".to_string(),
                    ..Default::default()
                }
            ),
            "Session"
        );
        assert_eq!(
            derive_display_name(
                "codex",
                &UsageWindow {
                    name: "gpt-5-3-codex-spark-five-hour".to_string(),
                    ..Default::default()
                }
            ),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(
            derive_display_name(
                "codex",
                &UsageWindow {
                    name: "code-review-seven-day".to_string(),
                    ..Default::default()
                }
            ),
            "Code Review"
        );
    }

    #[test]
    fn claude_display_names_are_derived_from_claude_agent_impl() {
        assert_eq!(
            derive_display_name(
                "claude-code",
                &UsageWindow {
                    name: "seven-day".to_string(),
                    ..Default::default()
                }
            ),
            "Weekly"
        );
        assert_eq!(
            derive_display_name(
                "claude-code",
                &UsageWindow {
                    name: "sonnet-4".to_string(),
                    ..Default::default()
                }
            ),
            "sonnet-4"
        );
    }

    #[test]
    fn codex_additional_rate_limits_use_stable_quota_name() {
        let codex_extra_week = UsageWindow {
            name: "gpt-5-3-codex-spark-seven-day".to_string(),
            is_extra: true,
            source_limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
            ..Default::default()
        };
        let codex_extra_session = UsageWindow {
            name: "gpt-5-3-codex-spark-five-hour".to_string(),
            is_extra: true,
            source_limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
            ..Default::default()
        };
        let codex_base = UsageWindow {
            name: "five-hour".to_string(),
            is_extra: false,
            ..Default::default()
        };

        assert_eq!(
            derive_quota_name("codex", &codex_extra_week),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(
            derive_quota_name("codex", &codex_extra_session),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(derive_quota_name("codex", &codex_base), "five-hour");
        assert_eq!(
            derive_display_name("codex", &codex_extra_session),
            "GPT-5.3-Codex-Spark"
        );
        assert_eq!(
            derive_display_name("codex", &codex_extra_week),
            "GPT-5.3-Codex-Spark Weekly"
        );
    }

    #[test]
    fn only_codex_extra_uses_stable_quota_name() {
        let claude_extra = UsageWindow {
            name: "sonnet-4".to_string(),
            is_extra: true,
            ..Default::default()
        };
        assert_eq!(derive_quota_name("claude-code", &claude_extra), "sonnet-4");
    }

    #[test]
    fn usage_json_splits_extra_quotas_for_claude() {
        let info = UsageInfo {
            agent_name: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "five-hour".to_string(),
                    utilization_pct: 6.0,
                    resets_at: Some("2026-02-27T20:00:00Z".to_string()),
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "extra-usage".to_string(),
                    utilization_pct: 0.0,
                    resets_at: Some("2026-03-01T00:00:00Z".to_string()),
                    is_extra: true,
                    source_limit_name: None,
                },
            ],
            extra_usage: None,
        };

        let json_agent = to_usage_json_agent(&info);
        assert_eq!(json_agent.quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas[0].quota_name, "extra-usage");
        assert_eq!(json_agent.extra_quotas[0].name, "Extra Usage");
    }

    #[test]
    fn usage_json_keeps_extra_quota_without_reset_at() {
        let info = UsageInfo {
            agent_name: "claude-code".to_string(),
            display_name: "Claude Code".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "five-hour".to_string(),
                    utilization_pct: 6.0,
                    resets_at: None,
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "extra-usage".to_string(),
                    utilization_pct: 0.0,
                    resets_at: None,
                    is_extra: true,
                    source_limit_name: None,
                },
            ],
            extra_usage: None,
        };

        let json_agent = to_usage_json_agent(&info);
        assert!(json_agent.quotas.is_empty());
        assert_eq!(json_agent.extra_quotas.len(), 1);
        assert_eq!(json_agent.extra_quotas[0].quota_name, "extra-usage");
        assert!(json_agent.extra_quotas[0].resets_at.is_none());
    }

    #[test]
    fn filter_extra_windows_removes_only_extra_quotas() {
        let mut usage = vec![UsageInfo {
            agent_name: "codex".to_string(),
            display_name: "Codex".to_string(),
            plan: None,
            windows: vec![
                UsageWindow {
                    name: "five-hour".to_string(),
                    utilization_pct: 10.0,
                    resets_at: Some("2026-02-28T18:00:00Z".to_string()),
                    is_extra: false,
                    source_limit_name: None,
                },
                UsageWindow {
                    name: "gpt-5-3-codex-spark-seven-day".to_string(),
                    utilization_pct: 30.0,
                    resets_at: Some("2026-03-05T18:00:00Z".to_string()),
                    is_extra: true,
                    source_limit_name: Some("GPT-5.3-Codex-Spark".to_string()),
                },
            ],
            extra_usage: None,
        }];

        filter_extra_windows(&mut usage);
        assert_eq!(usage[0].windows.len(), 1);
        assert_eq!(usage[0].windows[0].name, "five-hour");
        assert!(!usage[0].windows[0].is_extra);
    }

    #[test]
    fn validates_supported_usage_target_agent() {
        assert_eq!(validate_target_agent(None).unwrap(), None);
        assert_eq!(validate_target_agent(Some("codex")).unwrap(), Some("codex"));
    }

    #[test]
    fn rejects_unknown_usage_target_agent() {
        let err = validate_target_agent(Some("unknown-agent")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "OAuth error: Unsupported agent 'unknown-agent'. Use 'codex', 'claude-code', 'cursor'"
        );
    }
}
