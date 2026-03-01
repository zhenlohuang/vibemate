pub mod handler;
pub mod logging;
pub mod middleware;
pub mod router;
pub mod server;
pub mod stream;

use chrono::{Local, SecondsFormat};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestLog {
    pub timestamp: String,
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub original_model: String,
    #[serde(alias = "model")]
    pub routed_model: String,
    pub provider: String,
    pub status: u16,
    pub latency_ms: u64,
    pub stream: bool,
    pub error_summary: String,
}

impl RequestLog {
    pub fn now_timestamp() -> String {
        Local::now().to_rfc3339_opts(SecondsFormat::Millis, false)
    }
}
