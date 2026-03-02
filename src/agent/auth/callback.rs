use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use serde::Deserialize;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

use crate::error::{AppError, Result};

#[derive(Debug, Clone)]
pub struct CallbackPayload {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Clone)]
struct CallbackState {
    payload_tx: Arc<Mutex<Option<oneshot::Sender<CallbackPayload>>>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn start_callback_server(listener: TcpListener) -> Result<CallbackPayload> {
    let (payload_tx, payload_rx) = oneshot::channel::<CallbackPayload>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let state = CallbackState {
        payload_tx: Arc::new(Mutex::new(Some(payload_tx))),
        shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
    };

    let app = Router::new()
        .route("/auth/callback", get(callback_handler))
        .with_state(state);

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });

    let mut server_task = tokio::spawn(async move {
        if let Err(err) = server.await {
            tracing::error!("callback server error: {err}");
        }
    });

    let payload = payload_rx.await.map_err(|_| {
        AppError::OAuth("Callback server closed before receiving response".to_string())
    })?;

    // Give the in-flight response a brief window to flush; then force-stop if keepalive
    // connections prevent graceful shutdown from completing.
    match tokio::time::timeout(Duration::from_secs(2), &mut server_task).await {
        Ok(_) => {}
        Err(_) => {
            tracing::debug!("callback server shutdown timed out; aborting task");
            server_task.abort();
            let _ = server_task.await;
        }
    }
    Ok(payload)
}

async fn callback_handler(
    State(state): State<CallbackState>,
    Query(query): Query<CallbackQuery>,
) -> Html<&'static str> {
    let payload = CallbackPayload {
        code: query.code.clone(),
        state: query.state.clone(),
        error: query.error.clone(),
        error_description: query.error_description.clone(),
    };

    if let Some(tx) = state.payload_tx.lock().await.take() {
        let _ = tx.send(payload);
    }

    if let Some(tx) = state.shutdown_tx.lock().await.take() {
        let _ = tx.send(());
    }

    if query.error.is_some() {
        return Html(
            "<html><body><h1>OAuth failed</h1><p>OAuth callback returned an error. Check terminal logs for details.</p></body></html>",
        );
    }

    if query.code.is_none() {
        return Html(
            "<html><body><h1>Missing code</h1><p>OAuth callback did not include a code parameter.</p></body></html>",
        );
    }

    Html("<html><body><h1>VibeMate login complete</h1><p>You can close this tab.</p></body></html>")
}
