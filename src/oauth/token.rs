use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub last_refresh: Option<DateTime<Utc>>,
}

pub fn save_token(path: &Path, token: &TokenData) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = serde_json::to_string_pretty(token)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn load_token(path: &Path) -> Result<Option<TokenData>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    let token = serde_json::from_str::<TokenData>(&raw)?;
    Ok(Some(token))
}

pub fn is_expired(token: &TokenData) -> bool {
    token.expires_at <= Utc::now()
}

pub fn vibemate_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::Config("Unable to find home directory".to_string()))?;
    let dir = home.join(".vibemate");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn auth_dir() -> Result<PathBuf> {
    let dir = vibemate_dir()?.join("auth");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn auth_file_path(file_name: &str) -> Result<PathBuf> {
    Ok(auth_dir()?.join(file_name))
}
