use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{broadcast, mpsc};

use crate::agent::auth::token::{auth_file_path, save_token};
use crate::agent::{UsageInfo, global_agent_registry};
use crate::config::AppConfig;
use crate::error::Result;
use crate::model_router;
use crate::model_router::RequestLog;
use crate::model_router::logging::{FileLogTailer, resolve_log_path};
use crate::tui::app::App;
use crate::tui::ui;

struct UsageUpdate {
    usage: Vec<UsageInfo>,
    message: Option<String>,
}

enum LogEvent {
    Entry(RequestLog),
    Note(Option<String>),
}

#[derive(Debug, Clone)]
enum DashboardLogMode {
    Memory,
    File { path: PathBuf, max_files: u32 },
}

pub async fn run(config: &AppConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_dashboard_loop(config, &mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_result
}

async fn run_dashboard_loop(
    config: &AppConfig,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<()> {
    let log_mode = dashboard_log_mode(config);
    let (log_event_tx, mut log_event_rx) = mpsc::channel::<LogEvent>(1024);
    let (usage_tx, mut usage_rx) = mpsc::channel::<UsageUpdate>(8);

    let mut log_task = match &log_mode {
        DashboardLogMode::Memory => {
            let (memory_log_tx, memory_log_rx) = broadcast::channel(1024);
            let tx = log_event_tx.clone();
            let task = tokio::spawn(async move { bridge_memory_logs(memory_log_rx, tx).await });
            (Some(memory_log_tx), task)
        }
        DashboardLogMode::File { path, max_files } => {
            let tx = log_event_tx.clone();
            let path = path.clone();
            let max_files = *max_files;
            let task = tokio::spawn(async move { tail_file_logs(path, max_files, tx).await });
            (None, task)
        }
    };

    let router_config = config.clone();
    let memory_log_tx = log_task.0.take();
    let mut router_task = Some(tokio::spawn(async move {
        model_router::server::start(&router_config, memory_log_tx).await
    }));

    let usage_task_tx = usage_tx.clone();
    let usage_config = config.clone();
    let show_extra_quota = config.show_extra_quota();
    let usage_refresh_interval = config.usage_refresh_interval();
    let usage_task = tokio::spawn(async move {
        loop {
            let update = collect_usage(&usage_config, show_extra_quota).await;
            if usage_task_tx.send(update).await.is_err() {
                break;
            }
            tokio::time::sleep(usage_refresh_interval).await;
        }
    });

    let mut app = App::new(format!(
        "http://{}:{}",
        config.router.host, config.router.port
    ));
    app.router_running = true;
    app.log_source = match &log_mode {
        DashboardLogMode::Memory => "memory".to_string(),
        DashboardLogMode::File { path, .. } => format!("file {}", path.display()),
    };
    app.log_source_note = match &log_mode {
        DashboardLogMode::Memory => Some("Realtime logs from embedded router process".to_string()),
        DashboardLogMode::File { path, .. } => {
            Some(format!("Reading router logs from {}", path.display()))
        }
    };

    loop {
        while let Ok(event) = log_event_rx.try_recv() {
            match event {
                LogEvent::Entry(log) => app.push_log(log),
                LogEvent::Note(note) => app.log_source_note = note,
            }
        }

        while let Ok(update) = usage_rx.try_recv() {
            app.usage = update.usage;
            app.status_message = update.message;
        }

        if let Some(task) = router_task.as_mut() {
            if task.is_finished() {
                app.router_running = false;
                let result = task.await;
                router_task = None;

                let detail = match result {
                    Ok(Ok(())) => "router task stopped".to_string(),
                    Ok(Err(err)) => format!("router task error: {err}"),
                    Err(err) => format!("router task join error: {err}"),
                };

                match &log_mode {
                    DashboardLogMode::Memory => {
                        app.log_source_note = Some(format!(
                            "{detail}; no available log source (memory mode requires embedded router)"
                        ));
                    }
                    DashboardLogMode::File { .. } => {
                        app.log_source_note = Some(format!(
                            "{detail}; continuing in file-tail mode for external router logs"
                        ));
                    }
                }
            }
        }

        terminal.draw(|frame| ui::render(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let ctrl = key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if ctrl => break,
                    KeyCode::Char('r') => {
                        let update = collect_usage(config, config.show_extra_quota()).await;
                        app.usage = update.usage;
                        app.status_message = update.message;
                    }
                    KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
                    KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
                    KeyCode::Tab => app.next_tab(),
                    _ => {}
                }
            }
        }
    }

    if let Some(task) = router_task.as_mut() {
        task.abort();
    }
    usage_task.abort();
    log_task.1.abort();

    Ok(())
}

fn dashboard_log_mode(config: &AppConfig) -> DashboardLogMode {
    if config.router.logging.enabled {
        DashboardLogMode::File {
            path: resolve_log_path(&config.router.logging.file_path),
            max_files: config.router.logging.max_files_or_default(),
        }
    } else {
        DashboardLogMode::Memory
    }
}

async fn bridge_memory_logs(
    mut memory_log_rx: broadcast::Receiver<RequestLog>,
    log_event_tx: mpsc::Sender<LogEvent>,
) {
    loop {
        match memory_log_rx.recv().await {
            Ok(log) => {
                if log_event_tx.send(LogEvent::Entry(log)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                let _ = log_event_tx
                    .send(LogEvent::Note(Some(format!(
                        "Skipped {skipped} in-memory logs due channel backpressure"
                    ))))
                    .await;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn tail_file_logs(path: PathBuf, max_files: u32, log_event_tx: mpsc::Sender<LogEvent>) {
    let mut tailer = FileLogTailer::new(path.clone(), max_files);
    let history = match tailer.load_recent(1_000) {
        Ok(items) => items,
        Err(err) => {
            let _ = log_event_tx
                .send(LogEvent::Note(Some(format!(
                    "Failed to load log history from {}: {err}",
                    path.display()
                ))))
                .await;
            Vec::new()
        }
    };

    for log in history {
        if log_event_tx.send(LogEvent::Entry(log)).await.is_err() {
            return;
        }
    }

    let mut last_note: Option<String> = None;
    loop {
        match tailer.poll() {
            Ok(poll) => {
                for log in poll.logs {
                    if log_event_tx.send(LogEvent::Entry(log)).await.is_err() {
                        return;
                    }
                }

                let next_note = if poll.waiting_for_file {
                    Some(format!("Waiting for router log file at {}", path.display()))
                } else if tailer.total_parse_errors() > 0 {
                    Some(format!(
                        "Tailing {} (skipped {} malformed lines)",
                        path.display(),
                        tailer.total_parse_errors()
                    ))
                } else {
                    None
                };

                if next_note != last_note {
                    if log_event_tx
                        .send(LogEvent::Note(next_note.clone()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    last_note = next_note;
                }
            }
            Err(err) => {
                let next_note = Some(format!("Failed reading {}: {err}", path.display()));
                if next_note != last_note {
                    if log_event_tx
                        .send(LogEvent::Note(next_note.clone()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    last_note = next_note;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{DashboardLogMode, dashboard_log_mode};
    use crate::config::AppConfig;

    #[test]
    fn dashboard_log_mode_uses_memory_when_disabled() {
        let mut config = AppConfig::default();
        config.router.logging.enabled = false;
        let mode = dashboard_log_mode(&config);
        assert!(matches!(mode, DashboardLogMode::Memory));
    }

    #[test]
    fn dashboard_log_mode_uses_file_when_enabled() {
        let mut config = AppConfig::default();
        config.router.logging.enabled = true;
        config.router.logging.file_path = "~/.vibemate/logs/custom.log".to_string();
        let mode = dashboard_log_mode(&config);
        match mode {
            DashboardLogMode::File { path, max_files } => {
                assert!(
                    path.display()
                        .to_string()
                        .contains(".vibemate/logs/custom.log")
                );
                assert_eq!(max_files, 3);
            }
            DashboardLogMode::Memory => panic!("expected file mode"),
        }
    }
}

async fn collect_usage(config: &AppConfig, show_extra_quota: bool) -> UsageUpdate {
    let registry = global_agent_registry();
    let mut usage = Vec::new();
    let mut errors = Vec::new();
    let client = match config.system.build_http_client() {
        Ok(client) => client,
        Err(err) => {
            return UsageUpdate {
                usage,
                message: Some(format!("http client build error: {err}")),
            };
        }
    };

    for agent_impl in registry.iter() {
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
                if let Err(err) = auth.refresh_if_needed(&mut token, &client).await {
                    errors.push(format!("{agent_id} refresh error: {err}"));
                } else {
                    let path = match auth_file_path(agent_impl.descriptor().token_file_name) {
                        Ok(path) => path,
                        Err(err) => {
                            errors.push(format!("token directory error: {err}"));
                            return UsageUpdate {
                                usage,
                                message: Some(errors.join(" | ")),
                            };
                        }
                    };
                    if let Err(err) = save_token(&path, &token) {
                        errors.push(format!("{agent_id} token save error: {err}"));
                    }
                    match usage_capability.get_usage(&token, &client).await {
                        Ok(info) => usage.push(info),
                        Err(err) => errors.push(format!("{agent_id} usage error: {err}")),
                    }
                }
            }
            Ok(None) => {}
            Err(err) => errors.push(format!("{agent_id} token load error: {err}")),
        }
    }

    let message = if errors.is_empty() {
        None
    } else {
        Some(errors.join(" | "))
    };

    if !show_extra_quota {
        for info in &mut usage {
            info.windows.retain(|window| !window.is_extra);
        }
    }

    UsageUpdate { usage, message }
}
