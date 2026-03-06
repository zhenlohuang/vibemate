use std::path::PathBuf;
use std::time::Duration;

use crate::agent::usage_source::cli_runner::run_command;
use crate::error::{AppError, Result};

pub async fn extract_cookie(
    browser: Option<&str>,
    domain_like: &str,
    cookie_name: &str,
) -> Result<String> {
    let browser = browser.unwrap_or("chrome").trim().to_ascii_lowercase();
    match browser.as_str() {
        "firefox" => extract_firefox_cookie(domain_like, cookie_name).await,
        "chrome" => extract_chrome_cookie(domain_like, cookie_name).await,
        "safari" => Err(AppError::CookieExtraction(
            "Safari cookie extraction is not implemented yet".to_string(),
        )),
        other => Err(AppError::CookieExtraction(format!(
            "Unsupported cookie browser `{other}`"
        ))),
    }
}

async fn extract_firefox_cookie(domain_like: &str, cookie_name: &str) -> Result<String> {
    let profiles_root = home_path(&["Library", "Application Support", "Firefox", "Profiles"])?;
    let mut db_path = None;
    if let Ok(entries) = std::fs::read_dir(&profiles_root) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("cookies.sqlite");
            if candidate.exists() {
                db_path = Some(candidate);
                break;
            }
        }
    }

    let db_path = db_path.ok_or_else(|| {
        AppError::CookieExtraction(format!(
            "Firefox cookies.sqlite not found under {}",
            profiles_root.display()
        ))
    })?;

    query_cookie_sqlite(&db_path, "moz_cookies", "host", domain_like, cookie_name).await
}

async fn extract_chrome_cookie(domain_like: &str, cookie_name: &str) -> Result<String> {
    let db_path = home_path(&[
        "Library",
        "Application Support",
        "Google",
        "Chrome",
        "Default",
        "Cookies",
    ])?;
    if !db_path.exists() {
        return Err(AppError::CookieExtraction(format!(
            "Chrome cookie DB not found at {}",
            db_path.display()
        )));
    }

    let result =
        query_cookie_sqlite(&db_path, "cookies", "host_key", domain_like, cookie_name).await?;
    if result.is_empty() {
        return Err(AppError::CookieExtraction(format!(
            "Cookie `{cookie_name}` for `{domain_like}` is empty or encrypted"
        )));
    }
    Ok(result)
}

async fn query_cookie_sqlite(
    db_path: &PathBuf,
    table: &str,
    domain_column: &str,
    domain_like: &str,
    cookie_name: &str,
) -> Result<String> {
    let temp_db = std::env::temp_dir().join(format!(
        "vibemate-cookie-{}-{}.sqlite",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::copy(db_path, &temp_db).map_err(|err| {
        AppError::CookieExtraction(format!(
            "Failed to copy cookie DB {}: {err}",
            db_path.display()
        ))
    })?;

    let sql = format!(
        "select value from {table} where {domain_column} like '%{domain_like}%' and name = '{cookie_name}' order by lastAccessed desc limit 1;"
    );
    let result = run_command(
        "sqlite3",
        &[temp_db.to_string_lossy().as_ref(), &sql],
        None,
        Duration::from_secs(5),
    )
    .await
    .map_err(|err| AppError::CookieExtraction(err.to_string()))?;

    let _ = std::fs::remove_file(&temp_db);
    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::CookieExtraction(format!(
            "Cookie `{cookie_name}` for `{domain_like}` not found in {}",
            db_path.display()
        )));
    }
    Ok(trimmed)
}

fn home_path(components: &[&str]) -> Result<PathBuf> {
    let mut path = dirs::home_dir().ok_or_else(|| {
        AppError::CookieExtraction("Unable to resolve home directory".to_string())
    })?;
    for component in components {
        path.push(component);
    }
    Ok(path)
}
