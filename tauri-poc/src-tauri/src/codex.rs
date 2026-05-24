use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::WindowSlice;

#[derive(Debug, Clone)]
pub enum CodexError {
    NotConfigured,
    SessionExpired,
    Other(String),
}

impl fmt::Display for CodexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodexError::NotConfigured => {
                write!(f, "Codex not configured. Run `codex` CLI to sign in.")
            }
            CodexError::SessionExpired => write!(
                f,
                "Codex session expired. Run `codex` CLI to sign in again."
            ),
            CodexError::Other(s) => write!(f, "{s}"),
        }
    }
}

const CHATGPT_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const USER_AGENT: &str = "AIUsageWidget/0.1.1 (Tauri)";
const CODEX_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

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
    refresh_token: Option<String>,
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

fn load_auth() -> Result<AuthFile, CodexError> {
    let path = codex_home().join("auth.json");
    if !path.exists() {
        return Err(CodexError::NotConfigured);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| CodexError::Other(e.to_string()))?;
    serde_json::from_str(&raw)
        .map_err(|e| CodexError::Other(format!("auth.json parse error: {e}")))
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

pub async fn fetch_usage(timeout_ms: u64, max_retries: u32) -> Result<CodexUsage, CodexError> {
    let auth = load_auth()?;
    let tokens = auth.tokens.unwrap_or(AuthTokens {
        access_token: None,
        account_id: None,
        refresh_token: None,
    });
    let access_token = tokens
        .access_token
        .or(auth.openai_api_key)
        .ok_or(CodexError::NotConfigured)?;
    let account_id = tokens.account_id.unwrap_or_default();
    let refresh_token = tokens.refresh_token.unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(2000)))
        .build()
        .map_err(|e| CodexError::Other(e.to_string()))?;

    let body =
        send_with_retries(&client, &access_token, &account_id, &refresh_token, max_retries).await?;

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
    refresh_token: &str,
    max_retries: u32,
) -> Result<serde_json::Value, CodexError> {
    let mut last_error: Option<CodexError> = None;
    let mut token = access_token.to_string();
    let mut refreshed_once = false;
    for attempt in 0..=max_retries {
        let mut req = client
            .get(CHATGPT_USAGE_URL)
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT);
        if !account_id.is_empty() {
            req = req.header("ChatGPT-Account-Id", account_id);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    if !refreshed_once && !refresh_token.is_empty() {
                        refreshed_once = true;
                        match refresh_and_persist(client, refresh_token).await {
                            Ok(new_token) => {
                                token = new_token;
                                continue;
                            }
                            Err(_) => return Err(CodexError::SessionExpired),
                        }
                    }
                    return Err(CodexError::SessionExpired);
                }
                if !status.is_success() {
                    let code = status.as_u16();
                    let retryable = code == 429 || code >= 500;
                    if retryable && attempt < max_retries {
                        sleep_backoff(attempt).await;
                        last_error =
                            Some(CodexError::Other(format!("Codex usage request failed: {code}")));
                        continue;
                    }
                    return Err(CodexError::Other(format!("Codex usage request failed: {code}")));
                }
                return resp
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| CodexError::Other(format!("Codex usage parse error: {e}")));
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
                    last_error = Some(CodexError::Other(msg));
                    continue;
                }
                return Err(CodexError::Other(msg));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| CodexError::Other("Usage request failed".into())))
}

async fn sleep_backoff(attempt: u32) {
    let ms = 400u64 * u64::from(attempt + 1);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
}

async fn refresh_and_persist(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<String, CodexError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CODEX_OAUTH_CLIENT_ID),
    ];
    let resp = client
        .post(CODEX_OAUTH_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .form(&form)
        .send()
        .await
        .map_err(|e| CodexError::Other(format!("token refresh failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(CodexError::Other(format!(
            "token refresh failed: {}",
            resp.status().as_u16()
        )));
    }
    let body: RefreshResponse = resp
        .json()
        .await
        .map_err(|e| CodexError::Other(format!("token refresh parse: {e}")))?;
    let new_access = body
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CodexError::Other("token refresh: empty access_token".into()))?;

    // Write-back to auth.json preserving unknown fields.
    let path = codex_home().join("auth.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(tokens) = v.get_mut("tokens").and_then(|t| t.as_object_mut()) {
                tokens.insert(
                    "access_token".into(),
                    serde_json::Value::String(new_access.clone()),
                );
                if let Some(id) = body.id_token.filter(|s| !s.is_empty()) {
                    tokens.insert("id_token".into(), serde_json::Value::String(id));
                }
                if let Some(rt) = body.refresh_token.filter(|s| !s.is_empty()) {
                    tokens.insert("refresh_token".into(), serde_json::Value::String(rt));
                }
            }
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "last_refresh".into(),
                    serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                );
            }
            if let Ok(json) = serde_json::to_string_pretty(&v) {
                let _ = std::fs::write(&path, json);
            }
        }
    }
    Ok(new_access)
}
