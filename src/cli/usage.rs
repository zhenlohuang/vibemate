use crate::config::AppConfig;
use crate::error::Result;
use crate::oauth::token::{save_token, vibemate_dir};
use crate::oauth::{claude, codex, UsageInfo};

pub async fn run(_config: &AppConfig) -> Result<()> {
    let mut results = Vec::new();

    if let Some(mut token) = codex::load_saved_token().await? {
        codex::refresh_if_needed(&mut token).await?;
        let path = vibemate_dir()?.join("codex_auth.json");
        save_token(&path, &token)?;
        results.push(codex::get_usage(&token).await?);
    }

    if let Some(mut token) = claude::load_saved_token().await? {
        claude::refresh_if_needed(&mut token).await?;
        let path = vibemate_dir()?.join("claude_auth.json");
        save_token(&path, &token)?;
        results.push(claude::get_usage(&token).await?);
    }

    if results.is_empty() {
        println!(
            "No login tokens found. Run `vibemate login codex` or `vibemate login claude-code`."
        );
        return Ok(());
    }

    print_usage_table(&results);
    Ok(())
}

fn print_usage_table(items: &[UsageInfo]) {
    println!("\nUsage Summary");
    println!("=============");

    for item in items {
        let plan = item.plan.clone().unwrap_or_else(|| "unknown".to_string());
        println!("\nAgent: {} (plan: {})", item.agent_name, plan);
        for window in &item.windows {
            let reset = window
                .resets_at
                .clone()
                .unwrap_or_else(|| "n/a".to_string());
            println!(
                "  - {:14} {:>6.2}%   resets_at={} ",
                window.name, window.utilization_pct, reset
            );
        }
    }
}
