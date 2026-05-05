mod claude;
mod codex;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, PhysicalSize, Url, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_notification::NotificationExt;

const DEFAULT_WIDTH: u32 = 780;
const SINGLE_PANEL_WIDTH: u32 = 410;
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
struct WidgetState {
    plan_type: String,
    primary: WindowSlice,
    secondary: WindowSlice,
    claude: ClaudeState,
    session_label: String,
    display_mode: String,
    error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PublicSettings {
    display_mode: String,
    show_claude: bool,
    show_codex: bool,
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
    cached_org_uuid: Option<String>,
    #[serde(default)]
    claude_session_key: Option<String>,
}

impl Default for PublicSettings {
    fn default() -> Self {
        PublicSettings {
            display_mode: "used".into(),
            show_claude: true,
            show_codex: true,
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
            cached_org_uuid: None,
            claude_session_key: None,
        }
    }
}

struct AppState {
    settings: Mutex<PublicSettings>,
    settings_path: PathBuf,
    alert_state: Mutex<UsageAlertState>,
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
    if s.show_claude && s.show_codex { DEFAULT_WIDTH } else { SINGLE_PANEL_WIDTH }
}

fn load_from_disk(path: &PathBuf) -> PublicSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PublicSettings>(&raw).ok())
        .unwrap_or_default()
}

fn save_to_disk(path: &PathBuf, settings: &PublicSettings) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

async fn build_widget_state(app: &tauri::AppHandle, settings: &PublicSettings) -> WidgetState {
    let codex_future = codex::fetch_usage(settings.fetch_timeout_ms, settings.fetch_retries);
    let claude_future = fetch_claude_with_fallback(settings);
    let (codex_result, claude_result) = futures_join(codex_future, claude_future).await;

    let (plan_type, primary, secondary, error) = match codex_result {
        Ok(u) => (u.plan_type, u.primary, u.secondary, None),
        Err(e) => (
            "CODEX".into(),
            WindowSlice::default(),
            WindowSlice::default(),
            Some(e),
        ),
    };

    let claude = match claude_result {
        Ok(u) => {
            persist_org_uuid(app, &u.org_uuid);
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
        Err(claude::ClaudeError::Other(msg)) => ClaudeState {
            is_configured: true,
            needs_login: false,
            primary: WindowSlice::default(),
            secondary: WindowSlice::default(),
            error: Some(msg),
            is_cached: false,
        },
    };

    WidgetState {
        plan_type,
        primary,
        secondary,
        claude,
        session_label: "Mock session".into(),
        display_mode: settings.display_mode.clone(),
        error,
    }
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
    let _ = app.emit("usage:update", state);
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
    match claude::fetch_usage(settings.fetch_timeout_ms, settings.cached_org_uuid.clone()).await {
        Ok(u) => Ok(u),
        Err(claude::ClaudeError::SessionExpired) => match settings.claude_session_key.as_ref() {
            Some(sk) if !sk.is_empty() => {
                claude::fetch_usage_with_cookie(
                    settings.fetch_timeout_ms,
                    sk,
                    settings.cached_org_uuid.clone(),
                )
                .await
            }
            _ => Err(claude::ClaudeError::SessionExpired),
        },
        Err(other) => Err(other),
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

fn merge_partial(current: &PublicSettings, partial: &serde_json::Value) -> PublicSettings {
    let mut next = current.clone();
    if let Some(o) = partial.as_object() {
        if let Some(v) = o.get("displayMode").and_then(|x| x.as_str()) { next.display_mode = v.to_string(); }
        if let Some(v) = o.get("showClaude").and_then(|x| x.as_bool()) { next.show_claude = v; }
        if let Some(v) = o.get("showCodex").and_then(|x| x.as_bool()) { next.show_codex = v; }
        if let Some(v) = o.get("enableUsageAlerts").and_then(|x| x.as_bool()) { next.enable_usage_alerts = v; }
        if let Some(v) = o.get("refreshIntervalMs").and_then(|x| x.as_u64()) { next.refresh_interval_ms = v; }
        if let Some(v) = o.get("openOnStartup").and_then(|x| x.as_bool()) { next.open_on_startup = v; }
        if let Some(arr) = o.get("usageAlertThresholds").and_then(|x| x.as_array()) {
            next.usage_alert_thresholds = arr.iter().filter_map(|n| n.as_u64().map(|x| x as u32)).collect();
        }
    }
    if !next.show_claude && !next.show_codex { next.show_claude = true; }
    next
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
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_always_on_top(next.always_on_top);
        let target = target_width(&next);
        if let Ok(size) = win.inner_size() {
            if size.width != target {
                let _ = win.set_size(PhysicalSize { width: target, height: HEIGHT });
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

        if let Some(state) = app.try_state::<AppState>() {
            let snapshot = {
                let mut guard = match state.settings.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                guard.claude_session_key = Some(session_key);
                guard.clone()
            };
            save_to_disk(&state.settings_path, &snapshot);
        }

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
async fn claude_logout(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let snapshot = {
        let mut guard = state.settings.lock().map_err(|e| e.to_string())?;
        guard.claude_session_key = None;
        guard.cached_org_uuid = None;
        guard.clone()
    };
    save_to_disk(&state.settings_path, &snapshot);
    Ok(true)
}

#[tauri::command]
async fn hide_widget(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show widget", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "Hide widget", true, None::<&str>)?;
    let toggle_top = MenuItem::with_id(app, "toggle_top", "Toggle always-on-top", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &hide_item, &toggle_top, &refresh_item, &quit_item])?;

    TrayIconBuilder::with_id("main-tray")
        .tooltip("AI Usage Widget")
        .icon(app.default_window_icon().cloned().unwrap())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "toggle_top" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let next = {
                        let mut guard = match state.settings.lock() {
                            Ok(g) => g,
                            Err(_) => return,
                        };
                        guard.always_on_top = !guard.always_on_top;
                        guard.clone()
                    };
                    save_to_disk(&state.settings_path, &next);
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.set_always_on_top(next.always_on_top);
                    }
                }
            }
            "refresh" => {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let settings = match app_handle
                        .try_state::<AppState>()
                        .and_then(|s| s.settings.lock().ok().map(|g| g.clone()))
                    {
                        Some(s) => s,
                        None => return,
                    };
                    let state = build_widget_state(&app_handle, &settings).await;
                    emit_usage_update(&app_handle, &settings, state);
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let settings_path = app
                .path()
                .app_config_dir()
                .expect("app_config_dir")
                .join("settings.json");
            let loaded = load_from_disk(&settings_path);

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_position(PhysicalPosition { x: loaded.x, y: loaded.y });
                let _ = win.set_size(PhysicalSize { width: target_width(&loaded), height: HEIGHT });
                let _ = win.set_always_on_top(loaded.always_on_top);
            }

            app.manage(AppState {
                settings: Mutex::new(loaded),
                settings_path,
                alert_state: Mutex::new(UsageAlertState::default()),
            });

            setup_tray(&app.handle().clone())?;

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
            if let WindowEvent::Moved(pos) = event {
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
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            get_settings,
            update_settings,
            set_display_mode,
            refresh_now,
            claude_login,
            claude_logout,
            hide_widget,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
