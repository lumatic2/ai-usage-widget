use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

pub const CONNECTOR_VERSION: &str = "1";
pub const HOOK_NAME: &str = "ai-usage-widget-connector";

const HOOK_SCRIPT: &str = r#"# ai-usage-widget connector v1
param([Parameter(Mandatory)][string]$Provider)
$body = [Console]::In.ReadToEnd()
try {
  Invoke-WebRequest -Uri "http://127.0.0.1:8766/v1/usage/$Provider" `
    -Method POST -ContentType "application/json" -Body $body `
    -TimeoutSec 2 -UseBasicParsing | Out-Null
} catch { }
exit 0
"#;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstallStatus {
    pub provider: String,
    pub installed: bool,
    pub settings_path: String,
    pub connector_version: Option<String>,
    pub error: Option<String>,
}

fn gemini_settings_path() -> Result<PathBuf, String> {
    let home = std::env::var("USERPROFILE").map_err(|e| e.to_string())?;
    Ok(PathBuf::from(home).join(".gemini").join("settings.json"))
}

fn hook_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    Ok(dir.join("connector").join("post.ps1"))
}

fn ensure_hook_script(app: &AppHandle) -> Result<PathBuf, String> {
    let path = hook_script_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let need_write = match fs::read_to_string(&path) {
        Ok(existing) => existing != HOOK_SCRIPT,
        Err(_) => true,
    };
    if need_write {
        fs::write(&path, HOOK_SCRIPT).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

fn build_hook_command(script_path: &PathBuf, provider: &str) -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" {}",
        script_path.display(),
        provider
    )
}

fn read_settings(path: &PathBuf) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

fn write_settings(path: &PathBuf, v: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(path, s).map_err(|e| e.to_string())
}

fn gemini_is_installed(v: &Value) -> bool {
    v.pointer("/hooks/AfterModel")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter().any(|block| {
                block
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hooks| {
                        hooks
                            .iter()
                            .any(|h| h.get("name").and_then(|n| n.as_str()) == Some(HOOK_NAME))
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn gemini_strip_existing(v: &mut Value) {
    let Some(arr) = v
        .pointer_mut("/hooks/AfterModel")
        .and_then(|x| x.as_array_mut())
    else {
        return;
    };
    arr.retain(|block| {
        let hooks = block.get("hooks").and_then(|h| h.as_array());
        match hooks {
            Some(hs) => !hs
                .iter()
                .any(|h| h.get("name").and_then(|n| n.as_str()) == Some(HOOK_NAME)),
            None => true,
        }
    });
}

pub fn status_gemini(_app: &AppHandle) -> ProviderInstallStatus {
    let path = match gemini_settings_path() {
        Ok(p) => p,
        Err(e) => {
            return ProviderInstallStatus {
                provider: "gemini".into(),
                error: Some(e),
                ..Default::default()
            };
        }
    };
    let v = read_settings(&path).unwrap_or(json!({}));
    let installed = gemini_is_installed(&v);
    ProviderInstallStatus {
        provider: "gemini".into(),
        installed,
        settings_path: path.display().to_string(),
        connector_version: if installed {
            Some(CONNECTOR_VERSION.into())
        } else {
            None
        },
        error: None,
    }
}

pub fn install_gemini(app: &AppHandle) -> Result<ProviderInstallStatus, String> {
    let script = ensure_hook_script(app)?;
    let cmd = build_hook_command(&script, "gemini");
    let path = gemini_settings_path()?;
    let mut v = read_settings(&path)?;

    if !v.is_object() {
        v = json!({});
    }
    {
        let obj = v.as_object_mut().unwrap();
        if !obj.contains_key("hooks") {
            obj.insert("hooks".into(), json!({}));
        }
    }
    {
        let hooks = v.get_mut("hooks").unwrap();
        if !hooks.is_object() {
            *hooks = json!({});
        }
        let hooks_obj = hooks.as_object_mut().unwrap();
        if !hooks_obj.contains_key("AfterModel") {
            hooks_obj.insert("AfterModel".into(), json!([]));
        }
    }

    gemini_strip_existing(&mut v);
    let entry = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "name": HOOK_NAME,
            "command": cmd,
        }]
    });
    v.pointer_mut("/hooks/AfterModel")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| "hooks.AfterModel not an array".to_string())?
        .push(entry);

    write_settings(&path, &v)?;
    Ok(status_gemini(app))
}

pub fn uninstall_gemini(app: &AppHandle) -> Result<ProviderInstallStatus, String> {
    let path = gemini_settings_path()?;
    let mut v = read_settings(&path)?;
    gemini_strip_existing(&mut v);
    write_settings(&path, &v)?;
    Ok(status_gemini(app))
}

pub fn status_stub(provider: &str) -> ProviderInstallStatus {
    ProviderInstallStatus {
        provider: provider.into(),
        installed: false,
        settings_path: String::new(),
        connector_version: None,
        error: None,
    }
}
