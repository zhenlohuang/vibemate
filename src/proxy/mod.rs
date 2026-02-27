pub mod handler;
pub mod middleware;
pub mod router;
pub mod server;
pub mod stream;

#[derive(Debug, Clone, Default)]
pub struct RequestLog {
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub model: String,
    pub provider: String,
    pub status: u16,
    pub latency_ms: u64,
}
