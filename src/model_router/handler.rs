use std::sync::Arc;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::error::AppError;
use crate::model_router::RequestLog;
use crate::model_router::logging::RouterLogSink;
use crate::model_router::router::ModelRouter;
use crate::model_router::stream::relay_sse_stream;
use crate::provider::ProviderRegistry;

#[derive(Clone)]
pub struct RouterState {
    pub provider_registry: ProviderRegistry,
    pub model_router: ModelRouter,
    pub http_client: reqwest::Client,
    pub log_sink: RouterLogSink,
}

pub async fn chat_completions(
    State(state): State<Arc<RouterState>>,
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
    State(state): State<Arc<RouterState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_request(state, "/v1/responses", "/api/v1/responses", headers, body).await
}

pub async fn messages(
    State(state): State<Arc<RouterState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_request(state, "/v1/messages", "/api/v1/messages", headers, body).await
}

async fn forward_request(
    state: Arc<RouterState>,
    upstream_path: &str,
    request_path: &str,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let mut original_model = String::new();
    let mut routed_model = String::new();
    let mut provider_name = "unknown".to_string();

    let mut body_json = match serde_json::from_slice::<Value>(&body) {
        Ok(v) => v,
        Err(err) => {
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                &original_model,
                &routed_model,
                &provider_name,
                false,
                StatusCode::BAD_REQUEST.as_u16(),
                started.elapsed().as_millis() as u64,
                "Invalid JSON body",
            );
            return error_response(StatusCode::BAD_REQUEST, format!("Invalid JSON body: {err}"));
        }
    };

    original_model = match body_json.get("model").and_then(Value::as_str) {
        Some(model) => model.to_string(),
        None => {
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                "",
                "",
                &provider_name,
                false,
                StatusCode::BAD_REQUEST.as_u16(),
                started.elapsed().as_millis() as u64,
                &AppError::MissingModel.to_string(),
            );
            return error_response(StatusCode::BAD_REQUEST, AppError::MissingModel.to_string());
        }
    };

    let resolved = state.model_router.resolve(&original_model);
    routed_model = resolved.model.clone();
    provider_name = resolved.provider.clone();
    body_json["model"] = Value::String(routed_model.clone());
    let stream_requested = body_json
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let provider = match state.provider_registry.get(&resolved.provider) {
        Some(provider) => provider,
        None => {
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                &original_model,
                &routed_model,
                &provider_name,
                stream_requested,
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                started.elapsed().as_millis() as u64,
                &AppError::ProviderNotFound(resolved.provider.clone()).to_string(),
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                AppError::ProviderNotFound(resolved.provider).to_string(),
            );
        }
    };

    let upstream_url = build_upstream_url(&provider.base_url, upstream_path);

    let payload = match serde_json::to_vec(&body_json) {
        Ok(p) => p,
        Err(err) => {
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                &original_model,
                &routed_model,
                &provider.name,
                stream_requested,
                StatusCode::BAD_REQUEST.as_u16(),
                started.elapsed().as_millis() as u64,
                "Failed to serialize upstream body",
            );
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
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                &original_model,
                &routed_model,
                &provider.name,
                stream_requested,
                StatusCode::BAD_GATEWAY.as_u16(),
                started.elapsed().as_millis() as u64,
                "Failed to reach upstream provider",
            );
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach upstream provider: {err}"),
            );
        }
    };

    let status = upstream_response.status();

    if !status.is_success() {
        emit_log(
            &state.log_sink,
            request_path,
            &request_id,
            &original_model,
            &routed_model,
            &provider.name,
            stream_requested,
            status.as_u16(),
            started.elapsed().as_millis() as u64,
            &format!("Upstream returned HTTP {}", status.as_u16()),
        );
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
        emit_log(
            &state.log_sink,
            request_path,
            &request_id,
            &original_model,
            &routed_model,
            &provider.name,
            true,
            status.as_u16(),
            started.elapsed().as_millis() as u64,
            "",
        );
        return relay_sse_stream(upstream_response);
    }

    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let body_bytes = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            emit_log(
                &state.log_sink,
                request_path,
                &request_id,
                &original_model,
                &routed_model,
                &provider.name,
                false,
                StatusCode::BAD_GATEWAY.as_u16(),
                started.elapsed().as_millis() as u64,
                "Failed to read upstream response body",
            );
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

    let response = response.body(Body::from(body_bytes)).unwrap_or_else(|_| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build response",
        )
    });

    emit_log(
        &state.log_sink,
        request_path,
        &request_id,
        &original_model,
        &routed_model,
        &provider.name,
        false,
        status.as_u16(),
        started.elapsed().as_millis() as u64,
        "",
    );

    response
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    let payload = json!({ "error": message.into() });
    (status, axum::Json(payload)).into_response()
}

fn build_upstream_url(base_url: &str, upstream_path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let mut path = if upstream_path.starts_with('/') {
        upstream_path.to_string()
    } else {
        format!("/{upstream_path}")
    };

    // If provider base URL already includes `/v1`, avoid duplicating `/v1` in
    // paths like `/v1/chat/completions` -> `/v1/v1/chat/completions`.
    if base.ends_with("/v1") {
        if path == "/v1" {
            path.clear();
        } else if let Some(stripped) = path.strip_prefix("/v1/") {
            path = format!("/{stripped}");
        }
    }

    format!("{base}{path}")
}

fn emit_log(
    log_sink: &RouterLogSink,
    path: &str,
    request_id: &str,
    original_model: &str,
    routed_model: &str,
    provider: &str,
    stream: bool,
    status: u16,
    latency_ms: u64,
    error_summary: &str,
) {
    log_sink.emit(RequestLog {
        timestamp: RequestLog::now_timestamp(),
        request_id: request_id.to_string(),
        method: "POST".to_string(),
        path: path.to_string(),
        original_model: original_model.to_string(),
        routed_model: routed_model.to_string(),
        provider: provider.to_string(),
        status,
        latency_ms,
        stream,
        error_summary: error_summary.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::build_upstream_url;

    #[test]
    fn build_upstream_url_avoids_duplicate_v1() {
        let url = build_upstream_url("https://api.openai.com/v1", "/v1/chat/completions");
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_upstream_url_keeps_v1_when_base_has_no_version() {
        let url = build_upstream_url("https://api.openai.com", "/v1/chat/completions");
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn build_upstream_url_handles_trailing_slash_in_base() {
        let url = build_upstream_url("https://openrouter.ai/api/v1/", "/v1/responses");
        assert_eq!(url, "https://openrouter.ai/api/v1/responses");
    }
}
