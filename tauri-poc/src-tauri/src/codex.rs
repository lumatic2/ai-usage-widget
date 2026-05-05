use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

use crate::WindowSlice;

const CHATGPT_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const USER_AGENT: &str = "AIUsageWidget/0.1.0 (Tauri)";

#[derive(Debug, Clone)]
pub struct CodexUsage {
    pub plan_type: String,
    pub primary: WindowSlice,
    pub secondary: WindowSlice,
}

#[derive(Deserialize)]
struct AuthTokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<AuthTokens>,
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
}

fn codex_home() -> PathBuf {
    if let Ok(custom) = std::env::var("CODEX_HOME") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".codex")
}

fn load_auth() -> Result<AuthFile, String> {
    let path = codex_home().join("auth.json");
    if !path.exists() {
        return Err(format!("Codex auth file not found: {}", path.display()));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("auth.json parse error: {e}"))
}

fn parse_window(v: Option<&serde_json::Value>) -> WindowSlice {
    let Some(obj) = v else {
        return WindowSlice {
            used_percent: Some(0.0),
            reset_after_seconds: None,
        };
    };
    WindowSlice {
        used_percent: Some(obj.get("used_percent").and_then(|x| x.as_f64()).unwrap_or(0.0)),
        reset_after_seconds: obj.get("reset_after_seconds").and_then(|x| x.as_i64()),
    }
}

pub async fn fetch_usage(timeout_ms: u64, max_retries: u32) -> Result<CodexUsage, String> {
    let auth = load_auth()?;
    let tokens = auth.tokens.unwrap_or(AuthTokens {
        access_token: None,
        account_id: None,
    });
    let access_token = tokens
        .access_token
        .or(auth.openai_api_key)
        .ok_or_else(|| "Codex access token is missing from auth.json".to_string())?;
    let account_id = tokens.account_id.unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(2000)))
        .build()
        .map_err(|e| e.to_string())?;

    let body = send_with_retries(&client, &access_token, &account_id, max_retries).await?;

    let plan_type = body
        .get("plan_type")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_uppercase();
    let rate_limit = body.get("rate_limit");
    let primary = parse_window(rate_limit.and_then(|r| r.get("primary_window")));
    let secondary = parse_window(rate_limit.and_then(|r| r.get("secondary_window")));

    Ok(CodexUsage {
        plan_type,
        primary,
        secondary,
    })
}

async fn send_with_retries(
    client: &reqwest::Client,
    access_token: &str,
    account_id: &str,
    max_retries: u32,
) -> Result<serde_json::Value, String> {
    let mut last_error: Option<String> = None;
    for attempt in 0..=max_retries {
        let mut req = client
            .get(CHATGPT_USAGE_URL)
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", USER_AGENT);
        if !account_id.is_empty() {
            req = req.header("ChatGPT-Account-Id", account_id);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err("Codex login is expired. Please login again.".into());
                }
                if !status.is_success() {
                    let code = status.as_u16();
                    let retryable = code == 429 || code >= 500;
                    if retryable && attempt < max_retries {
                        sleep_backoff(attempt).await;
                        last_error = Some(format!("Codex usage request failed: {code}"));
                        continue;
                    }
                    return Err(format!("Codex usage request failed: {code}"));
                }
                return resp
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| format!("Codex usage parse error: {e}"));
            }
            Err(err) => {
                let retryable =
                    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body();
                let msg = if err.is_timeout() {
                    "Usage request timed out. Please try again.".to_string()
                } else {
                    err.to_string()
                };
                if retryable && attempt < max_retries {
                    sleep_backoff(attempt).await;
                    last_error = Some(msg);
                    continue;
                }
                return Err(msg);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "Usage request failed".into()))
}

async fn sleep_backoff(attempt: u32) {
    let ms = 400u64 * u64::from(attempt + 1);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}
