use chrono::DateTime;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

use crate::WindowSlice;

const ORGS_URL: &str = "https://claude.ai/api/organizations";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ClaudeUsage {
    pub primary: WindowSlice,
    pub secondary: WindowSlice,
    pub org_uuid: String,
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
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(default, rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
    #[serde(default, rename = "organizationUuid")]
    organization_uuid: Option<String>,
}

pub struct ClaudeCredentials {
    pub access_token: String,
    pub organization_uuid: Option<String>,
}

fn claude_home() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".claude")
}

pub fn load_credentials() -> Option<ClaudeCredentials> {
    let path = claude_home().join(".credentials.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed: CredentialsFile = serde_json::from_str(&raw).ok()?;
    let oauth = parsed.claude_ai_oauth?;
    let token = oauth.access_token?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    let org = parsed
        .organization_uuid
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(ClaudeCredentials {
        access_token: token,
        organization_uuid: org,
    })
}

fn build_client(timeout_ms: u64) -> Result<reqwest::Client, ClaudeError> {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(15_000)))
        .build()
        .map_err(|e| ClaudeError::Other(e.to_string()))
}

async fn resolve_org_uuid(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<String, ClaudeError> {
    let resp = client
        .get(ORGS_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
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
    let uuid = match &body {
        serde_json::Value::Array(arr) => arr
            .first()
            .and_then(|x| x.get("uuid"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        serde_json::Value::Object(_) => body.get("uuid").and_then(|x| x.as_str()).map(|s| s.to_string()),
        _ => None,
    };
    uuid.filter(|s| !s.is_empty())
        .ok_or_else(|| ClaudeError::Other("Claude org UUID not found".into()))
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
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| {
            let now = chrono::Utc::now().timestamp();
            (dt.timestamp() - now).max(0)
        });
    WindowSlice {
        used_percent,
        reset_after_seconds,
    }
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

async fn fetch_usage_payload(
    client: &reqwest::Client,
    access_token: &str,
    org_uuid: &str,
) -> Result<serde_json::Value, ClaudeError> {
    let url = format!(
        "https://claude.ai/api/organizations/{}/usage",
        urlencoding(org_uuid)
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| ClaudeError::Other(e.to_string()))?;
    let status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| ClaudeError::Other(e.to_string()))?;
    let payload: Option<serde_json::Value> = if body_text.is_empty() {
        None
    } else {
        serde_json::from_str(&body_text).ok()
    };
    let permission_error = payload
        .as_ref()
        .map(detect_permission_error)
        .unwrap_or(false);
    if status.as_u16() == 401 || status.as_u16() == 403 || permission_error {
        return Err(ClaudeError::SessionExpired);
    }
    if !status.is_success() {
        return Err(ClaudeError::Other(format!(
            "Claude usage request failed: {}",
            status.as_u16()
        )));
    }
    payload.ok_or_else(|| ClaudeError::Other("Claude usage response was empty".into()))
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

pub async fn fetch_usage(
    timeout_ms: u64,
    cached_org_uuid: Option<String>,
) -> Result<ClaudeUsage, ClaudeError> {
    let creds = load_credentials().ok_or(ClaudeError::NotConfigured)?;
    let client = build_client(timeout_ms)?;
    let org_uuid = match creds.organization_uuid.or(cached_org_uuid) {
        Some(u) => u,
        None => resolve_org_uuid(&client, &creds.access_token).await?,
    };
    let payload = fetch_usage_payload(&client, &creds.access_token, &org_uuid).await?;
    Ok(ClaudeUsage {
        primary: parse_window(payload.get("five_hour")),
        secondary: parse_window(payload.get("seven_day")),
        org_uuid,
    })
}

async fn fetch_usage_payload_with_cookie(
    client: &reqwest::Client,
    session_key: &str,
    org_uuid: &str,
) -> Result<serde_json::Value, ClaudeError> {
    let url = format!(
        "https://claude.ai/api/organizations/{}/usage",
        urlencoding(org_uuid)
    );
    let resp = client
        .get(&url)
        .header("Cookie", format!("sessionKey={session_key}"))
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| ClaudeError::Other(e.to_string()))?;
    let status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| ClaudeError::Other(e.to_string()))?;
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
        return Err(ClaudeError::Other(format!(
            "Claude usage request failed: {}",
            status.as_u16()
        )));
    }
    payload.ok_or_else(|| ClaudeError::Other("Claude usage response was empty".into()))
}

pub async fn resolve_org_uuid_with_cookie(
    timeout_ms: u64,
    session_key: &str,
) -> Result<String, ClaudeError> {
    let client = build_client(timeout_ms)?;
    let resp = client
        .get(ORGS_URL)
        .header("Cookie", format!("sessionKey={session_key}"))
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
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
    let uuid = match &body {
        serde_json::Value::Array(arr) => arr
            .first()
            .and_then(|x| x.get("uuid"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        serde_json::Value::Object(_) => body.get("uuid").and_then(|x| x.as_str()).map(|s| s.to_string()),
        _ => None,
    };
    uuid.filter(|s| !s.is_empty())
        .ok_or_else(|| ClaudeError::Other("Claude org UUID not found".into()))
}

pub async fn fetch_usage_with_cookie(
    timeout_ms: u64,
    session_key: &str,
    cached_org_uuid: Option<String>,
) -> Result<ClaudeUsage, ClaudeError> {
    let client = build_client(timeout_ms)?;
    let org_uuid = match cached_org_uuid {
        Some(u) if !u.is_empty() => u,
        _ => resolve_org_uuid_with_cookie(timeout_ms, session_key).await?,
    };
    let payload = fetch_usage_payload_with_cookie(&client, session_key, &org_uuid).await?;
    Ok(ClaudeUsage {
        primary: parse_window(payload.get("five_hour")),
        secondary: parse_window(payload.get("seven_day")),
        org_uuid,
    })
}
