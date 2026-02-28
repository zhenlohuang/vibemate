use tokio::sync::broadcast;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model_router;

pub async fn run(config: &AppConfig) -> Result<()> {
    let (log_tx, _) = broadcast::channel(1024);

    println!(
        "Model router listening on http://{}:{}",
        config.router.host, config.router.port
    );

    model_router::server::start(config, log_tx).await
}
