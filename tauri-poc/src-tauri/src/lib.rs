use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, PhysicalPosition, PhysicalSize, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const DEFAULT_WIDTH: u32 = 780;
const SINGLE_PANEL_WIDTH: u32 = 410;
const HEIGHT: u32 = 320;
const DEFAULT_X: i32 = 40;
const DEFAULT_Y: i32 = 40;

// ---- State shapes (camelCase to match the renderer) ----

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct WindowSlice {
    used_percent: Option<f64>,
    reset_after_seconds: Option<i64>,
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
        }
    }
}

struct AppState {
    settings: Mutex<PublicSettings>,
    settings_path: PathBuf,
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

fn build_widget_state(settings: &PublicSettings) -> WidgetState {
    WidgetState {
        plan_type: "CODEX".into(),
        primary: WindowSlice { used_percent: Some(42.0), reset_after_seconds: Some(3 * 3600 + 12 * 60) },
        secondary: WindowSlice { used_percent: Some(67.0), reset_after_seconds: Some(4 * 24 * 3600) },
        claude: ClaudeState {
            is_configured: true,
            needs_login: false,
            primary: WindowSlice { used_percent: Some(30.0), reset_after_seconds: Some(2 * 3600 + 30 * 60) },
            secondary: WindowSlice { used_percent: Some(55.0), reset_after_seconds: Some(3 * 24 * 3600 + 12 * 3600) },
            error: None,
            is_cached: false,
        },
        session_label: "Mock session".into(),
        display_mode: settings.display_mode.clone(),
        error: None,
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
async fn get_initial_state(state: tauri::State<'_, AppState>) -> Result<WidgetState, String> {
    let s = state.settings.lock().map_err(|e| e.to_string())?.clone();
    Ok(build_widget_state(&s))
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
async fn refresh_now() -> bool { true }

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
    Ok(())
}

#[tauri::command]
async fn claude_logout() -> bool { true }

#[tauri::command]
async fn hide_widget(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
