use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::model_router::RequestLog;
use crate::model_router::handler::{self, RouterState};
use crate::model_router::middleware::apply_middleware;
use crate::model_router::router::ModelRouter;
use crate::provider::ProviderRegistry;

pub async fn start(config: &AppConfig, log_tx: broadcast::Sender<RequestLog>) -> Result<()> {
    let mut client_builder = reqwest::Client::builder();

    if let Some(proxy) = &config.system.proxy {
        let proxy_config = reqwest::Proxy::all(proxy)
            .map_err(|e| AppError::Config(format!("Invalid network proxy URL '{proxy}': {e}")))?;
        client_builder = client_builder.proxy(proxy_config);
    }

    let http_client = client_builder
        .build()
        .map_err(|e| AppError::RouterServer(format!("Failed to build HTTP client: {e}")))?;

    let state = Arc::new(RouterState {
        provider_registry: ProviderRegistry::from_config(config),
        model_router: ModelRouter::from_config(&config.router),
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
