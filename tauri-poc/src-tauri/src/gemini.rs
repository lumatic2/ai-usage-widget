use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";

#[derive(Debug)]
pub enum GeminiError {
    NotConfigured,
    SessionExpired,
    Other(String),
}

impl fmt::Display for GeminiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeminiError::NotConfigured => write!(f, "Gemini not configured (no ~/.gemini/oauth_creds.json)"),
            GeminiError::SessionExpired => write!(f, "Gemini session expired — run `gemini` CLI to refresh"),
            GeminiError::Other(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GeminiQuotaEntry {
    pub id: String,
    pub label: String,
    pub model: String,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at: Option<String>,
    pub unit: String,
}

#[derive(Clone, Debug)]
pub struct GeminiQuotaResponse {
    pub plan_type: Option<String>,
    pub quotas: Vec<GeminiQuotaEntry>,
}

#[derive(Deserialize)]
struct OauthCreds {
    access_token: String,
    #[serde(default)]
    expiry_date: Option<i64>,
}

fn oauth_path() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|h| PathBuf::from(h).join(".gemini").join("oauth_creds.json"))
}

fn read_oauth_token() -> Result<String, GeminiError> {
    let path = oauth_path().ok_or(GeminiError::NotConfigured)?;
    let raw = std::fs::read_to_string(&path).map_err(|_| GeminiError::NotConfigured)?;
    let creds: OauthCreds =
        serde_json::from_str(&raw).map_err(|e| GeminiError::Other(format!("oauth_creds parse: {e}")))?;
    if creds.access_token.is_empty() {
        return Err(GeminiError::NotConfigured);
    }
    if let Some(exp) = creds.expiry_date {
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Token already expired — return session expired so widget shows actionable message.
        if now_ms >= exp {
            return Err(GeminiError::SessionExpired);
        }
    }
    Ok(creds.access_token)
}

pub async fn fetch_quota(timeout_ms: u64, retries: u32) -> Result<GeminiQuotaResponse, GeminiError> {
    let token = read_oauth_token()?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| GeminiError::Other(format!("reqwest build: {e}")))?;

    let tier = post_code_assist(&client, &token, "loadCodeAssist", load_payload(None), retries).await?;
    let project_id = tier
        .get("cloudaicompanionProject")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| GeminiError::Other("cloudaicompanionProject missing in loadCodeAssist response".into()))?;

    let quota_resp = post_code_assist(
        &client,
        &token,
        "retrieveUserQuota",
        json!({ "project": project_id }),
        retries,
    )
    .await?;

    let plan_type = tier
        .get("paidTier")
        .and_then(|p| p.get("name"))
        .or_else(|| tier.get("currentTier").and_then(|c| c.get("name")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let quotas = parse_quota_buckets(&quota_resp);
    Ok(GeminiQuotaResponse { plan_type, quotas })
}

fn load_payload(project_id: Option<&str>) -> Value {
    let mut body = json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    if let Some(p) = project_id {
        body["cloudaicompanionProject"] = Value::String(p.to_string());
    }
    body
}

async fn post_code_assist(
    client: &reqwest::Client,
    token: &str,
    method: &str,
    body: Value,
    retries: u32,
) -> Result<Value, GeminiError> {
    let url = format!("{CODE_ASSIST_BASE}:{method}");
    let mut attempts = 0u32;
    loop {
        let resp = client
            .post(&url)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", "ai-usage-widget/0.1")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    return r
                        .json::<Value>()
                        .await
                        .map_err(|e| GeminiError::Other(format!("{method} body: {e}")));
                }
                if status == StatusCode::UNAUTHORIZED {
                    return Err(GeminiError::SessionExpired);
                }
                if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS) && attempts < retries {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(400 * (attempts as u64))).await;
                    continue;
                }
                let body_text = r.text().await.unwrap_or_default();
                return Err(GeminiError::Other(format!("{method} HTTP {status}: {body_text}")));
            }
            Err(e) if attempts < retries => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(400 * (attempts as u64))).await;
                let _ = e;
            }
            Err(e) => return Err(GeminiError::Other(format!("{method} request: {e}"))),
        }
    }
}

fn parse_quota_buckets(resp: &Value) -> Vec<GeminiQuotaEntry> {
    let Some(buckets) = resp.get("buckets").and_then(|b| b.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(buckets.len());
    for bucket in buckets {
        let model = bucket.get("modelId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if model.is_empty() {
            continue;
        }
        let remaining_fraction = bucket
            .get("remainingFraction")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let remaining_percent = (remaining_fraction * 100.0).clamp(0.0, 100.0);
        let used_percent = (100.0 - remaining_percent).clamp(0.0, 100.0);
        let reset_at = bucket
            .get("resetTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let unit = bucket
            .get("tokenType")
            .and_then(|v| v.as_str())
            .unwrap_or("REQUESTS")
            .to_lowercase();
        out.push(GeminiQuotaEntry {
            id: format!("gemini:{model}"),
            label: friendly_model_label(&model),
            model,
            used_percent,
            remaining_percent,
            reset_at,
            unit,
        });
    }
    // Pro models first, then by label. Cap to 3 (AgentCat parity).
    out.sort_by(|a, b| {
        let a_pro = !a.model.contains("pro");
        let b_pro = !b.model.contains("pro");
        a_pro.cmp(&b_pro).then(a.label.cmp(&b.label))
    });
    out.truncate(3);
    out
}

fn friendly_model_label(model: &str) -> String {
    match model {
        "gemini-3-pro-preview" => "Gemini 3 Pro".into(),
        "gemini-3.1-pro-preview" => "Gemini 3.1 Pro".into(),
        "gemini-2.5-pro" => "Gemini 2.5 Pro".into(),
        "gemini-3-flash-preview" => "Gemini 3 Flash".into(),
        "gemini-3.1-flash-lite-preview" | "gemini-3.1-flash-lite" => "Gemini 3.1 Flash Lite".into(),
        "gemini-2.5-flash" => "Gemini 2.5 Flash".into(),
        "gemini-2.5-flash-lite" => "Gemini 2.5 Flash Lite".into(),
        other => other.into(),
    }
}
