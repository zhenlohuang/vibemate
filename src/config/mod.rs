mod types;

pub use types::{AppConfig, RoutingConfig, RoutingRule};

use crate::error::{AppError, Result};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG: &str = r#"[server]
host = "127.0.0.1"
port = 12345

[routing]
default_provider = "openai-official"
rules = []
"#;

pub fn load_config(path: &Path) -> Result<AppConfig> {
    ensure_vibemate_dir()?;

    let resolved_path = expand_tilde(path);
    if let Some(parent) = resolved_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !resolved_path.exists() {
        fs::write(&resolved_path, DEFAULT_CONFIG)?;
    }

    let raw = fs::read_to_string(&resolved_path)?;
    let config = toml::from_str::<AppConfig>(&raw)?;
    Ok(config)
}

fn ensure_vibemate_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| AppError::Config("Unable to find home directory".to_string()))?;
    let dir = home.join(".vibemate");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }

    if let Some(suffix) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(suffix);
        }
    }

    path.to_path_buf()
}
