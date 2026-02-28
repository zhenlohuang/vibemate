use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::oauth::global_agent_registry;

pub async fn run(agent: &str, _config: &AppConfig) -> Result<()> {
    let registry = global_agent_registry();
    let oauth_agent = registry.get(agent).ok_or_else(|| {
        let supported = registry
            .supported_ids()
            .into_iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        AppError::OAuth(format!("Unsupported agent '{agent}'. Use {supported}"))
    })?;

    println!(
        "Starting {} OAuth flow...",
        oauth_agent.descriptor().display_name
    );
    oauth_agent.login().await?;
    println!(
        "{} login successful.",
        oauth_agent.descriptor().display_name
    );

    Ok(())
}
