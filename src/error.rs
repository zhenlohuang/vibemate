use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Token expired for {agent}. Please run `vibemate login {agent}`")]
    TokenExpired { agent: String },

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Missing 'model' field in request body")]
    MissingModel,

    #[error("Proxy server error: {0}")]
    ProxyServer(String),

    #[error("Upstream error: HTTP {status} from {provider}")]
    Upstream { status: u16, provider: String },
}

pub type Result<T> = std::result::Result<T, AppError>;
