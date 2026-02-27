use tokio::sync::broadcast;

use crate::config::AppConfig;
use crate::error::Result;
use crate::proxy;

pub async fn run(config: &AppConfig) -> Result<()> {
    let (log_tx, _) = broadcast::channel(1024);

    println!(
        "Proxy listening on http://{}:{}",
        config.server.host, config.server.port
    );

    proxy::server::start(config, log_tx).await
}
