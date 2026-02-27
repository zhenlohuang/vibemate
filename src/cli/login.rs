use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::oauth::{claude, codex};

pub async fn run(agent: &str, _config: &AppConfig) -> Result<()> {
    match agent {
        "codex" => {
            println!("Starting Codex OAuth flow...");
            codex::login().await?;
            println!("Codex login successful.");
        }
        "claude-code" => {
            println!("Starting Claude Code OAuth flow...");
            claude::login().await?;
            println!("Claude Code login successful.");
        }
        other => {
            return Err(AppError::OAuth(format!(
                "Unsupported agent '{other}'. Use 'codex' or 'claude-code'"
            )));
        }
    }

    Ok(())
}
