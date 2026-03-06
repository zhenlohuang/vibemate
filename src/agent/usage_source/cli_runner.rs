use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{AppError, Result};

pub fn resolve_binary(configured_path: Option<&str>, default_binary: &str) -> Option<String> {
    if let Some(path) = configured_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    which::which(default_binary)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

pub async fn run_command(
    binary: &str,
    args: &[&str],
    stdin: Option<&str>,
    timeout: Duration,
) -> Result<String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .map_err(|err| AppError::CliSubprocess(format!("Failed to spawn `{binary}`: {err}")))?;

    if let Some(input) = stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        child_stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|err| {
                AppError::CliSubprocess(format!("Failed to write to `{binary}` stdin: {err}"))
            })?;
    }

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            AppError::CliSubprocess(format!("`{binary}` timed out after {}s", timeout.as_secs()))
        })?
        .map_err(|err| AppError::CliSubprocess(format!("Failed to wait for `{binary}`: {err}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let detail = if stderr.is_empty() {
            stdout.clone()
        } else {
            stderr
        };
        return Err(AppError::CliSubprocess(format!(
            "`{binary}` exited with status {}: {}",
            output.status, detail
        )));
    }

    if stdout.is_empty() {
        return Err(AppError::CliSubprocess(format!(
            "`{binary}` produced empty output"
        )));
    }

    Ok(stdout)
}
