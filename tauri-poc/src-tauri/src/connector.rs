use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TokenStats {
    pub total: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cached: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorSnapshot {
    pub schema_version: u32,
    pub provider: String,
    pub session_id: String,
    pub generated_at: DateTime<Utc>,
    pub transcript_path: Option<String>,
    pub tokens: Option<TokenStats>,
    pub model: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProviderState {
    pub last_snapshot: ConnectorSnapshot,
    pub session_tokens_total: u64,
    pub daily_tokens_total: u64,
    pub daily_date: NaiveDate,
    pub last_seen: Instant,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DailyEntry {
    date: NaiveDate,
    daily_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct AgentEvent {
    pub event: String,                // "stop" | "session_end"
    pub session_id: String,
    pub cwd: Option<String>,
    pub at: DateTime<Utc>,
}

pub struct ConnectorStore {
    providers: Mutex<HashMap<String, ProviderState>>,
    agent_events: Mutex<HashMap<String, AgentEvent>>, // keyed by session_id
    session_pids: Mutex<HashMap<String, u32>>,         // session_id → parent (Claude Code) pid
    persistence_path: PathBuf,
    session_pids_path: PathBuf,
    last_good_sessions: Mutex<HashMap<String, crate::agent_office::SessionInfo>>,
}

impl ConnectorStore {
    pub fn new(persistence_path: PathBuf) -> Self {
        let providers = load_persisted(&persistence_path);
        let session_pids_path = persistence_path
            .parent()
            .map(|p| p.join("connector_session_pids.json"))
            .unwrap_or_else(|| PathBuf::from("connector_session_pids.json"));
        let session_pids = load_session_pids(&session_pids_path);
        Self {
            providers: Mutex::new(providers),
            agent_events: Mutex::new(HashMap::new()),
            session_pids: Mutex::new(session_pids),
            persistence_path,
            session_pids_path,
            last_good_sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_session_pid(&self, session_id: &str) -> Option<u32> {
        self.session_pids.lock().ok()?.get(session_id).copied()
    }

    pub fn record_session_pid(&self, session_id: String, pid: u32) {
        if let Ok(mut g) = self.session_pids.lock() {
            g.insert(session_id, pid);
            save_session_pids(&self.session_pids_path, &g);
        }
    }

    pub fn purge_dead_pids(&self, live_pids: &std::collections::HashSet<u32>) {
        if let Ok(mut g) = self.session_pids.lock() {
            let before = g.len();
            g.retain(|_, pid| live_pids.contains(pid));
            if g.len() != before {
                save_session_pids(&self.session_pids_path, &g);
            }
        }
    }

    pub fn cache_session(&self, session: &crate::agent_office::SessionInfo) {
        if let Ok(mut g) = self.last_good_sessions.lock() {
            g.insert(session.session_id.clone(), session.clone());
        }
    }

    pub fn get_cached_session(&self, session_id: &str) -> Option<crate::agent_office::SessionInfo> {
        self.last_good_sessions.lock().ok()?.get(session_id).cloned()
    }

    pub fn get(&self, provider: &str) -> Option<ProviderState> {
        self.providers.lock().ok()?.get(provider).cloned()
    }

    pub fn get_agent_event(&self, session_id: &str) -> Option<AgentEvent> {
        self.agent_events.lock().ok()?.get(session_id).cloned()
    }

    pub fn record_agent_event(&self, event: AgentEvent) {
        if let Ok(mut g) = self.agent_events.lock() {
            g.insert(event.session_id.clone(), event);
        }
    }

    fn insert(&self, provider: &str, snapshot: ConnectorSnapshot) {
        let today = Local::now().date_naive();
        let to_persist: Option<HashMap<String, DailyEntry>> = {
            let Ok(mut g) = self.providers.lock() else { return };
            let tokens = snapshot.tokens.as_ref().map(|t| t.total).unwrap_or(0);
            let prev = g.get(provider);

            let session_tokens_total = prev
                .filter(|p| p.last_snapshot.session_id == snapshot.session_id)
                .map(|p| p.session_tokens_total.saturating_add(tokens))
                .unwrap_or(tokens);

            let daily_tokens_total = match prev {
                Some(p) if p.daily_date == today => p.daily_tokens_total.saturating_add(tokens),
                _ => tokens,
            };

            g.insert(
                provider.to_string(),
                ProviderState {
                    last_snapshot: snapshot,
                    session_tokens_total,
                    daily_tokens_total,
                    daily_date: today,
                    last_seen: Instant::now(),
                },
            );

            Some(
                g.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            DailyEntry {
                                date: v.daily_date,
                                daily_tokens: v.daily_tokens_total,
                            },
                        )
                    })
                    .collect(),
            )
        };
        if let Some(snap) = to_persist {
            save_persisted(&self.persistence_path, &snap);
        }
    }
}

fn load_persisted(path: &PathBuf) -> HashMap<String, ProviderState> {
    let Ok(raw) = std::fs::read_to_string(path) else { return HashMap::new() };
    let Ok(entries): Result<HashMap<String, DailyEntry>, _> = serde_json::from_str(&raw) else {
        return HashMap::new();
    };
    let today = Local::now().date_naive();
    entries
        .into_iter()
        .filter_map(|(k, e)| {
            // Stale entries (other days) collapse to 0; today carries forward.
            let (daily_tokens_total, daily_date) = if e.date == today {
                (e.daily_tokens, e.date)
            } else {
                (0, today)
            };
            Some((
                k,
                ProviderState {
                    last_snapshot: ConnectorSnapshot {
                        schema_version: SCHEMA_VERSION,
                        provider: String::new(),
                        session_id: String::new(),
                        generated_at: Utc::now(),
                        transcript_path: None,
                        tokens: None,
                        model: None,
                    },
                    session_tokens_total: 0,
                    daily_tokens_total,
                    daily_date,
                    last_seen: Instant::now()
                        .checked_sub(std::time::Duration::from_secs(24 * 3600))
                        .unwrap_or_else(Instant::now),
                },
            ))
        })
        .collect()
}

fn save_persisted(path: &PathBuf, entries: &HashMap<String, DailyEntry>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(path, json);
    }
}

fn load_session_pids(path: &PathBuf) -> HashMap<String, u32> {
    let Ok(raw) = std::fs::read_to_string(path) else { return HashMap::new() };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_session_pids(path: &PathBuf, pids: &HashMap<String, u32>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(pids) {
        let _ = std::fs::write(path, json);
    }
}

pub async fn start_server(port: u16, store: Arc<ConnectorStore>) -> std::io::Result<()> {
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/usage/:provider", post(post_usage))
        .route("/v1/agent-office/event/:event", post(post_agent_event))
        .with_state(store);
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    axum::serve(listener, app).await
}

async fn post_agent_event(
    State(store): State<Arc<ConnectorStore>>,
    Path(event): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !matches!(
        event.as_str(),
        "session_start" | "stop" | "session_end" | "subagent_stop"
    ) {
        return Err(StatusCode::NOT_FOUND);
    }
    let session_id = pick_str(&body, &["session_id", "sessionId"]).unwrap_or_default();
    if session_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if event == "session_start" {
        if let Some(pid) = body
            .get("parentPid")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
        {
            store.record_session_pid(session_id.clone(), pid);
        }
        return Ok(Json(serde_json::json!({ "ok": true })));
    }
    let cwd = pick_str(&body, &["cwd"]);
    store.record_agent_event(AgentEvent {
        event,
        session_id,
        cwd,
        at: Utc::now(),
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "schemaVersion": SCHEMA_VERSION,
    }))
}

async fn post_usage(
    State(store): State<Arc<ConnectorStore>>,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !matches!(provider.as_str(), "claude" | "codex" | "gemini") {
        return Err(StatusCode::NOT_FOUND);
    }
    let snapshot = parse_snapshot(&provider, &body);
    store.insert(&provider, snapshot);
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn parse_snapshot(provider: &str, body: &Value) -> ConnectorSnapshot {
    let session_id = pick_str(body, &["session_id", "sessionId"]).unwrap_or_else(|| "unknown".into());
    let transcript_path = pick_str(body, &["transcript_path", "transcriptPath"]);
    let model = pick_str(body, &["model"]);
    let tokens = extract_tokens(provider, body);
    ConnectorSnapshot {
        schema_version: SCHEMA_VERSION,
        provider: provider.to_string(),
        session_id,
        generated_at: Utc::now(),
        transcript_path,
        tokens,
        model,
    }
}

fn pick_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

fn extract_tokens(provider: &str, body: &Value) -> Option<TokenStats> {
    if let Some(t) = body.get("tokens").and_then(|t| t.as_object()) {
        let total = t.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        return Some(TokenStats {
            total,
            input: t.get("input").and_then(|v| v.as_u64()),
            output: t.get("output").and_then(|v| v.as_u64()),
            cached: t.get("cached").and_then(|v| v.as_u64()),
        });
    }
    if provider == "gemini" {
        // Top-level (raw API response shape).
        if let Some(stats) = extract_gemini_usage_metadata(body) {
            return Some(stats);
        }
        // Gemini CLI AfterModel hook payload nests it under llm_response.
        if let Some(stats) = body
            .get("llm_response")
            .or_else(|| body.get("llmResponse"))
            .and_then(extract_gemini_usage_metadata)
        {
            return Some(stats);
        }
    }
    None
}

fn extract_gemini_usage_metadata(v: &Value) -> Option<TokenStats> {
    let um = v
        .get("usageMetadata")
        .or_else(|| v.get("usage_metadata"))?;
    let total = um.get("totalTokenCount").and_then(|v| v.as_u64())?;
    Some(TokenStats {
        total,
        input: um.get("promptTokenCount").and_then(|v| v.as_u64()),
        output: um.get("candidatesTokenCount").and_then(|v| v.as_u64()),
        cached: um.get("cachedContentTokenCount").and_then(|v| v.as_u64()),
    })
}
