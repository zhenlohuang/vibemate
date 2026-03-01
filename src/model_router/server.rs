use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model_router::handler::{self, RouterState};
use crate::model_router::logging::{RouterLogSink, build_file_writer};
use crate::model_router::middleware::apply_middleware;
use crate::model_router::router::ModelRouter;
use crate::provider::ProviderRegistry;

pub async fn start(
    config: &AppConfig,
    memory_log_tx: Option<broadcast::Sender<crate::model_router::RequestLog>>,
) -> Result<()> {
    let http_client = config.system.build_http_client()?;
    let file_writer = build_file_writer(&config.router.logging)?;

    let state = Arc::new(RouterState {
        provider_registry: ProviderRegistry::from_config(config),
        model_router: ModelRouter::from_config(&config.router),
        http_client,
        log_sink: RouterLogSink::new(memory_log_tx, file_writer),
    });

    let app = Router::new()
        .route("/api/v1/chat/completions", post(handler::chat_completions))
        .route("/api/v1/responses", post(handler::responses))
        .route("/api/v1/messages", post(handler::messages))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let app = apply_middleware(app);

    let listener = tokio::net::TcpListener::bind((config.router.host.as_str(), config.router.port))
        .await
        .map_err(|e| {
            AppError::RouterServer(format!(
                "Failed to bind model router server on {}:{}: {e}",
                config.router.host, config.router.port
            ))
        })?;

    tracing::info!(
        "Model router listening on http://{}:{}",
        config.router.host,
        config.router.port
    );

    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::RouterServer(format!("Model router server exited with error: {e}")))
}
