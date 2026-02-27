use std::sync::Arc;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::error::AppError;
use crate::provider::ProviderRegistry;
use crate::proxy::router::ModelRouter;
use crate::proxy::stream::relay_sse_stream;
use crate::proxy::RequestLog;

#[derive(Clone)]
pub struct ProxyState {
    pub provider_registry: ProviderRegistry,
    pub model_router: ModelRouter,
    pub http_client: reqwest::Client,
    pub log_tx: broadcast::Sender<RequestLog>,
}

pub async fn chat_completions(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_request(
        state,
        "/v1/chat/completions",
        "/api/v1/chat/completions",
        headers,
        body,
    )
    .await
}

pub async fn responses(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_request(state, "/v1/responses", "/api/v1/responses", headers, body).await
}

pub async fn messages(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_request(state, "/v1/messages", "/api/v1/messages", headers, body).await
}

async fn forward_request(
    state: Arc<ProxyState>,
    upstream_path: &str,
    request_path: &str,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();

    let mut body_json = match serde_json::from_slice::<Value>(&body) {
        Ok(v) => v,
        Err(err) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON body: {err}"),
            );
        }
    };

    let original_model = match body_json.get("model").and_then(Value::as_str) {
        Some(model) => model.to_string(),
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                AppError::MissingModel.to_string(),
            );
        }
    };

    let resolved = state.model_router.resolve(&original_model);
    body_json["model"] = Value::String(resolved.model.clone());
    let stream_requested = body_json
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let provider = match state.provider_registry.get(&resolved.provider) {
        Some(provider) => provider,
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                AppError::ProviderNotFound(resolved.provider).to_string(),
            );
        }
    };

    let mut upstream_url = provider.base_url.trim_end_matches('/').to_string();
    upstream_url.push_str(upstream_path);

    let payload = match serde_json::to_vec(&body_json) {
        Ok(p) => p,
        Err(err) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("Failed to serialize upstream body: {err}"),
            );
        }
    };

    let mut request = state
        .http_client
        .post(upstream_url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(payload);

    if let Some(accept) = headers.get(header::ACCEPT).cloned() {
        request = request.header(header::ACCEPT, accept);
    }

    for (key, value) in &provider.headers {
        if let (Ok(name), Ok(val)) = (
            HeaderName::try_from(key.as_str()),
            HeaderValue::try_from(value.as_str()),
        ) {
            request = request.header(name, val);
        }
    }

    let upstream_response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            let latency = started.elapsed().as_millis() as u64;
            emit_log(
                &state.log_tx,
                request_path,
                &original_model,
                &provider.name,
                StatusCode::BAD_GATEWAY.as_u16(),
                latency,
            );
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach upstream provider: {err}"),
            );
        }
    };

    let status = upstream_response.status();
    let latency = started.elapsed().as_millis() as u64;
    emit_log(
        &state.log_tx,
        request_path,
        &resolved.model,
        &provider.name,
        status.as_u16(),
        latency,
    );

    if !status.is_success() {
        return error_response(
            StatusCode::BAD_GATEWAY,
            AppError::Upstream {
                status: status.as_u16(),
                provider: provider.name.clone(),
            }
            .to_string(),
        );
    }

    if stream_requested && status.is_success() {
        return relay_sse_stream(upstream_response);
    }

    let content_type = upstream_response.headers().get(header::CONTENT_TYPE).cloned();
    let body_bytes = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("Failed to read upstream response body: {err}"),
            );
        }
    };

    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }

    response
        .body(Body::from(body_bytes))
        .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response"))
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    let payload = json!({ "error": message.into() });
    (status, axum::Json(payload)).into_response()
}

fn emit_log(
    log_tx: &broadcast::Sender<RequestLog>,
    path: &str,
    model: &str,
    provider: &str,
    status: u16,
    latency_ms: u64,
) {
    let _ = log_tx.send(RequestLog {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        method: "POST".to_string(),
        path: path.to_string(),
        model: model.to_string(),
        provider: provider.to_string(),
        status,
        latency_ms,
    });
}
