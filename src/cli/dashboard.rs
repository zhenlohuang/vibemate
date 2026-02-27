use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::{broadcast, mpsc};

use crate::config::AppConfig;
use crate::error::Result;
use crate::oauth::token::{auth_file_path, save_token};
use crate::oauth::{claude, codex, UsageInfo};
use crate::proxy;
use crate::tui::app::App;
use crate::tui::ui;

struct UsageUpdate {
    usage: Vec<UsageInfo>,
    message: Option<String>,
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
    let (log_tx, mut log_rx) = broadcast::channel(1024);
    let (usage_tx, mut usage_rx) = mpsc::channel::<UsageUpdate>(8);

    let proxy_config = config.clone();
    let proxy_task = tokio::spawn(async move { proxy::server::start(&proxy_config, log_tx).await });

    let usage_task_tx = usage_tx.clone();
    let show_extra_quota = config.server.show_extra_quota;
    let usage_task = tokio::spawn(async move {
        loop {
            let update = collect_usage(show_extra_quota).await;
            if usage_task_tx.send(update).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let mut app = App::new(format!(
        "http://{}:{}",
        config.server.host, config.server.port
    ));
    app.proxy_running = true;

    loop {
        while let Ok(log) = log_rx.try_recv() {
            app.push_log(log);
        }

        while let Ok(update) = usage_rx.try_recv() {
            app.usage = update.usage;
            app.status_message = update.message;
        }

        if proxy_task.is_finished() {
            app.proxy_running = false;
            app.status_message = Some("Proxy task stopped unexpectedly".to_string());
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
                        let update = collect_usage(config.server.show_extra_quota).await;
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

    proxy_task.abort();
    usage_task.abort();

    Ok(())
}

async fn collect_usage(show_extra_quota: bool) -> UsageUpdate {
    let mut usage = Vec::new();
    let mut errors = Vec::new();

    match codex::load_saved_token().await {
        Ok(Some(mut token)) => {
            if let Err(err) = codex::refresh_if_needed(&mut token).await {
                errors.push(format!("codex refresh error: {err}"));
            } else {
                let path = match auth_file_path("codex_auth.json") {
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
                    errors.push(format!("codex token save error: {err}"));
                }
                match codex::get_usage(&token).await {
                    Ok(info) => usage.push(info),
                    Err(err) => errors.push(format!("codex usage error: {err}")),
                }
            }
        }
        Ok(None) => {}
        Err(err) => errors.push(format!("codex token load error: {err}")),
    }

    match claude::load_saved_token().await {
        Ok(Some(mut token)) => {
            if let Err(err) = claude::refresh_if_needed(&mut token).await {
                errors.push(format!("claude-code refresh error: {err}"));
            } else {
                let path = match auth_file_path("claude_auth.json") {
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
                    errors.push(format!("claude-code token save error: {err}"));
                }
                match claude::get_usage(&token).await {
                    Ok(info) => usage.push(info),
                    Err(err) => errors.push(format!("claude-code usage error: {err}")),
                }
            }
        }
        Ok(None) => {}
        Err(err) => errors.push(format!("claude-code token load error: {err}")),
    }

    let message = if errors.is_empty() {
        Some("q/esc:quit  r:refresh  Tab:switch page  j/k:scroll".to_string())
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
