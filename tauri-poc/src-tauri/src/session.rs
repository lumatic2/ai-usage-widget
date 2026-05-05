use serde::Deserialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

const ROLLOUT_TAIL_BYTES: u64 = 256 * 1024;
const MAX_LABEL_CHARS: usize = 40;

#[derive(Default)]
pub struct SessionCache {
    latest_path: Option<PathBuf>,
    latest_path_mtime: Option<SystemTime>,
    latest_message_mtime: Option<SystemTime>,
    label: String,
    last_scan_at: Option<Instant>,
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

fn find_latest_rollout() -> Option<(PathBuf, SystemTime)> {
    let root = codex_home().join("sessions");
    if !root.exists() {
        return None;
    }
    let mut latest: Option<(PathBuf, SystemTime)> = None;
    let mut stack: Vec<PathBuf> = vec![root];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            match &latest {
                Some((_, prev)) if *prev >= mtime => {}
                _ => latest = Some((path, mtime)),
            }
        }
    }
    latest
}

fn read_tail(path: &PathBuf, max_bytes: u64) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return Some(String::new());
    }
    let to_read = max_bytes.min(len);
    let start = len - to_read;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = vec![0u8; to_read as usize];
    file.read_exact(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[derive(Deserialize)]
struct RolloutLine {
    payload: Option<RolloutPayload>,
}

#[derive(Deserialize)]
struct RolloutPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    message: Option<String>,
}

fn find_latest_user_message(content: &str) -> Option<String> {
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<RolloutLine>(trimmed) else { continue };
        let Some(payload) = parsed.payload else { continue };
        if payload.payload_type.as_deref() == Some("user_message") {
            if let Some(msg) = payload.message {
                let trimmed_msg = msg.trim().to_string();
                if !trimmed_msg.is_empty() {
                    return Some(trimmed_msg);
                }
            }
        }
    }
    None
}

fn compact_label(message: &str) -> String {
    let collapsed: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX_LABEL_CHARS {
        let truncated: String = collapsed.chars().take(MAX_LABEL_CHARS - 1).collect();
        format!("{truncated}...")
    } else {
        collapsed
    }
}

pub fn load_label(cache: &mut SessionCache, scan_ttl_ms: u64) -> String {
    let needs_scan = match (cache.last_scan_at, &cache.latest_path) {
        (Some(at), Some(_)) => at.elapsed().as_millis() as u64 >= scan_ttl_ms,
        _ => true,
    };

    if needs_scan {
        match find_latest_rollout() {
            Some((path, mtime)) => {
                let path_changed = cache.latest_path.as_ref() != Some(&path);
                cache.latest_path = Some(path);
                cache.latest_path_mtime = Some(mtime);
                if path_changed {
                    cache.latest_message_mtime = None;
                }
            }
            None => {
                cache.latest_path = None;
                cache.latest_path_mtime = None;
                cache.latest_message_mtime = None;
                cache.label = "No recent session".into();
            }
        }
        cache.last_scan_at = Some(Instant::now());
    }

    let Some(path) = cache.latest_path.clone() else {
        return cache.label.clone();
    };

    let mtime = match fs::metadata(&path).and_then(|m| m.modified()) {
        Ok(m) => m,
        Err(_) => return cache.label.clone(),
    };

    if cache.latest_path_mtime != Some(mtime) {
        cache.latest_path_mtime = Some(mtime);
        cache.latest_message_mtime = None;
    }
    if cache.latest_message_mtime == Some(mtime) {
        return cache.label.clone();
    }

    let tail = read_tail(&path, ROLLOUT_TAIL_BYTES).unwrap_or_default();
    let mut latest_message = find_latest_user_message(&tail);
    if latest_message.is_none() {
        if let Ok(full) = fs::read_to_string(&path) {
            latest_message = find_latest_user_message(&full);
        }
    }

    cache.latest_message_mtime = Some(mtime);
    cache.label = match latest_message {
        Some(msg) => compact_label(&msg),
        None => "Recent session".into(),
    };
    cache.label.clone()
}
