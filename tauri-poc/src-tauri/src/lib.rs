mod agent_office;
mod agent_scan;
mod focus_session;
mod claude;
mod codex;
mod connector;
mod connector_install;
mod gemini;
mod secure;
mod session;
mod widget_core;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LAST_GOOD_TTL: Duration = Duration::from_secs(5 * 60);
const CONNECTOR_PORT: u16 = 8766;
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize, Url, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

const DEFAULT_WIDTH: u32 = 780;
const SINGLE_PANEL_WIDTH: u32 = 410;
const TRIPLE_PANEL_WIDTH: u32 = 1150;
const HEIGHT: u32 = 320;
const DEFAULT_X: i32 = 40;
const DEFAULT_Y: i32 = 40;

// ---- State shapes (camelCase to match the renderer) ----

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WindowSlice {
    pub used_percent: Option<f64>,
    pub reset_after_seconds: Option<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ClaudeState {
    is_configured: bool,
    needs_login: bool,
    primary: WindowSlice,
    secondary: WindowSlice,
    error: Option<String>,
    is_cached: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CodexState {
    is_configured: bool,
    needs_login: bool,
    is_cached: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WidgetState {
    plan_type: String,
    primary: WindowSlice,
    secondary: WindowSlice,
    codex: CodexState,
    claude: ClaudeState,
    session_label: String,
    display_mode: String,
    error: Option<String>,
    agent_presence: agent_scan::AgentPresence,
    gemini: GeminiState,
    connector: ConnectorSummary,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
struct GeminiState {
    is_configured: bool,
    connector_active: bool,
    daily_tokens: Option<u64>,
    last_active_seconds_ago: Option<u64>,
    model: Option<String>,
    cloud_available: bool,
    plan_type: Option<String>,
    quotas: Vec<gemini::GeminiQuotaEntry>,
    cloud_error: Option<String>,
    needs_login: bool,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
struct ConnectorSummary {
    server_running: bool,
    port: u16,
    providers: Vec<connector_install::ProviderInstallStatus>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PublicSettings {
    display_mode: String,
    show_claude: bool,
    show_codex: bool,
    #[serde(default = "default_show_gemini")]
    show_gemini: bool,
    #[serde(default = "default_ui_scale")]
    ui_scale: f32,
    enable_usage_alerts: bool,
    usage_alert_thresholds: Vec<u32>,
    refresh_interval_ms: u64,
    open_on_startup: bool,
    always_on_top: bool,
    fetch_timeout_ms: u64,
    fetch_retries: u32,
    session_scan_ttl_ms: u64,
    x: i32,
    y: i32,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    cached_org_uuid: Option<String>,
    #[serde(default)]
    claude_session_key: Option<String>,
    #[serde(default)]
    consent_accepted: bool,
    #[serde(default = "default_language")]
    language: String,
}

fn default_language() -> String {
    "en".into()
}

fn default_show_gemini() -> bool {
    false
}

fn default_ui_scale() -> f32 {
    1.0
}

impl Default for PublicSettings {
    fn default() -> Self {
        PublicSettings {
            display_mode: "used".into(),
            show_claude: true,
            show_codex: true,
            show_gemini: false,
            ui_scale: 1.0,
            enable_usage_alerts: true,
            usage_alert_thresholds: vec![30, 60, 80, 90],
            refresh_interval_ms: 60_000,
            open_on_startup: true,
            always_on_top: true,
            fetch_timeout_ms: 8_000,
            fetch_retries: 2,
            session_scan_ttl_ms: 5 * 60 * 1000,
            x: DEFAULT_X,
            y: DEFAULT_Y,
            width: None,
            height: None,
            cached_org_uuid: None,
            claude_session_key: None,
            consent_accepted: false,
            language: default_language(),
        }
    }
}

struct AppState {
    settings: Mutex<PublicSettings>,
    settings_path: PathBuf,
    alert_state: Mutex<UsageAlertState>,
    session_cache: Mutex<session::SessionCache>,
    last_codex_good: Mutex<Option<CachedCodex>>,
    last_claude_good: Mutex<Option<CachedClaude>>,
    connector_store: Arc<connector::ConnectorStore>,
}

#[derive(Clone)]
struct CachedCodex {
    fetched_at: Instant,
    plan_type: String,
    primary: WindowSlice,
    secondary: WindowSlice,
}

#[derive(Clone)]
struct CachedClaude {
    fetched_at: Instant,
    primary: WindowSlice,
    secondary: WindowSlice,
    #[allow(dead_code)]
    org_uuid: String,
}

#[derive(Default)]
struct UsageAlertState {
    codex_primary: WindowAlertSlot,
    codex_secondary: WindowAlertSlot,
    claude_primary: WindowAlertSlot,
    claude_secondary: WindowAlertSlot,
}

#[derive(Default)]
struct WindowAlertSlot {
    last_value: Option<f64>,
    notified: std::collections::BTreeSet<u32>,
}

fn target_width(s: &PublicSettings) -> u32 {
    let n = (s.show_claude as u32) + (s.show_codex as u32) + (s.show_gemini as u32);
    match n {
        3 => TRIPLE_PANEL_WIDTH,
        2 => DEFAULT_WIDTH,
        _ => SINGLE_PANEL_WIDTH,
    }
}

fn load_from_disk(path: &PathBuf) -> PublicSettings {
    let mut loaded: PublicSettings = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PublicSettings>(&raw).ok())
        .unwrap_or_default();
    // Decrypt at-rest sessionKey if it was protected on a previous save.
    if let Some(sk) = loaded.claude_session_key.as_deref() {
        if secure::is_protected(sk) {
            loaded.claude_session_key = secure::unprotect(sk);
        }
    }
    sanitize(loaded)
}

fn save_to_disk(path: &PathBuf, settings: &PublicSettings) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Encrypt sessionKey at rest. In-memory copy stays plaintext for the running session.
    let mut to_persist = settings.clone();
    if let Some(sk) = to_persist.claude_session_key.as_deref() {
        if !sk.is_empty() && !secure::is_protected(sk) {
            if let Some(enc) = secure::protect(sk) {
                to_persist.claude_session_key = Some(enc);
            }
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(&to_persist) {
        let _ = std::fs::write(path, json);
    }
}

async fn build_widget_state(app: &tauri::AppHandle, settings: &PublicSettings) -> WidgetState {
    if !settings.consent_accepted {
        return WidgetState {
            plan_type: "CODEX".into(),
            primary: WindowSlice::default(),
            secondary: WindowSlice::default(),
            codex: CodexState {
                is_configured: false,
                needs_login: false,
                is_cached: false,
            },
            claude: ClaudeState {
                is_configured: false,
                needs_login: false,
                primary: WindowSlice::default(),
                secondary: WindowSlice::default(),
                error: None,
                is_cached: false,
            },
            session_label: String::new(),
            display_mode: settings.display_mode.clone(),
            error: None,
            agent_presence: agent_scan::AgentPresence::default(),
            gemini: GeminiState {
                cloud_available: false,
                ..GeminiState::default()
            },
            connector: ConnectorSummary { server_running: true, port: CONNECTOR_PORT, providers: Vec::new() },
        };
    }
    let codex_future = codex::fetch_usage(settings.fetch_timeout_ms, settings.fetch_retries);
    let claude_future = fetch_claude_with_fallback(settings);
    let gemini_future = gemini::fetch_quota(settings.fetch_timeout_ms, settings.fetch_retries);
    let (codex_result, claude_result, gemini_result) =
        tokio::join!(codex_future, claude_future, gemini_future);

    let (plan_type, primary, secondary, codex, error) = match codex_result {
        Ok(u) => {
            store_codex_cache(app, &u);
            let plan = u.plan_type.clone();
            let p = u.primary.clone();
            let s = u.secondary.clone();
            (
                plan,
                p,
                s,
                CodexState {
                    is_configured: true,
                    needs_login: false,
                    is_cached: false,
                },
                None,
            )
        }
        Err(codex::CodexError::NotConfigured) => (
            "CODEX".into(),
            WindowSlice::default(),
            WindowSlice::default(),
            CodexState {
                is_configured: false,
                needs_login: false,
                is_cached: false,
            },
            None,
        ),
        Err(codex::CodexError::SessionExpired) => (
            "CODEX".into(),
            WindowSlice::default(),
            WindowSlice::default(),
            CodexState {
                is_configured: true,
                needs_login: true,
                is_cached: false,
            },
            Some(codex::CodexError::SessionExpired.to_string()),
        ),
        Err(codex::CodexError::Other(msg)) => match read_codex_cache(app) {
            Some(cached) => (
                cached.plan_type,
                cached.primary,
                cached.secondary,
                CodexState {
                    is_configured: true,
                    needs_login: false,
                    is_cached: true,
                },
                Some(msg),
            ),
            None => (
                "CODEX".into(),
                WindowSlice::default(),
                WindowSlice::default(),
                CodexState {
                    is_configured: true,
                    needs_login: false,
                    is_cached: false,
                },
                Some(msg),
            ),
        },
    };

    let claude = match claude_result {
        Ok(u) => {
            persist_org_uuid(app, &u.org_uuid);
            store_claude_cache(app, &u);
            ClaudeState {
                is_configured: true,
                needs_login: false,
                primary: u.primary,
                secondary: u.secondary,
                error: None,
                is_cached: false,
            }
        }
        Err(claude::ClaudeError::NotConfigured) => ClaudeState {
            is_configured: false,
            needs_login: false,
            primary: WindowSlice::default(),
            secondary: WindowSlice::default(),
            error: None,
            is_cached: false,
        },
        Err(claude::ClaudeError::SessionExpired) => ClaudeState {
            is_configured: true,
            needs_login: true,
            primary: WindowSlice::default(),
            secondary: WindowSlice::default(),
            error: Some("Claude session expired. Please log in again.".into()),
            is_cached: false,
        },
        Err(claude::ClaudeError::Other(msg)) => match read_claude_cache(app) {
            Some(cached) => ClaudeState {
                is_configured: true,
                needs_login: false,
                primary: cached.primary,
                secondary: cached.secondary,
                error: Some(msg),
                is_cached: true,
            },
            None => ClaudeState {
                is_configured: true,
                needs_login: false,
                primary: WindowSlice::default(),
                secondary: WindowSlice::default(),
                error: Some(msg),
                is_cached: false,
            },
        },
    };

    WidgetState {
        plan_type,
        primary,
        secondary,
        codex,
        claude,
        session_label: load_session_label(app, settings),
        display_mode: settings.display_mode.clone(),
        error,
        agent_presence: agent_scan::scan(),
        gemini: build_gemini_state(app, gemini_result),
        connector: build_connector_summary(app),
    }
}

fn build_gemini_state(
    app: &tauri::AppHandle,
    cloud_result: Result<gemini::GeminiQuotaResponse, gemini::GeminiError>,
) -> GeminiState {
    let installed = connector_install::status_gemini(app).installed;
    let snapshot = app
        .try_state::<AppState>()
        .and_then(|s| s.connector_store.get("gemini"));
    let active_ttl = Duration::from_secs(5 * 60);

    let connector_active = snapshot
        .as_ref()
        .is_some_and(|p| p.last_seen.elapsed() <= active_ttl);
    let (daily_tokens, last_active_seconds_ago, model) = match snapshot {
        Some(p) if p.last_seen.elapsed() <= active_ttl => (
            Some(p.daily_tokens_total),
            Some(p.last_seen.elapsed().as_secs()),
            p.last_snapshot.model.clone(),
        ),
        Some(p) => (
            (p.daily_tokens_total > 0).then_some(p.daily_tokens_total),
            None,
            None,
        ),
        None => (None, None, None),
    };

    let (cloud_available, plan_type, quotas, cloud_error, needs_login) = match cloud_result {
        Ok(resp) => (true, resp.plan_type, resp.quotas, None, false),
        Err(gemini::GeminiError::NotConfigured) => (false, None, Vec::new(), None, false),
        Err(gemini::GeminiError::SessionExpired) => (
            false,
            None,
            Vec::new(),
            Some("Gemini session expired — run `gemini` CLI to refresh".into()),
            true,
        ),
        Err(gemini::GeminiError::Other(msg)) => (false, None, Vec::new(), Some(msg), false),
    };

    GeminiState {
        is_configured: installed || cloud_available,
        connector_active,
        daily_tokens,
        last_active_seconds_ago,
        model,
        cloud_available,
        plan_type,
        quotas,
        cloud_error,
        needs_login,
    }
}

fn build_connector_summary(app: &tauri::AppHandle) -> ConnectorSummary {
    ConnectorSummary {
        server_running: true,
        port: CONNECTOR_PORT,
        providers: vec![
            connector_install::status_gemini(app),
            connector_install::status_claude(app),
            connector_install::status_stub("codex"),
        ],
    }
}

fn sync_autostart(app: &tauri::AppHandle, want_enabled: bool) {
    let manager = app.autolaunch();
    let currently = manager.is_enabled().unwrap_or(false);
    let _ = match (want_enabled, currently) {
        (true, false) => manager.enable(),
        (false, true) => manager.disable(),
        _ => Ok(()),
    };
}

fn store_codex_cache(app: &tauri::AppHandle, usage: &codex::CodexUsage) {
    let Some(state) = app.try_state::<AppState>() else { return; };
    if let Ok(mut guard) = state.last_codex_good.lock() {
        *guard = Some(CachedCodex {
            fetched_at: Instant::now(),
            plan_type: usage.plan_type.clone(),
            primary: usage.primary.clone(),
            secondary: usage.secondary.clone(),
        });
    };
}

fn read_codex_cache(app: &tauri::AppHandle) -> Option<CachedCodex> {
    let state = app.try_state::<AppState>()?;
    let guard = state.last_codex_good.lock().ok()?;
    let cached = guard.as_ref()?;
    if cached.fetched_at.elapsed() <= LAST_GOOD_TTL {
        Some(cached.clone())
    } else {
        None
    }
}

fn store_claude_cache(app: &tauri::AppHandle, usage: &claude::ClaudeUsage) {
    let Some(state) = app.try_state::<AppState>() else { return; };
    if let Ok(mut guard) = state.last_claude_good.lock() {
        *guard = Some(CachedClaude {
            fetched_at: Instant::now(),
            primary: usage.primary.clone(),
            secondary: usage.secondary.clone(),
            org_uuid: usage.org_uuid.clone(),
        });
    };
}

fn read_claude_cache(app: &tauri::AppHandle) -> Option<CachedClaude> {
    let state = app.try_state::<AppState>()?;
    let mut guard = state.last_claude_good.lock().ok()?;

    let rolled_over = guard.as_ref().is_some_and(|cached| {
        matches!(cached.primary.reset_after_seconds, Some(reset)
            if reset >= 0 && cached.fetched_at.elapsed().as_secs() as i64 >= reset)
    });
    if rolled_over {
        *guard = None;
        return None;
    }

    let cached = guard.as_ref()?;
    if cached.fetched_at.elapsed() <= LAST_GOOD_TTL {
        Some(cached.clone())
    } else {
        None
    }
}

fn load_session_label(app: &tauri::AppHandle, settings: &PublicSettings) -> String {
    let Some(state) = app.try_state::<AppState>() else {
        return "Recent session".into();
    };
    let Ok(mut cache) = state.session_cache.lock() else {
        return "Recent session".into();
    };
    session::load_label(&mut *cache, settings.session_scan_ttl_ms)
}

async fn futures_join<A, B, T1, T2>(a: A, b: B) -> (T1, T2)
where
    A: std::future::Future<Output = T1>,
    B: std::future::Future<Output = T2>,
{
    tokio::join!(a, b)
}

fn check_thresholds(
    slot: &mut WindowAlertSlot,
    used_percent: Option<f64>,
    thresholds: &[u32],
) -> Vec<u32> {
    let Some(value) = used_percent else { return Vec::new(); };
    if let Some(prev) = slot.last_value {
        if prev - value >= 10.0 {
            slot.notified.clear();
        }
    }
    slot.last_value = Some(value);
    let mut crossed = Vec::new();
    for &t in thresholds {
        if value >= f64::from(t) && !slot.notified.contains(&t) {
            slot.notified.insert(t);
            crossed.push(t);
        }
    }
    crossed
}

fn emit_usage_update(app: &tauri::AppHandle, settings: &PublicSettings, state: WidgetState) {
    dispatch_alerts(app, settings, &state);
    let _ = app.emit("widget-state", state);
}

fn dispatch_alerts(app: &tauri::AppHandle, settings: &PublicSettings, state: &WidgetState) {
    if !settings.enable_usage_alerts {
        return;
    }
    let Some(app_state) = app.try_state::<AppState>() else { return; };
    let mut alerts: Vec<(String, String)> = Vec::new();
    {
        let Ok(mut guard) = app_state.alert_state.lock() else { return; };
        let thresholds = &settings.usage_alert_thresholds;
        for t in check_thresholds(&mut guard.codex_primary, state.primary.used_percent, thresholds) {
            alerts.push((
                format!("{} 5-HOUR usage at {t}%", state.plan_type),
                format!("Codex usage hit {t}%"),
            ));
        }
        for t in check_thresholds(&mut guard.codex_secondary, state.secondary.used_percent, thresholds) {
            alerts.push((
                format!("{} WEEKLY usage at {t}%", state.plan_type),
                format!("Codex weekly usage hit {t}%"),
            ));
        }
        for t in check_thresholds(&mut guard.claude_primary, state.claude.primary.used_percent, thresholds) {
            alerts.push((
                format!("CLAUDE 5-HOUR usage at {t}%"),
                format!("Claude usage hit {t}%"),
            ));
        }
        for t in check_thresholds(&mut guard.claude_secondary, state.claude.secondary.used_percent, thresholds) {
            alerts.push((
                format!("CLAUDE WEEKLY usage at {t}%"),
                format!("Claude weekly usage hit {t}%"),
            ));
        }
    }
    for (title, body) in alerts {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

async fn fetch_claude_with_fallback(
    settings: &PublicSettings,
) -> Result<claude::ClaudeUsage, claude::ClaudeError> {
    let bearer = claude::fetch_usage(
        settings.fetch_timeout_ms,
        settings.fetch_retries,
        settings.cached_org_uuid.clone(),
    )
    .await;
    if let Ok(u) = bearer {
        return Ok(u);
    }
    match settings.claude_session_key.as_ref() {
        Some(sk) if !sk.is_empty() => {
            claude::fetch_usage_with_cookie(
                settings.fetch_timeout_ms,
                settings.fetch_retries,
                sk,
                settings.cached_org_uuid.clone(),
            )
            .await
        }
        _ => bearer,
    }
}

fn persist_org_uuid(app: &tauri::AppHandle, uuid: &str) {
    let Some(state) = app.try_state::<AppState>() else { return; };
    let mut should_save = false;
    let snapshot = {
        let mut guard = match state.settings.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.cached_org_uuid.as_deref() != Some(uuid) {
            guard.cached_org_uuid = Some(uuid.to_string());
            should_save = true;
        }
        guard.clone()
    };
    if should_save {
        save_to_disk(&state.settings_path, &snapshot);
    }
}

fn clamp_u64(v: u64, min: u64, max: u64) -> u64 {
    v.clamp(min, max)
}

use widget_core::{normalize_display_mode, sanitize_thresholds_with_default};

fn sanitize(mut s: PublicSettings) -> PublicSettings {
    s.refresh_interval_ms = clamp_u64(s.refresh_interval_ms, 10_000, 10 * 60 * 1000);
    s.fetch_timeout_ms = clamp_u64(s.fetch_timeout_ms, 2_000, 60_000);
    s.fetch_retries = s.fetch_retries.min(5);
    s.session_scan_ttl_ms = clamp_u64(s.session_scan_ttl_ms, 30_000, 60 * 60 * 1000);
    s.display_mode = normalize_display_mode(&s.display_mode);
    s.usage_alert_thresholds = sanitize_thresholds_with_default(s.usage_alert_thresholds);
    if !s.ui_scale.is_finite() {
        s.ui_scale = 1.0;
    }
    s.ui_scale = s.ui_scale.clamp(0.7, 1.5);
    s.language = match s.language.as_str() {
        "ko" => "ko".into(),
        _ => "en".into(),
    };
    if !s.show_claude && !s.show_codex && !s.show_gemini {
        s.show_claude = true;
    }
    s
}

fn merge_partial(current: &PublicSettings, partial: &serde_json::Value) -> PublicSettings {
    let mut next = current.clone();
    if let Some(o) = partial.as_object() {
        if let Some(v) = o.get("displayMode").and_then(|x| x.as_str()) { next.display_mode = v.to_string(); }
        if let Some(v) = o.get("showClaude").and_then(|x| x.as_bool()) { next.show_claude = v; }
        if let Some(v) = o.get("showCodex").and_then(|x| x.as_bool()) { next.show_codex = v; }
        if let Some(v) = o.get("showGemini").and_then(|x| x.as_bool()) { next.show_gemini = v; }
        if let Some(v) = o.get("uiScale").and_then(|x| x.as_f64()) { next.ui_scale = v as f32; }
        if let Some(v) = o.get("enableUsageAlerts").and_then(|x| x.as_bool()) { next.enable_usage_alerts = v; }
        if let Some(v) = o.get("refreshIntervalMs").and_then(|x| x.as_u64()) { next.refresh_interval_ms = v; }
        if let Some(v) = o.get("openOnStartup").and_then(|x| x.as_bool()) { next.open_on_startup = v; }
        if let Some(v) = o.get("language").and_then(|x| x.as_str()) { next.language = v.to_string(); }
        if let Some(arr) = o.get("usageAlertThresholds").and_then(|x| x.as_array()) {
            next.usage_alert_thresholds = arr.iter().filter_map(|n| n.as_u64().map(|x| x as u32)).collect();
        }
    }
    sanitize(next)
}

// ---- Commands ----

#[tauri::command]
async fn get_initial_state(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<WidgetState, String> {
    let s = state.settings.lock().map_err(|e| e.to_string())?.clone();
    Ok(build_widget_state(&app, &s).await)
}

#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> Result<PublicSettings, String> {
    state.settings.lock().map(|s| s.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    partial: serde_json::Value,
) -> Result<PublicSettings, String> {
    let next = {
        let mut guard = state.settings.lock().map_err(|e| e.to_string())?;
        let next = merge_partial(&guard, &partial);
        *guard = next.clone();
        next
    };
    save_to_disk(&state.settings_path, &next);
    sync_autostart(&app, next.open_on_startup);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_always_on_top(next.always_on_top);
        let target = target_width(&next);
        if let Ok(size) = win.inner_size() {
            if size.width != target {
                let _ = win.set_size(PhysicalSize { width: target, height: size.height });
            }
        }
    }
    Ok(next)
}

#[tauri::command]
async fn set_display_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<serde_json::Value, String> {
    {
        let mut guard = state.settings.lock().map_err(|e| e.to_string())?;
        guard.display_mode = mode.clone();
    }
    let snapshot = state.settings.lock().map(|s| s.clone()).map_err(|e| e.to_string())?;
    save_to_disk(&state.settings_path, &snapshot);
    Ok(serde_json::json!({ "displayMode": mode }))
}

#[tauri::command]
async fn refresh_now(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let widget_state = build_widget_state(&app, &settings).await;
    emit_usage_update(&app, &settings, widget_state);
    Ok(true)
}

#[tauri::command]
async fn accept_consent(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    show_claude: bool,
    show_codex: bool,
) -> Result<PublicSettings, String> {
    let snapshot = {
        let mut guard = state.settings.lock().map_err(|e| e.to_string())?;
        guard.consent_accepted = true;
        guard.show_claude = show_claude;
        guard.show_codex = show_codex;
        if !guard.show_claude && !guard.show_codex {
            guard.show_claude = true;
        }
        guard.clone()
    };
    save_to_disk(&state.settings_path, &snapshot);
    if let Some(win) = app.get_webview_window("main") {
        let target = target_width(&snapshot);
        if let Ok(size) = win.inner_size() {
            if size.width != target {
                let _ = win.set_size(PhysicalSize { width: target, height: size.height });
            }
        }
    }
    let widget_state = build_widget_state(&app, &snapshot).await;
    emit_usage_update(&app, &snapshot, widget_state);
    Ok(snapshot)
}

#[tauri::command]
async fn claude_login(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("claude_login") {
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let url: Url = "https://claude.ai/login"
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    WebviewWindowBuilder::new(&app, "claude_login", WebviewUrl::External(url))
        .title("Claude Login")
        .inner_size(900.0, 720.0)
        .build()
        .map_err(|e| e.to_string())?;

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        poll_claude_session_cookie(app_handle).await;
    });
    Ok(())
}

async fn poll_claude_session_cookie(app: tauri::AppHandle) {
    let claude_url: Url = match "https://claude.ai".parse() {
        Ok(u) => u,
        Err(_) => return,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    loop {
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(window) = app.get_webview_window("claude_login") else {
            break;
        };
        let Ok(cookies) = window.cookies_for_url(claude_url.clone()) else {
            continue;
        };
        let Some(session_key) = cookies
            .iter()
            .find(|c| c.name() == "sessionKey")
            .map(|c| c.value().to_string())
        else {
            continue;
        };
        if session_key.is_empty() {
            continue;
        }

        let resolved_uuid = claude::resolve_org_uuid_with_cookie(15_000, &session_key)
            .await
            .ok();

        if let Some(state) = app.try_state::<AppState>() {
            let snapshot = {
                let mut guard = match state.settings.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                guard.claude_session_key = Some(session_key);
                if let Some(uuid) = resolved_uuid {
                    guard.cached_org_uuid = Some(uuid);
                }
                guard.clone()
            };
            save_to_disk(&state.settings_path, &snapshot);
        };

        let _ = window.close();
        if let Some(state) = app.try_state::<AppState>() {
            let settings = state.settings.lock().ok().map(|g| g.clone());
            if let Some(s) = settings {
                let widget_state = build_widget_state(&app, &s).await;
                emit_usage_update(&app, &s, widget_state);
            }
        }
        break;
    }
}

#[tauri::command]
async fn claude_logout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let snapshot = {
        let mut guard = state.settings.lock().map_err(|e| e.to_string())?;
        guard.claude_session_key = None;
        guard.cached_org_uuid = None;
        guard.clone()
    };
    save_to_disk(&state.settings_path, &snapshot);
    if let Ok(mut guard) = state.last_claude_good.lock() {
        *guard = None;
    }
    let widget_state = build_widget_state(&app, &snapshot).await;
    emit_usage_update(&app, &snapshot, widget_state);
    Ok(true)
}

#[tauri::command]
async fn hide_widget(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn get_agent_office_state(
    state: tauri::State<'_, AppState>,
) -> Result<agent_office::AgentOfficeState, String> {
    let store = state.connector_store.clone();
    tokio::task::spawn_blocking(move || agent_office::scan(Some(store)))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn focus_session_window(cwd: String) -> Result<bool, String> {
    let cwd_clone = cwd.clone();
    tokio::task::spawn_blocking(move || focus_session::focus_for_cwd(&cwd_clone))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn close_agent_office(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("agent_office") {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn open_agent_office(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("agent_office") {
        existing.show().map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        &app,
        "agent_office",
        WebviewUrl::App("agent-office.html".into()),
    )
    .title("Agent Office")
    .inner_size(1280.0, 720.0)
    .min_inner_size(800.0, 480.0)
    .decorations(false)
    .transparent(true)
    .resizable(true)
    .shadow(false)
    .skip_taskbar(false)
    .always_on_top(false)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn connector_status(app: tauri::AppHandle) -> ConnectorSummary {
    build_connector_summary(&app)
}

#[tauri::command]
async fn install_connector(
    app: tauri::AppHandle,
    provider: String,
) -> Result<ConnectorSummary, String> {
    match provider.as_str() {
        "gemini" => {
            connector_install::install_gemini(&app)?;
        }
        "claude" => {
            connector_install::install_claude(&app)?;
        }
        _ => return Err(format!("install for '{provider}' not yet supported")),
    }
    let summary = build_connector_summary(&app);
    spawn_widget_refresh(app.clone());
    Ok(summary)
}

fn spawn_widget_refresh(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else { return };
        let Some(settings) = state.settings.lock().ok().map(|g| g.clone()) else { return };
        let widget_state = build_widget_state(&app, &settings).await;
        emit_usage_update(&app, &settings, widget_state);
    });
}

#[tauri::command]
async fn uninstall_connector(
    app: tauri::AppHandle,
    provider: String,
) -> Result<ConnectorSummary, String> {
    match provider.as_str() {
        "gemini" => {
            connector_install::uninstall_gemini(&app)?;
        }
        "claude" => {
            connector_install::uninstall_claude(&app)?;
        }
        _ => return Err(format!("uninstall for '{provider}' not yet supported")),
    }
    let summary = build_connector_summary(&app);
    spawn_widget_refresh(app.clone());
    Ok(summary)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let settings_path = app
                .path()
                .app_config_dir()
                .expect("app_config_dir")
                .join("settings.json");
            let loaded = load_from_disk(&settings_path);

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_position(PhysicalPosition { x: loaded.x, y: loaded.y });
                let width = loaded.width.unwrap_or_else(|| target_width(&loaded));
                let height = loaded.height.unwrap_or(HEIGHT);
                let _ = win.set_size(PhysicalSize { width, height });
                let _ = win.set_always_on_top(loaded.always_on_top);
            }

            sync_autostart(app.handle(), loaded.open_on_startup);

            let connector_daily_path = settings_path
                .parent()
                .map(|p| p.join("connector_daily.json"))
                .unwrap_or_else(|| PathBuf::from("connector_daily.json"));
            let connector_store = Arc::new(connector::ConnectorStore::new(connector_daily_path));
            app.manage(AppState {
                settings: Mutex::new(loaded),
                settings_path,
                alert_state: Mutex::new(UsageAlertState::default()),
                session_cache: Mutex::new(session::SessionCache::default()),
                last_codex_good: Mutex::new(None),
                last_claude_good: Mutex::new(None),
                connector_store: connector_store.clone(),
            });

            tauri::async_runtime::spawn(async move {
                if let Err(e) = connector::start_server(CONNECTOR_PORT, connector_store).await {
                    eprintln!("connector server failed to start on 127.0.0.1:{CONNECTOR_PORT}: {e}");
                }
            });

            let office_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(3000)).await;
                    let Some(window) = office_handle.get_webview_window("agent_office") else {
                        continue;
                    };
                    if !window.is_visible().unwrap_or(false) {
                        continue;
                    }
                    let store = office_handle
                        .try_state::<AppState>()
                        .map(|st| st.connector_store.clone());
                    let state = tokio::task::spawn_blocking(move || agent_office::scan(store))
                        .await
                        .ok();
                    if let Some(s) = state {
                        let _ = window.emit("agent-office:update", s);
                    }
                }
            });

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let snapshot = app_handle
                        .try_state::<AppState>()
                        .and_then(|s| s.settings.lock().ok().map(|g| g.clone()));
                    let interval_ms = snapshot
                        .as_ref()
                        .map(|s| s.refresh_interval_ms)
                        .unwrap_or(60_000);
                    tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                    let settings = match app_handle
                        .try_state::<AppState>()
                        .and_then(|s| s.settings.lock().ok().map(|g| g.clone()))
                    {
                        Some(s) => s,
                        None => continue,
                    };
                    let state = build_widget_state(&app_handle, &settings).await;
                    emit_usage_update(&app_handle, &settings, state);
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" { return; }
            match event {
                WindowEvent::Moved(pos) => {
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        let mut snapshot = None;
                        if let Ok(mut guard) = state.settings.lock() {
                            guard.x = pos.x;
                            guard.y = pos.y;
                            snapshot = Some(guard.clone());
                        }
                        if let Some(s) = snapshot {
                            save_to_disk(&state.settings_path, &s);
                        }
                    }
                }
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 { return; }
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        let mut snapshot = None;
                        if let Ok(mut guard) = state.settings.lock() {
                            guard.width = Some(size.width);
                            guard.height = Some(size.height);
                            snapshot = Some(guard.clone());
                        }
                        if let Some(s) = snapshot {
                            save_to_disk(&state.settings_path, &s);
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            get_settings,
            update_settings,
            set_display_mode,
            refresh_now,
            accept_consent,
            claude_login,
            claude_logout,
            hide_widget,
            get_agent_office_state,
            open_agent_office,
            close_agent_office,
            focus_session_window,
            connector_status,
            install_connector,
            uninstall_connector,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
