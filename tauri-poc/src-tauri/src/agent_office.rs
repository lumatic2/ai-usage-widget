use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use crate::connector::ConnectorStore;

const TAIL_BYTES: u64 = 256 * 1024;
const ACTIVE_WINDOW_SECONDS: i64 = 24 * 3600;
const MAX_PROMPT_CHARS: usize = 160;
const MAX_LOG_ENTRIES: usize = 25;
const STALE_DONE_SECONDS: i64 = 30 * 60; // 30m idle → effectively dead session

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentOfficeState {
    pub sessions: Vec<SessionInfo>,
    pub log: Vec<ActivityLogEntry>,
    pub scanned_at_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub provider: String,             // "claude" | "codex"
    pub session_id: String,
    pub project_name: String,
    pub project_root: Option<String>, // git toplevel — used as room key
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    pub is_detached: bool,
    pub last_user_prompt: Option<String>,
    pub last_activity_kind: String,
    pub last_activity_at_ms: i64,
    pub idle_seconds: i64,
    pub status: String,
    pub turn_count: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogEntry {
    pub timestamp_ms: i64,
    pub session_id: String,
    pub project_name: String,
    pub kind: String,
    pub text: String,
}

fn projects_root() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".claude").join("projects")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn parse_iso8601_ms(s: &str) -> Option<i64> {
    // Minimal parser for "2026-05-21T02:18:04.509Z" — no chrono dep.
    // Accepts trailing Z, optional fractional seconds.
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    let second: i64 = s.get(17..19)?.parse().ok()?;
    let mut millis: i64 = 0;
    if let Some(dot_idx) = s.find('.') {
        let frac_start = dot_idx + 1;
        let frac_end = s[frac_start..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| frac_start + i)
            .unwrap_or(s.len());
        let frac = &s[frac_start..frac_end];
        if !frac.is_empty() {
            let truncated: String = frac.chars().take(3).collect();
            let padded = format!("{:0<3}", truncated);
            millis = padded.parse().unwrap_or(0);
        }
    }
    let days = days_from_civil(year, month, day);
    let total = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(total * 1000 + millis)
}

// Howard Hinnant's days_from_civil — converts (y,m,d) to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let doy = ((153 * (if m > 2 { m as i64 - 3 } else { m as i64 + 9 }) + 2) / 5 + d as i64 - 1) as i64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn git_toplevel(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn detect_worktree(start: &Path) -> bool {
    let Some(top) = git_toplevel(start) else { return false };
    let git_path = top.join(".git");
    // In worktrees, `.git` is a regular file containing "gitdir: ..." pointer.
    // In the primary working dir, `.git` is a directory.
    git_path.is_file()
}

fn project_name_from_slug_or_cwd(slug: &str, cwd: Option<&str>) -> String {
    if let Some(c) = cwd {
        let p = Path::new(c);
        if let Some(top) = git_toplevel(p) {
            if let Some(name) = top.file_name().and_then(|n| n.to_str()) {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    // slug like "C--Users-yusun-projects-ai-usage-widget" — take last hyphen-separated segment as a hint
    slug.rsplit('-').next().unwrap_or(slug).to_string()
}

fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
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

fn compact(text: &str, max_chars: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max_chars {
        let truncated: String = collapsed.chars().take(max_chars - 1).collect();
        format!("{truncated}…")
    } else {
        collapsed
    }
}

fn newest_jsonl_in(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &best {
            Some((_, prev)) if *prev >= mtime => {}
            _ => best = Some((path, mtime)),
        }
    }
    best
}

fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    // Real user prompt: message.content is a string.
    // tool_result entries: message.content is an array of objects with type=="tool_result".
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    None
}

fn extract_assistant_summary(content: &serde_json::Value) -> Option<(String, String)> {
    // Returns (kind, text). kind ∈ {"assistant_text", "tool_use:<name>"}
    let arr = content.as_array()?;
    // Prefer text blocks; fall back to last tool_use.
    let mut last_tool: Option<String> = None;
    let mut text: Option<String> = None;
    for block in arr {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    if !t.trim().is_empty() {
                        text = Some(t.to_string());
                    }
                }
            }
            "tool_use" => {
                if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
                    last_tool = Some(name.to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(t) = text {
        return Some(("assistant_text".into(), t));
    }
    if let Some(name) = last_tool {
        return Some((format!("tool_use:{name}"), format!("[{name}]")));
    }
    None
}

struct ParsedSession {
    last_user_prompt: Option<String>,
    last_activity_kind: String,
    last_activity_at_ms: i64,
    last_turn_role: String,           // "user_external" | "user_tool_result" | "assistant"
    last_assistant_stop_reason: String, // "tool_use" | "end_turn" | "" — only set when last_turn_role=="assistant"
    cwd: Option<String>,
    git_branch: Option<String>,
    turn_count: u32,
    events: Vec<(i64, String, String)>, // (ts_ms, kind, text)
}

fn parse_transcript_tail(content: &str) -> ParsedSession {
    let mut last_user_prompt: Option<String> = None;
    let mut last_activity_kind = String::from("unknown");
    let mut last_activity_at_ms: i64 = 0;
    let mut last_turn_role = String::new();
    let mut last_assistant_stop_reason = String::new();
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut turn_count: u32 = 0;
    let mut events: Vec<(i64, String, String)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else { continue };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts_ms = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);

        if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
            cwd = Some(c.to_string());
        }
        if let Some(b) = v.get("gitBranch").and_then(|x| x.as_str()) {
            if !b.is_empty() {
                git_branch = Some(b.to_string());
            }
        }

        match ty {
            "user" => {
                let user_type = v.get("userType").and_then(|x| x.as_str()).unwrap_or("");
                let content_v = v.pointer("/message/content");
                if let Some(cv) = content_v {
                    if user_type == "external" {
                        if let Some(txt) = extract_user_text(cv) {
                            let compacted = compact(&txt, MAX_PROMPT_CHARS);
                            last_user_prompt = Some(compacted.clone());
                            last_activity_kind = "user_prompt".into();
                            if ts_ms >= last_activity_at_ms {
                                last_activity_at_ms = ts_ms;
                                last_turn_role = "user_external".into();
                                last_assistant_stop_reason.clear();
                            }
                            turn_count += 1;
                            events.push((ts_ms, "info".into(), compacted));
                        }
                    } else if ts_ms >= last_activity_at_ms {
                        // tool_result coming back from the harness — Claude is actively iterating
                        last_activity_at_ms = ts_ms;
                        last_turn_role = "user_tool_result".into();
                        last_assistant_stop_reason.clear();
                    }
                }
            }
            "assistant" => {
                let content_v = v.pointer("/message/content");
                if let Some(cv) = content_v {
                    if let Some((kind, text)) = extract_assistant_summary(cv) {
                        last_activity_kind = kind.clone();
                        if ts_ms >= last_activity_at_ms {
                            last_activity_at_ms = ts_ms;
                            last_turn_role = "assistant".into();
                            last_assistant_stop_reason = v
                                .pointer("/message/stop_reason")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                        }
                        let log_kind = if kind == "assistant_text" { "success" } else { "info" };
                        events.push((ts_ms, log_kind.into(), compact(&text, MAX_PROMPT_CHARS)));
                    }
                }
            }
            _ => {}
        }
    }

    ParsedSession {
        last_user_prompt,
        last_activity_kind,
        last_activity_at_ms,
        last_turn_role,
        last_assistant_stop_reason,
        cwd,
        git_branch,
        turn_count,
        events,
    }
}

fn classify_status(idle_seconds: i64, last_turn_role: &str, stop_reason: &str) -> String {
    // Phase 1: no Stop hook yet. Use transcript signals.
    // "idle" = Claude is waiting for the user (end_turn). Otherwise we assume Claude is
    // still working unless transcript has gone silent for a long time (5 min).
    match last_turn_role {
        "assistant" => {
            if stop_reason == "end_turn" {
                "idle".into()
            } else if idle_seconds < 300 {
                // tool_use, max_tokens, etc. — still iterating
                "working".into()
            } else {
                "idle".into()
            }
        }
        "user_external" | "user_tool_result" => {
            if idle_seconds < 300 {
                "working".into()
            } else {
                "idle".into()
            }
        }
        _ => "idle".into(),
    }
}

pub fn scan(store: Option<Arc<ConnectorStore>>) -> AgentOfficeState {
    let root = projects_root();
    let now = now_ms();
    if !root.exists() {
        return AgentOfficeState {
            sessions: Vec::new(),
            log: Vec::new(),
            scanned_at_ms: now,
        };
    }

    let mut sessions: Vec<SessionInfo> = Vec::new();
    let mut all_events: Vec<ActivityLogEntry> = Vec::new();
    let live_pids = snapshot_live_pids();

    if let Some(s) = store.as_ref() {
        s.purge_dead_pids(&live_pids);
    }

    let Ok(dirs) = fs::read_dir(&root) else {
        return AgentOfficeState {
            sessions: Vec::new(),
            log: Vec::new(),
            scanned_at_ms: now,
        };
    };

    for entry in dirs.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some((jsonl_path, mtime)) = newest_jsonl_in(&path) else { continue };

        let mtime_ms = mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if (now - mtime_ms) / 1000 > ACTIVE_WINDOW_SECONDS {
            continue;
        }

        let Some(tail) = read_tail(&jsonl_path, TAIL_BYTES) else { continue };
        let parsed = parse_transcript_tail(&tail);

        let session_id = jsonl_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if parsed.last_activity_at_ms == 0 {
            // Race condition: Claude Code was writing to the file mid-read.
            // Fall back to the last-good cached SessionInfo for this session.
            if let Some(s) = store.as_ref() {
                if let Some(mut cached) = s.get_cached_session(&session_id) {
                    cached.idle_seconds = ((now - cached.last_activity_at_ms) / 1000).max(0);
                    sessions.push(cached);
                }
            }
            continue;
        }

        let project_name = project_name_from_slug_or_cwd(slug, parsed.cwd.as_deref());
        let idle_seconds = ((now - parsed.last_activity_at_ms) / 1000).max(0);
        let mut status = classify_status(
            idle_seconds,
            &parsed.last_turn_role,
            &parsed.last_assistant_stop_reason,
        );
        if let Some(s) = store.as_ref() {
            if let Some(pid) = s.get_session_pid(&session_id) {
                if !live_pids.contains(&pid) {
                    continue;
                }
            }
            if let Some(event) = s.get_agent_event(&session_id) {
                if event.event == "session_end" {
                    let event_ms = event.at.timestamp_millis();
                    if event_ms >= parsed.last_activity_at_ms - 2_000 {
                        continue;
                    }
                }
            }
        }
        if status == "idle" && idle_seconds > STALE_DONE_SECONDS {
            status = "done".into();
        }

        for (ts, kind, text) in &parsed.events {
            all_events.push(ActivityLogEntry {
                timestamp_ms: *ts,
                session_id: session_id.clone(),
                project_name: project_name.clone(),
                kind: kind.clone(),
                text: text.clone(),
            });
        }

        let (project_root, is_worktree) = parsed
            .cwd
            .as_deref()
            .map(|c| {
                let p = Path::new(c);
                (
                    git_toplevel(p).and_then(|t| t.to_str().map(|s| s.to_string())),
                    detect_worktree(p),
                )
            })
            .unwrap_or((None, false));
        let is_detached = matches!(parsed.git_branch.as_deref(), Some("HEAD"));

        let session_info = SessionInfo {
            provider: "claude".into(),
            session_id,
            project_name,
            project_root,
            cwd: parsed.cwd,
            git_branch: parsed.git_branch,
            is_worktree,
            is_detached,
            last_user_prompt: parsed.last_user_prompt,
            last_activity_kind: parsed.last_activity_kind,
            last_activity_at_ms: parsed.last_activity_at_ms,
            idle_seconds,
            status,
            turn_count: parsed.turn_count,
        };

        if let Some(s) = store.as_ref() {
            s.cache_session(&session_info);
        }

        sessions.push(session_info);
    }

    let (codex_sessions, codex_events) = scan_codex(ACTIVE_WINDOW_SECONDS);
    sessions.extend(codex_sessions);
    all_events.extend(codex_events);

    sessions.sort_by(|a, b| b.last_activity_at_ms.cmp(&a.last_activity_at_ms));
    all_events.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    all_events.truncate(MAX_LOG_ENTRIES);

    AgentOfficeState {
        sessions,
        log: all_events,
        scanned_at_ms: now,
    }
}

fn snapshot_live_pids() -> HashSet<u32> {
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new(),
    );
    sys.processes().keys().map(|p: &Pid| p.as_u32()).collect()
}

// ---- Codex rollout scanner ----

fn codex_sessions_root() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile).join(".codex").join("sessions")
}

fn walk_codex_rollouts(root: &Path, cutoff_ms: i64) -> Vec<(PathBuf, i64)> {
    let mut out: Vec<(PathBuf, i64)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            let mtime_ms = mtime
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if mtime_ms >= cutoff_ms {
                out.push((path, mtime_ms));
            }
        }
    }
    out
}

struct CodexParsed {
    session_id: String,
    cwd: Option<String>,
    last_user_prompt: Option<String>,
    last_activity_kind: String,
    last_activity_at_ms: i64,
    last_task_complete_ms: i64,
    turn_count: u32,
    events: Vec<(i64, String, String)>,
}

fn parse_codex_tail(content: &str) -> CodexParsed {
    let mut session_id = String::new();
    let mut cwd: Option<String> = None;
    let mut last_user_prompt: Option<String> = None;
    let mut last_activity_kind = String::from("unknown");
    let mut last_activity_at_ms: i64 = 0;
    let mut last_task_complete_ms: i64 = 0;
    let mut turn_count: u32 = 0;
    let mut events: Vec<(i64, String, String)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else { continue };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let ts_ms = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(parse_iso8601_ms)
            .unwrap_or(0);

        match ty {
            "session_meta" => {
                if let Some(id) = v.pointer("/payload/id").and_then(|x| x.as_str()) {
                    session_id = id.to_string();
                }
                if let Some(c) = v.pointer("/payload/cwd").and_then(|x| x.as_str()) {
                    cwd = Some(c.to_string());
                }
            }
            "event_msg" => {
                let payload_type = v
                    .pointer("/payload/type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                match payload_type {
                    "user_message" => {
                        if let Some(msg) =
                            v.pointer("/payload/message").and_then(|x| x.as_str())
                        {
                            let compacted = compact(msg, MAX_PROMPT_CHARS);
                            last_user_prompt = Some(compacted.clone());
                            last_activity_kind = "user_prompt".into();
                            if ts_ms >= last_activity_at_ms {
                                last_activity_at_ms = ts_ms;
                            }
                            turn_count += 1;
                            events.push((ts_ms, "info".into(), compacted));
                        }
                    }
                    "task_started" => {
                        last_activity_kind = "task_started".into();
                        if ts_ms >= last_activity_at_ms {
                            last_activity_at_ms = ts_ms;
                        }
                    }
                    "task_complete" => {
                        last_activity_kind = "task_complete".into();
                        if ts_ms >= last_activity_at_ms {
                            last_activity_at_ms = ts_ms;
                        }
                        if ts_ms >= last_task_complete_ms {
                            last_task_complete_ms = ts_ms;
                        }
                        let summary = v
                            .pointer("/payload/last_agent_message")
                            .and_then(|x| x.as_str())
                            .unwrap_or("complete")
                            .to_string();
                        events.push((ts_ms, "success".into(), compact(&summary, MAX_PROMPT_CHARS)));
                    }
                    _ => {}
                }
            }
            "response_item" => {
                let role = v
                    .pointer("/payload/role")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if role == "assistant" {
                    if let Some(arr) =
                        v.pointer("/payload/content").and_then(|x| x.as_array())
                    {
                        for block in arr {
                            if let Some(text) =
                                block.get("text").and_then(|x| x.as_str())
                            {
                                if !text.trim().is_empty() {
                                    last_activity_kind = "assistant_text".into();
                                    if ts_ms >= last_activity_at_ms {
                                        last_activity_at_ms = ts_ms;
                                    }
                                    events.push((
                                        ts_ms,
                                        "success".into(),
                                        compact(text, MAX_PROMPT_CHARS),
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    CodexParsed {
        session_id,
        cwd,
        last_user_prompt,
        last_activity_kind,
        last_activity_at_ms,
        last_task_complete_ms,
        turn_count,
        events,
    }
}

fn classify_codex_status(idle_seconds: i64, last_kind: &str, _has_completed: bool) -> String {
    // task_complete fires per task, not per session — treat as idle (awaiting user).
    if matches!(last_kind, "user_prompt" | "task_started") && idle_seconds < 300 {
        return "working".into();
    }
    if last_kind == "assistant_text" && idle_seconds < 60 {
        return "working".into();
    }
    if idle_seconds > STALE_DONE_SECONDS {
        return "done".into();
    }
    "idle".into()
}

fn scan_codex(active_window_seconds: i64) -> (Vec<SessionInfo>, Vec<ActivityLogEntry>) {
    let root = codex_sessions_root();
    if !root.exists() {
        return (Vec::new(), Vec::new());
    }
    let now = now_ms();
    let cutoff_ms = now - active_window_seconds * 1000;

    let mut rollouts = walk_codex_rollouts(&root, cutoff_ms);
    // Newest first.
    rollouts.sort_by(|a, b| b.1.cmp(&a.1));

    // De-duplicate by session_id (a session may write multiple rollouts? defensive).
    let mut sessions: Vec<SessionInfo> = Vec::new();
    let mut events: Vec<ActivityLogEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (path, _) in rollouts {
        let Some(tail) = read_tail(&path, TAIL_BYTES) else { continue };
        let parsed = parse_codex_tail(&tail);
        if parsed.session_id.is_empty() || parsed.last_activity_at_ms == 0 {
            continue;
        }
        if !seen.insert(parsed.session_id.clone()) {
            continue;
        }

        let project_name =
            project_name_from_slug_or_cwd("codex", parsed.cwd.as_deref());
        let idle_seconds = ((now - parsed.last_activity_at_ms) / 1000).max(0);
        let has_completed = parsed.last_task_complete_ms >= parsed.last_activity_at_ms - 2_000;
        let status = classify_codex_status(idle_seconds, &parsed.last_activity_kind, has_completed);

        for (ts, kind, text) in &parsed.events {
            events.push(ActivityLogEntry {
                timestamp_ms: *ts,
                session_id: parsed.session_id.clone(),
                project_name: project_name.clone(),
                kind: kind.clone(),
                text: text.clone(),
            });
        }

        let (project_root, is_worktree) = parsed
            .cwd
            .as_deref()
            .map(|c| {
                let p = Path::new(c);
                (
                    git_toplevel(p).and_then(|t| t.to_str().map(|s| s.to_string())),
                    detect_worktree(p),
                )
            })
            .unwrap_or((None, false));

        sessions.push(SessionInfo {
            provider: "codex".into(),
            session_id: parsed.session_id,
            project_name,
            project_root,
            cwd: parsed.cwd,
            git_branch: None,
            is_worktree,
            is_detached: false,
            last_user_prompt: parsed.last_user_prompt,
            last_activity_kind: parsed.last_activity_kind,
            last_activity_at_ms: parsed.last_activity_at_ms,
            idle_seconds,
            status,
            turn_count: parsed.turn_count,
        });
    }

    (sessions, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_parses_with_millis() {
        let ms = parse_iso8601_ms("2026-05-21T02:18:04.509Z").unwrap();
        // 2026-05-21 02:18:04.509 UTC
        assert_eq!(ms, 1779329884_509);
    }

    #[test]
    fn compact_truncates_long_text() {
        let s = "a".repeat(200);
        let out = compact(&s, 50);
        assert!(out.chars().count() <= 50);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn classify_status_with_stop_reason() {
        // Assistant finished a turn — waiting for user
        assert_eq!(classify_status(2, "assistant", "end_turn"), "idle");
        // Assistant emitted a tool_use, still iterating
        assert_eq!(classify_status(2, "assistant", "tool_use"), "working");
        assert_eq!(classify_status(200, "assistant", "tool_use"), "working");
        // Tool use stale for >5min — silent failure or hung
        assert_eq!(classify_status(400, "assistant", "tool_use"), "idle");
        // User just sent a prompt — Claude responding
        assert_eq!(classify_status(5, "user_external", ""), "working");
        // Tool result just landed — Claude responding next
        assert_eq!(classify_status(5, "user_tool_result", ""), "working");
        // No data
        assert_eq!(classify_status(1000, "", ""), "idle");
    }
}
