use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::provider::ProviderRegistry;
use crate::proxy::RequestLog;
use crate::proxy::handler::{self, ProxyState};
use crate::proxy::middleware::apply_middleware;
use crate::proxy::router::ModelRouter;

pub async fn start(config: &AppConfig, log_tx: broadcast::Sender<RequestLog>) -> Result<()> {
    let http_client = config.server.build_http_client()?;

    let state = Arc::new(ProxyState {
        provider_registry: ProviderRegistry::from_config(config),
        model_router: ModelRouter::from_config(&config.routing),
        http_client,
        log_tx,
    });

    let app = Router::new()
        .route("/api/v1/chat/completions", post(handler::chat_completions))
        .route("/api/v1/responses", post(handler::responses))
        .route("/api/v1/messages", post(handler::messages))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let app = apply_middleware(app);

    let listener = tokio::net::TcpListener::bind((config.server.host.as_str(), config.server.port))
        .await
        .map_err(|e| {
            AppError::ProxyServer(format!(
                "Failed to bind proxy server on {}:{}: {e}",
                config.server.host, config.server.port
            ))
        })?;

    tracing::info!(
        "Proxy listening on http://{}:{}",
        config.server.host,
        config.server.port
    );

    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::ProxyServer(format!("Proxy server exited with error: {e}")))
}
