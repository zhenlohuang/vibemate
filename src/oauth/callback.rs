use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tokio::sync::{oneshot, Mutex};

use crate::error::{AppError, Result};

#[derive(Clone)]
struct CallbackState {
    code_tx: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
}

pub async fn start_callback_server(port: u16) -> Result<String> {
    let (code_tx, code_rx) = oneshot::channel::<String>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let state = CallbackState {
        code_tx: Arc::new(Mutex::new(Some(code_tx))),
        shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
    };

    let app = Router::new()
        .route("/auth/callback", get(callback_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| AppError::OAuth(format!("Failed to bind callback server: {e}")))?;

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });

    let server_task = tokio::spawn(async move {
        if let Err(err) = server.await {
            tracing::error!("callback server error: {err}");
        }
    });

    let code = code_rx
        .await
        .map_err(|_| AppError::OAuth("Callback server closed before receiving code".to_string()))?;

    let _ = server_task.await;
    Ok(code)
}

async fn callback_handler(
    State(state): State<CallbackState>,
    Query(query): Query<CallbackQuery>,
) -> Html<&'static str> {
    let Some(code) = query.code else {
        return Html(
            "<html><body><h1>Missing code</h1><p>OAuth callback did not include a code parameter.</p></body></html>",
        );
    };

    if let Some(tx) = state.code_tx.lock().await.take() {
        let _ = tx.send(code);
    }

    if let Some(tx) = state.shutdown_tx.lock().await.take() {
        let _ = tx.send(());
    }

    Html(
        "<html><body><h1>Vibemate login complete</h1><p>You can close this tab.</p></body></html>",
    )
}
