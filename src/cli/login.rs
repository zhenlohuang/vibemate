use crate::agent::global_agent_registry;
use crate::config::AppConfig;
use crate::error::{AppError, Result};

pub async fn run(agent: &str, _config: &AppConfig) -> Result<()> {
    let registry = global_agent_registry();
    let agent_impl = registry.get(agent).ok_or_else(|| {
        let supported = registry
            .supported_ids()
            .into_iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        AppError::OAuth(format!("Unsupported agent '{agent}'. Use {supported}"))
    })?;
    let auth = agent_impl
        .auth_capability()
        .ok_or_else(|| AppError::UnsupportedCapability {
            agent: agent.to_string(),
            capability: "auth".to_string(),
        })?;

    println!(
        "Starting {} OAuth flow...",
        agent_impl.descriptor().display_name
    );
    auth.login().await?;
    println!("{} login successful.", agent_impl.descriptor().display_name);

    Ok(())
}
