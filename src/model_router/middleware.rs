use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header::HeaderName};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::trace::TraceLayer;

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn apply_middleware(router: Router) -> Router {
    router
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(request_id_middleware))
}

async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let request_id = REQUEST_ID_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .to_string();
    let header_name = HeaderName::from_static("x-request-id");

    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        req.headers_mut()
            .insert(header_name.clone(), header_value.clone());
        let mut response = next.run(req).await;
        response.headers_mut().insert(header_name, header_value);
        return response;
    }

    next.run(req).await
}
