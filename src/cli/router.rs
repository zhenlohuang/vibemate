use crate::config::AppConfig;
use crate::error::Result;
use crate::model_router;

pub async fn run(config: &AppConfig) -> Result<()> {
    println!(
        "Model router listening on http://{}:{}",
        config.router.host, config.router.port
    );

    model_router::server::start(config, None).await
}
