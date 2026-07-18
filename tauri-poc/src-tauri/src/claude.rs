use chrono::DateTime;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

use crate::WindowSlice;

const ORGS_URL: &str = "https://claude.ai/api/organizations";
// The Claude Code OAuth token is rejected (403) by claude.ai/api/* — subscription
// usage for that token lives on api.anthropic.com behind an oauth beta header.
const OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ClaudeUsage {
    pub primary: WindowSlice,
    pub secondary: WindowSlice,
    pub scoped: Vec<ScopedWindow>,
    /// Only meaningful on the cookie fallback path; empty for the OAuth path.
    pub org_uuid: String,
    pub account_label: Option<String>,
}

/// Model-scoped usage window (e.g. the Fable-only weekly limit) from the
/// `limits` array of the OAuth usage response.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    pub reset_after_seconds: Option<i64>,
}

#[derive(Debug)]
pub enum ClaudeError {
    NotConfigured,
    SessionExpired,
    Other(String),
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::NotConfigured => write!(f, "Claude credentials not found"),
            ClaudeError::SessionExpired => write!(f, "Claude session expired. Please log in again."),
            ClaudeError::Other(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Deserialize)]
struct OauthBlock {
    #[serde(default, rename = "accessToken")]
    access_token: Option<String>,
    #[serde(default, rename = "subscriptionType")]
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(default, rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
}

pub struct ClaudeCredentials {
    pub access_token: String,
    pub subscription_type: Option<String>,
}

fn claude_home() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".claude")
}

#[derive(Deserialize)]
pub struct OauthAccount {
    #[serde(default, rename = "emailAddress")]
    pub email_address: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeJson {
    #[serde(default, rename = "oauthAccount")]
    oauth_account: Option<OauthAccount>,
}

/// Account identity of the current Claude Code login (`~/.claude.json`).
/// This file is rewritten on `claude login`, so it always matches the
/// access token in `.credentials.json` — unlike the widget's cached org UUID.
pub fn load_account_info() -> Option<OauthAccount> {
    let path = account_info_path();
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed: ClaudeJson = serde_json::from_str(&raw).ok()?;
    parsed.oauth_account
}

pub fn account_info_path() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".claude.json")
}

pub fn current_account_email() -> Option<String> {
    load_account_info()
        .and_then(|account| account.email_address)
        .map(|email| email.trim().to_ascii_lowercase())
        .filter(|email| !email.is_empty())
}

/// Rewritten by `claude login` — watched by lib.rs to follow account switches.
pub fn credentials_path() -> PathBuf {
    claude_home().join(".credentials.json")
}

pub fn load_credentials() -> Option<ClaudeCredentials> {
    let path = credentials_path();
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed: CredentialsFile = serde_json::from_str(&raw).ok()?;
    let oauth = parsed.claude_ai_oauth?;
    let subscription_type = oauth
        .subscription_type
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let token = oauth.access_token?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(ClaudeCredentials {
        access_token: token,
        subscription_type,
    })
}

fn build_client(timeout_ms: u64) -> Result<reqwest::Client, ClaudeError> {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(15_000)))
        .build()
        .map_err(|e| ClaudeError::Other(e.to_string()))
}

/// Parse `/api/organizations` response into (uuid, display name) of the first org.
fn parse_first_org(body: &serde_json::Value) -> Option<(String, Option<String>)> {
    let org = match body {
        serde_json::Value::Array(arr) => arr.first()?,
        serde_json::Value::Object(_) => body,
        _ => return None,
    };
    let uuid = org
        .get("uuid")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())?;
    let name = org
        .get("name")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some((uuid, name))
}

async fn resolve_org(
    client: &reqwest::Client,
    auth: &AuthHeader<'_>,
) -> Result<(String, Option<String>), ClaudeError> {
    let req = client
        .get(ORGS_URL)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);
    let req = match auth {
        AuthHeader::Bearer(t) => req.header("Authorization", format!("Bearer {t}")),
        AuthHeader::Cookie(c) => req.header("Cookie", format!("sessionKey={c}")),
    };
    let resp = req
        .send()
        .await
        .map_err(|e| ClaudeError::Other(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ClaudeError::SessionExpired);
    }
    if !status.is_success() {
        return Err(ClaudeError::Other(format!(
            "Claude organizations request failed: {}",
            status.as_u16()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ClaudeError::Other(format!("organizations parse error: {e}")))?;
    parse_first_org(&body).ok_or_else(|| ClaudeError::Other("Claude org UUID not found".into()))
}

fn seconds_until_rfc3339(s: &str) -> Option<i64> {
    let dt = DateTime::parse_from_rfc3339(s).ok()?;
    let now = chrono::Utc::now().timestamp();
    Some((dt.timestamp() - now).max(0))
}

fn parse_window(payload: Option<&serde_json::Value>) -> WindowSlice {
    let Some(obj) = payload.and_then(|v| v.as_object()) else {
        return WindowSlice {
            used_percent: None,
            reset_after_seconds: None,
        };
    };
    let used_percent = obj
        .get("utilization")
        .and_then(|v| v.as_f64())
        .map(|n| n.round().clamp(0.0, 100.0));
    let reset_after_seconds = obj
        .get("resets_at")
        .and_then(|v| v.as_str())
        .and_then(seconds_until_rfc3339);
    WindowSlice {
        used_percent,
        reset_after_seconds,
    }
}

/// Pull model-scoped windows (e.g. the Fable weekly limit) out of the OAuth
/// usage response's `limits` array. Unscoped entries (`session`, `weekly_all`)
/// duplicate `five_hour`/`seven_day` and are skipped.
fn parse_scoped_windows(payload: &serde_json::Value) -> Vec<ScopedWindow> {
    let Some(limits) = payload.get("limits").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    limits
        .iter()
        .filter_map(|limit| {
            let label = limit
                .get("scope")?
                .get("model")?
                .get("display_name")?
                .as_str()?
                .trim()
                .to_string();
            if label.is_empty() {
                return None;
            }
            let used_percent = limit
                .get("percent")
                .and_then(|v| v.as_f64())
                .map(|n| n.round().clamp(0.0, 100.0));
            let reset_after_seconds = limit
                .get("resets_at")
                .and_then(|v| v.as_str())
                .and_then(seconds_until_rfc3339);
            Some(ScopedWindow {
                label,
                used_percent,
                reset_after_seconds,
            })
        })
        .collect()
}

fn detect_permission_error(payload: &serde_json::Value) -> bool {
    let direct = payload
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    let nested = payload
        .get("error")
        .and_then(|v| v.get("type"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    matches!(direct.as_deref(), Some("permission_error"))
        || matches!(nested.as_deref(), Some("permission_error"))
}

enum AuthHeader<'a> {
    Bearer(&'a str),
    Cookie(&'a str),
}

async fn fetch_usage_payload_inner(
    client: &reqwest::Client,
    auth: &AuthHeader<'_>,
    url: &str,
    max_retries: u32,
) -> Result<serde_json::Value, ClaudeError> {
    let mut last_other: Option<ClaudeError> = None;
    for attempt in 0..=max_retries {
        let req = client
            .get(url)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT);
        let req = match auth {
            AuthHeader::Bearer(t) => req
                .header("Authorization", format!("Bearer {t}"))
                .header("anthropic-beta", OAUTH_BETA),
            AuthHeader::Cookie(c) => req.header("Cookie", format!("sessionKey={c}")),
        };
        let send_result = req.send().await;
        let resp = match send_result {
            Ok(r) => r,
            Err(e) => {
                let retryable = e.is_timeout() || e.is_connect() || e.is_request() || e.is_body();
                let msg = if e.is_timeout() {
                    "Claude usage request timed out.".to_string()
                } else {
                    e.to_string()
                };
                if retryable && attempt < max_retries {
                    sleep_backoff(attempt).await;
                    last_other = Some(ClaudeError::Other(msg));
                    continue;
                }
                return Err(ClaudeError::Other(msg));
            }
        };
        let status = resp.status();
        let body_text = match resp.text().await {
            Ok(t) => t,
            Err(e) => return Err(ClaudeError::Other(e.to_string())),
        };
        let payload: Option<serde_json::Value> = if body_text.is_empty() {
            None
        } else {
            serde_json::from_str(&body_text).ok()
        };
        let permission_error = payload.as_ref().map(detect_permission_error).unwrap_or(false);
        if status.as_u16() == 401 || status.as_u16() == 403 || permission_error {
            return Err(ClaudeError::SessionExpired);
        }
        if !status.is_success() {
            let code = status.as_u16();
            let retryable = code == 429 || code >= 500;
            if retryable && attempt < max_retries {
                sleep_backoff(attempt).await;
                last_other = Some(ClaudeError::Other(format!(
                    "Claude usage request failed: {code}"
                )));
                continue;
            }
            return Err(ClaudeError::Other(format!(
                "Claude usage request failed: {code}"
            )));
        }
        return payload.ok_or_else(|| ClaudeError::Other("Claude usage response was empty".into()));
    }
    Err(last_other.unwrap_or_else(|| ClaudeError::Other("Claude usage request failed".into())))
}

async fn sleep_backoff(attempt: u32) {
    let ms = 400u64 * u64::from(attempt + 1);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

fn org_usage_url(org_uuid: &str) -> String {
    format!(
        "https://claude.ai/api/organizations/{}/usage",
        urlencoding(org_uuid)
    )
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub async fn fetch_usage(timeout_ms: u64, max_retries: u32) -> Result<ClaudeUsage, ClaudeError> {
    let creds = load_credentials().ok_or(ClaudeError::NotConfigured)?;
    let client = build_client(timeout_ms)?;
    // The OAuth usage endpoint is account-scoped — no org UUID involved, so the
    // response always belongs to the *current* Claude Code login.
    let payload = fetch_usage_payload_inner(
        &client,
        &AuthHeader::Bearer(&creds.access_token),
        OAUTH_USAGE_URL,
        max_retries,
    )
    .await?;
    let email = current_account_email();
    let account_label = email.map(|e| match &creds.subscription_type {
        Some(sub) => format!("{e} · {sub}"),
        None => e,
    });
    Ok(ClaudeUsage {
        primary: parse_window(payload.get("five_hour")),
        secondary: parse_window(payload.get("seven_day")),
        scoped: parse_scoped_windows(&payload),
        org_uuid: String::new(),
        account_label,
    })
}

async fn fetch_usage_payload_with_cookie(
    client: &reqwest::Client,
    session_key: &str,
    org_uuid: &str,
    max_retries: u32,
) -> Result<serde_json::Value, ClaudeError> {
    let url = org_usage_url(org_uuid);
    fetch_usage_payload_inner(client, &AuthHeader::Cookie(session_key), &url, max_retries).await
}

pub async fn resolve_org_with_cookie(
    timeout_ms: u64,
    session_key: &str,
) -> Result<(String, Option<String>), ClaudeError> {
    let client = build_client(timeout_ms)?;
    resolve_org(&client, &AuthHeader::Cookie(session_key)).await
}

pub async fn fetch_usage_with_cookie(
    timeout_ms: u64,
    max_retries: u32,
    session_key: &str,
    cached_org_uuid: Option<String>,
) -> Result<ClaudeUsage, ClaudeError> {
    let client = build_client(timeout_ms)?;
    let (org_uuid, account_label) = match cached_org_uuid {
        Some(u) if !u.is_empty() => (u, None),
        _ => {
            let (uuid, name) = resolve_org(&client, &AuthHeader::Cookie(session_key)).await?;
            (uuid, name)
        }
    };
    let payload = fetch_usage_payload_with_cookie(&client, session_key, &org_uuid, max_retries).await?;
    Ok(ClaudeUsage {
        primary: parse_window(payload.get("five_hour")),
        secondary: parse_window(payload.get("seven_day")),
        // claude.ai's org usage response carries the same `limits` array as
        // the OAuth endpoint, so model-scoped windows work on this path too.
        scoped: parse_scoped_windows(&payload),
        org_uuid,
        account_label,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_scoped_windows;

    #[test]
    fn parses_model_scoped_limits_only() {
        let payload = serde_json::json!({
            "limits": [
                { "kind": "session", "group": "session", "percent": 21,
                  "resets_at": "2099-01-01T00:00:00+00:00", "scope": null },
                { "kind": "weekly_all", "group": "weekly", "percent": 11,
                  "resets_at": "2099-01-01T00:00:00+00:00", "scope": null },
                { "kind": "weekly_scoped", "group": "weekly", "percent": 18.4,
                  "resets_at": "2099-01-01T00:00:00+00:00",
                  "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null } }
            ]
        });
        let scoped = parse_scoped_windows(&payload);
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].label, "Fable");
        assert_eq!(scoped[0].used_percent, Some(18.0));
        assert!(scoped[0].reset_after_seconds.unwrap() > 0);
    }

    #[test]
    fn missing_limits_array_yields_empty() {
        assert!(parse_scoped_windows(&serde_json::json!({})).is_empty());
        assert!(parse_scoped_windows(&serde_json::json!({ "limits": "x" })).is_empty());
    }
}
