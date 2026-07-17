// Today's token usage + estimated cost per CLI, aggregated from local session
// files only (no API calls, read-only). Recipe and pitfall guards ported from
// cc-switch (rev f39d463) via W7-deep-cc-switch research notes:
// - Claude: fixed-depth scan incl. subagents/workflows, message.id dedup
//   (streaming snapshot vs final), "any tokens > 0" billable gate
// - Codex: token_count events carry *cumulative* totals — sum only deltas
//   (saturating_sub); forked/subagent sessions replay history, so their first
//   token_count is treated as baseline only
// - Gemini: tokens.thoughts bills as output
// - Cost: Claude input is fresh (cache excluded); Codex/Gemini input includes
//   cache reads and must be subtracted — see `input_includes_cache_read`

use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// ---- Public snapshot shape (camelCase for the renderer) ----

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CliUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    /// Provider-adjusted fresh input (cache reads subtracted where the CLI
    /// reports cache-inclusive input). What the display should call "tokens".
    pub billable_input_tokens: u64,
    pub cost_usd: f64,
    pub has_unpriced_model: bool,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TokenCostSnapshot {
    pub claude: CliUsage,
    pub codex: CliUsage,
    pub gemini: CliUsage,
}

// ---- Internal aggregation state ----

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl Tokens {
    fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_creation += other.cache_creation;
    }
    fn is_zero(&self) -> bool {
        self.input + self.output + self.cache_read + self.cache_creation == 0
    }
}

#[derive(Clone, Copy, Default)]
struct FileCursor {
    modified_nanos: u128,
    byte_offset: u64,
}

#[derive(Clone, Debug)]
pub struct ClaudeEntry {
    pub model: String,
    pub tokens: Tokens,
    pub has_stop: bool,
}

#[derive(Default)]
struct ClaudeScan {
    cursors: HashMap<PathBuf, FileCursor>,
    /// message.id -> best entry seen (today only; cleared on day rollover)
    entries: HashMap<String, ClaudeEntry>,
}

#[derive(Default)]
pub struct CodexFileState {
    cursor: FileCursor,
    prev: (u64, u64, u64), // cumulative (input, cached_input, output)
    /// Forked/subagent rollouts replay parent history into the first
    /// token_count — use it as baseline instead of importing it.
    baseline_pending: bool,
    model: String,
}

#[derive(Default)]
struct CodexScan {
    files: HashMap<PathBuf, CodexFileState>,
    totals: HashMap<String, Tokens>, // per normalized model
}

#[derive(Default)]
struct GeminiScan {
    /// Session files are whole JSON documents rewritten in place — recompute
    /// the file's totals from scratch whenever mtime changes.
    files: HashMap<PathBuf, (u128, HashMap<String, Tokens>)>,
}

#[derive(Default)]
pub struct TokenCostState {
    day_key: String,
    claude: ClaudeScan,
    codex: CodexScan,
    gemini: GeminiScan,
}

// ---- Pricing (USD per million tokens: input, output, cache_read, cache_creation) ----
// Sources: claude-api skill model table (2026-06); OpenAI/Gemini rates from the
// cc-switch pricing seed (rev f39d463). Prefix match, first hit wins — keep
// more specific keys before shorter prefixes.

type Rate = (f64, f64, f64, f64);

const PRICING: &[(&str, Rate)] = &[
    ("claude-fable-5", (10.0, 50.0, 1.0, 12.5)),
    ("claude-mythos-5", (10.0, 50.0, 1.0, 12.5)),
    ("claude-opus-4-8", (5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-7", (5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-6", (5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-5", (5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-1", (15.0, 75.0, 1.5, 18.75)),
    ("claude-opus-4", (15.0, 75.0, 1.5, 18.75)),
    ("claude-sonnet", (3.0, 15.0, 0.3, 3.75)),
    ("claude-haiku-4-5", (1.0, 5.0, 0.1, 1.25)),
    ("claude-3-5-haiku", (0.8, 4.0, 0.08, 1.0)),
    // GPT-5.6 tiers (official, 2026-07): Sol $5/$30, Terra $2.5/$15, Luna $1/$6;
    // cached input bills at 10% of the input rate. Bare "gpt-5.6" matches Sol.
    ("gpt-5.6-terra", (2.5, 15.0, 0.25, 0.0)),
    ("gpt-5.6-luna", (1.0, 6.0, 0.1, 0.0)),
    ("gpt-5.6", (5.0, 30.0, 0.5, 0.0)),
    ("gpt-5.5", (5.0, 30.0, 0.5, 0.0)),
    ("gpt-5.4-mini", (0.75, 4.5, 0.075, 0.0)),
    ("gpt-5.4-nano", (0.2, 1.25, 0.02, 0.0)),
    ("gpt-5.4", (2.5, 15.0, 0.25, 0.0)),
    ("gpt-5.3", (1.75, 14.0, 0.175, 0.0)),
    ("gpt-5.2", (1.75, 14.0, 0.175, 0.0)),
    ("gpt-5.1", (1.25, 10.0, 0.125, 0.0)),
    ("gpt-5", (1.25, 10.0, 0.125, 0.0)),
    ("gemini-3.5-flash", (1.5, 9.0, 0.15, 0.0)),
    ("gemini-3.1-pro", (2.0, 12.0, 0.2, 0.0)),
    ("gemini-3.1-flash-lite", (0.25, 1.5, 0.025, 0.0)),
    ("gemini-3-pro", (2.0, 12.0, 0.2, 0.0)),
    ("gemini-3-flash", (0.5, 3.0, 0.05, 0.0)),
    ("gemini-2.5-pro", (1.25, 10.0, 0.125, 0.0)),
    ("gemini-2.5-flash-lite", (0.1, 0.4, 0.01, 0.0)),
    ("gemini-2.5-flash", (0.3, 2.5, 0.03, 0.0)),
];

pub fn find_pricing(normalized_model: &str) -> Option<Rate> {
    PRICING
        .iter()
        .find(|(key, _)| normalized_model.starts_with(key))
        .map(|(_, rate)| *rate)
}

/// Lowercase, strip `provider/` prefix, `@effort` / `-YYYY-MM-DD` / `-YYYYMMDD`
/// suffixes — mirrors cc-switch `normalize_codex_model`, applied to all CLIs.
pub fn normalize_model(model: &str) -> String {
    let mut m = model.trim().to_ascii_lowercase();
    if let Some(idx) = m.rfind('/') {
        m = m[idx + 1..].to_string();
    }
    if let Some(idx) = m.find('@') {
        m.truncate(idx);
    }
    // -YYYY-MM-DD (11 chars)
    if m.len() > 11 {
        let tail = &m[m.len() - 11..];
        if tail.starts_with('-')
            && tail[1..].chars().all(|c| c.is_ascii_digit() || c == '-')
            && tail[1..].chars().filter(|c| *c == '-').count() == 2
        {
            m.truncate(m.len() - 11);
        }
    }
    // -YYYYMMDD (9 chars)
    if m.len() > 9 {
        let tail = &m[m.len() - 9..];
        if tail.starts_with('-') && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            m.truncate(m.len() - 9);
        }
    }
    m
}

#[derive(Clone, Copy, PartialEq)]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
}

/// Anthropic reports fresh input; OpenAI/Gemini input already includes cache
/// reads, which must be subtracted before pricing (cc-switch calculator.rs).
fn input_includes_cache_read(provider: Provider) -> bool {
    matches!(provider, Provider::Codex | Provider::Gemini)
}

fn build_cli_usage(provider: Provider, per_model: &HashMap<String, Tokens>) -> CliUsage {
    let mut usage = CliUsage::default();
    for (model, t) in per_model {
        usage.input_tokens += t.input;
        usage.output_tokens += t.output;
        usage.cache_read_tokens += t.cache_read;
        usage.cache_creation_tokens += t.cache_creation;
        let billable_input = if input_includes_cache_read(provider) {
            t.input.saturating_sub(t.cache_read)
        } else {
            t.input
        };
        usage.billable_input_tokens += billable_input;
        match find_pricing(model) {
            Some((pi, po, pcr, pcc)) => {
                usage.cost_usd += (billable_input as f64 * pi
                    + t.output as f64 * po
                    + t.cache_read as f64 * pcr
                    + t.cache_creation as f64 * pcc)
                    / 1_000_000.0;
            }
            None => usage.has_unpriced_model = true,
        }
    }
    usage
}

// ---- Time helpers ----

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn is_today_rfc3339(ts: &str) -> Option<bool> {
    let dt = DateTime::parse_from_rfc3339(ts).ok()?;
    Some(dt.with_timezone(&Local).format("%Y-%m-%d").to_string() == today_key())
}

fn modified_nanos(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn mtime_is_today(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| DateTime::<Local>::from(t).format("%Y-%m-%d").to_string() == today_key())
        .unwrap_or(false)
}

/// Read new content since `cursor.byte_offset`; returns None when unchanged.
/// A shrunk file (rotation/rewrite) resets to offset 0.
fn read_new_lines(path: &Path, cursor: &mut FileCursor) -> Option<String> {
    let modified = modified_nanos(path);
    if modified == cursor.modified_nanos {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len < cursor.byte_offset {
        cursor.byte_offset = 0;
    }
    file.seek(SeekFrom::Start(cursor.byte_offset)).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    cursor.modified_nanos = modified;
    cursor.byte_offset = len;
    Some(buf)
}

// ---- Claude Code (~/.claude/projects) ----

fn home_dir() -> PathBuf {
    let profile = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(profile)
}

fn push_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

/// Fixed-depth scan: projects/<proj>/*.jsonl, plus per-session
/// <proj>/<session>/subagents/*.jsonl and subagents/workflows/<wf>/*.jsonl.
/// Missing the subagent levels silently drops their token usage.
fn collect_claude_files() -> Vec<PathBuf> {
    let root = home_dir().join(".claude").join("projects");
    let mut files = Vec::new();
    let Ok(projects) = std::fs::read_dir(&root) else { return files };
    for project in projects.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        push_jsonl(&project_path, &mut files);
        let Ok(children) = std::fs::read_dir(&project_path) else { continue };
        for child in children.flatten() {
            let sub = child.path().join("subagents");
            if !sub.is_dir() {
                continue;
            }
            push_jsonl(&sub, &mut files);
            let workflows = sub.join("workflows");
            if let Ok(wf_dirs) = std::fs::read_dir(&workflows) {
                for wf in wf_dirs.flatten() {
                    if wf.path().is_dir() {
                        push_jsonl(&wf.path(), &mut files);
                    }
                }
            }
        }
    }
    files
}

/// Parse one assistant line: (message.id, entry, is_today). Applies the
/// billable gate (any token field > 0) — required to count partial
/// workflow/subagent writes that never receive a stop_reason.
pub fn parse_claude_line(line: &str) -> Option<(String, ClaudeEntry, bool)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    let id = msg.get("id")?.as_str()?.to_string();
    let usage = msg.get("usage")?;
    let g = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let tokens = Tokens {
        input: g("input_tokens"),
        output: g("output_tokens"),
        cache_read: g("cache_read_input_tokens"),
        cache_creation: g("cache_creation_input_tokens"),
    };
    if tokens.is_zero() {
        return None;
    }
    let entry = ClaudeEntry {
        model: normalize_model(
            msg.get("model").and_then(|m| m.as_str()).unwrap_or("unknown"),
        ),
        tokens,
        has_stop: msg
            .get("stop_reason")
            .map(|s| !s.is_null())
            .unwrap_or(false),
    };
    let is_today = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(is_today_rfc3339)
        .unwrap_or(true);
    Some((id, entry, is_today))
}

/// Dedup rule for duplicate message.id (streaming snapshot + final line):
/// prefer the entry with stop_reason; ties break on larger output.
pub fn merge_claude_entry(existing: &mut ClaudeEntry, new: ClaudeEntry) {
    let replace = match (existing.has_stop, new.has_stop) {
        (false, true) => true,
        (true, false) => false,
        _ => new.tokens.output > existing.tokens.output,
    };
    if replace {
        *existing = new;
    }
}

fn scan_claude(scan: &mut ClaudeScan) -> HashMap<String, Tokens> {
    for path in collect_claude_files() {
        // Files not touched today cannot contain today's messages.
        if !mtime_is_today(&path) {
            continue;
        }
        let cursor = scan.cursors.entry(path.clone()).or_default();
        let Some(new_text) = read_new_lines(&path, cursor) else { continue };
        for line in new_text.lines() {
            if let Some((id, entry, is_today)) = parse_claude_line(line) {
                if !is_today {
                    continue;
                }
                match scan.entries.get_mut(&id) {
                    Some(existing) => merge_claude_entry(existing, entry),
                    None => {
                        scan.entries.insert(id, entry);
                    }
                }
            }
        }
    }
    let mut per_model: HashMap<String, Tokens> = HashMap::new();
    for entry in scan.entries.values() {
        per_model.entry(entry.model.clone()).or_default().add(&entry.tokens);
    }
    per_model
}

// ---- Codex (~/.codex/sessions/YYYY/MM/DD) ----

pub enum CodexEvent {
    SessionMeta { fork: bool },
    TurnContext { model: String },
    TokenCount {
        input: u64,
        cached_input: u64,
        output: u64,
        /// None when the line has no parseable timestamp.
        is_today: Option<bool>,
    },
}

pub fn parse_codex_line(line: &str) -> Option<CodexEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload")?;
    match v.get("type")?.as_str()? {
        "session_meta" => {
            let fork = payload
                .get("forked_from_id")
                .and_then(|x| x.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
                || payload
                    .get("source")
                    .map(|s| s.get("subagent").is_some())
                    .unwrap_or(false);
            Some(CodexEvent::SessionMeta { fork })
        }
        "turn_context" => {
            let model = payload
                .get("model")
                .or_else(|| payload.get("info").and_then(|i| i.get("model")))
                .and_then(|m| m.as_str())?
                .to_string();
            Some(CodexEvent::TurnContext { model })
        }
        "event_msg" => {
            if payload.get("type")?.as_str()? != "token_count" {
                return None;
            }
            let info = payload.get("info")?;
            let usage = info
                .get("total_token_usage")
                .or_else(|| info.get("last_token_usage"))?;
            let g = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
            Some(CodexEvent::TokenCount {
                input: g("input_tokens"),
                cached_input: g("cached_input_tokens"),
                output: g("output_tokens"),
                is_today: v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(is_today_rfc3339),
            })
        }
        _ => None,
    }
}

/// Cumulative counters -> per-event delta. Returns None when the event only
/// establishes a baseline (fork history replay). Cached delta is clamped to
/// the input delta.
pub fn codex_delta(state: &mut CodexFileState, cur: (u64, u64, u64)) -> Option<Tokens> {
    if state.baseline_pending {
        state.prev = cur;
        state.baseline_pending = false;
        return None;
    }
    let input = cur.0.saturating_sub(state.prev.0);
    let cached = cur.1.saturating_sub(state.prev.1).min(input);
    let output = cur.2.saturating_sub(state.prev.2);
    state.prev = cur;
    if input + cached + output == 0 {
        return None;
    }
    Some(Tokens {
        input,
        output,
        cache_read: cached,
        cache_creation: 0,
    })
}

/// (path, dir_is_today). Sessions live under dated directories keyed by their
/// *start* day, so a session spanning midnight keeps writing into yesterday's
/// directory — scan both days and attribute per-event via line timestamps.
fn collect_codex_files() -> Vec<(PathBuf, bool)> {
    let sessions = home_dir().join(".codex").join("sessions");
    let now = Local::now();
    let mut files = Vec::new();
    for (day, is_today) in [(now - chrono::Duration::days(1), false), (now, true)] {
        let dir = sessions
            .join(day.format("%Y").to_string())
            .join(day.format("%m").to_string())
            .join(day.format("%d").to_string());
        let mut day_files = Vec::new();
        push_jsonl(&dir, &mut day_files);
        files.extend(day_files.into_iter().map(|p| (p, is_today)));
    }
    files
}

fn scan_codex(scan: &mut CodexScan) {
    for (path, dir_is_today) in collect_codex_files() {
        // Yesterday's files that haven't been written today can't contain
        // today's events — skip without opening.
        if !dir_is_today && !mtime_is_today(&path) {
            continue;
        }
        let state = scan.files.entry(path.clone()).or_default();
        let Some(new_text) = read_new_lines(&path, &mut state.cursor) else { continue };
        for line in new_text.lines() {
            match parse_codex_line(line) {
                Some(CodexEvent::SessionMeta { fork }) => {
                    if fork {
                        state.baseline_pending = true;
                    }
                }
                Some(CodexEvent::TurnContext { model }) => {
                    state.model = normalize_model(&model);
                }
                Some(CodexEvent::TokenCount { input, cached_input, output, is_today }) => {
                    // Always advance the cumulative baseline; only *count* the
                    // delta when the event happened today.
                    let delta = codex_delta(state, (input, cached_input, output));
                    if !is_today.unwrap_or(dir_is_today) {
                        continue;
                    }
                    if let Some(delta) = delta {
                        let model = if state.model.is_empty() {
                            "gpt-5.5".to_string()
                        } else {
                            state.model.clone()
                        };
                        scan.totals.entry(model).or_default().add(&delta);
                    }
                }
                None => {}
            }
        }
    }
}

// ---- Gemini (~/.gemini/tmp/<hash>/chats/session-*.json) ----

/// Whole-file totals per model. thoughts (reasoning) tokens bill as output;
/// pure cache-hit rows (cached > 0, rest 0) are NOT skipped.
pub fn parse_gemini_session(text: &str) -> HashMap<String, Tokens> {
    let mut per_model: HashMap<String, Tokens> = HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return per_model;
    };
    let Some(messages) = v.get("messages").and_then(|m| m.as_array()) else {
        return per_model;
    };
    for msg in messages {
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }
        let Some(tokens) = msg.get("tokens") else { continue };
        let g = |k: &str| tokens.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let (input, output, cached, thoughts) =
            (g("input"), g("output"), g("cached"), g("thoughts"));
        if input + output + cached + thoughts == 0 {
            continue;
        }
        let is_today = msg
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(is_today_rfc3339)
            .unwrap_or(true);
        if !is_today {
            continue;
        }
        let model = normalize_model(
            msg.get("model").and_then(|m| m.as_str()).unwrap_or("gemini"),
        );
        per_model.entry(model).or_default().add(&Tokens {
            input,
            output: output + thoughts,
            cache_read: cached,
            cache_creation: 0,
        });
    }
    per_model
}

fn collect_gemini_files() -> Vec<PathBuf> {
    let root = home_dir().join(".gemini").join("tmp");
    let mut files = Vec::new();
    let Ok(hashes) = std::fs::read_dir(&root) else { return files };
    for hash_dir in hashes.flatten() {
        let chats = hash_dir.path().join("chats");
        let Ok(entries) = std::fs::read_dir(&chats) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("session-") && name.ends_with(".json") {
                files.push(p);
            }
        }
    }
    files
}

fn scan_gemini(scan: &mut GeminiScan) -> HashMap<String, Tokens> {
    for path in collect_gemini_files() {
        if !mtime_is_today(&path) {
            continue;
        }
        let modified = modified_nanos(&path);
        let needs_parse = scan
            .files
            .get(&path)
            .map(|(m, _)| *m != modified)
            .unwrap_or(true);
        if needs_parse {
            if let Ok(text) = std::fs::read_to_string(&path) {
                scan.files.insert(path, (modified, parse_gemini_session(&text)));
            }
        }
    }
    let mut per_model: HashMap<String, Tokens> = HashMap::new();
    for (_, totals) in scan.files.values() {
        for (model, t) in totals {
            per_model.entry(model.clone()).or_default().add(t);
        }
    }
    per_model
}

// ---- Entry point ----

pub fn refresh(state: &mut TokenCostState) -> TokenCostSnapshot {
    let today = today_key();
    if state.day_key != today {
        *state = TokenCostState {
            day_key: today,
            ..TokenCostState::default()
        };
    }
    let claude = scan_claude(&mut state.claude);
    scan_codex(&mut state.codex);
    let gemini = scan_gemini(&mut state.gemini);
    TokenCostSnapshot {
        claude: build_cli_usage(Provider::Claude, &claude),
        codex: build_cli_usage(Provider::Codex, &state.codex.totals),
        gemini: build_cli_usage(Provider::Gemini, &gemini),
    }
}

// ---- Tests (pure functions only, per repo convention) ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_provider_effort_and_dates() {
        assert_eq!(normalize_model("openai/gpt-5.5-high"), "gpt-5.5-high");
        assert_eq!(normalize_model("gpt-5.2-codex@low"), "gpt-5.2-codex");
        assert_eq!(normalize_model("claude-opus-4-5-20251101"), "claude-opus-4-5");
        assert_eq!(normalize_model("gemini-3-pro-preview-2026-05-14"), "gemini-3-pro-preview");
        assert_eq!(normalize_model("Claude-Fable-5"), "claude-fable-5");
    }

    #[test]
    fn pricing_prefix_match_prefers_specific_keys() {
        assert_eq!(find_pricing("claude-fable-5").unwrap().0, 10.0);
        assert_eq!(find_pricing("claude-opus-4-8").unwrap().0, 5.0);
        // Old Opus 4 must not match the 4-8 rate
        assert_eq!(find_pricing("claude-opus-4").unwrap().0, 15.0);
        assert_eq!(find_pricing("gemini-2.5-flash-lite").unwrap().1, 0.4);
        assert_eq!(find_pricing("gpt-5.5-high").unwrap().1, 30.0);
        assert!(find_pricing("mystery-model").is_none());
    }

    #[test]
    fn claude_line_parses_and_gates_billable() {
        let line = r#"{"type":"assistant","timestamp":"2099-01-01T00:00:00Z","message":{"id":"m1","model":"claude-fable-5","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":5,"cache_creation_input_tokens":1}}}"#;
        let (id, entry, is_today) = parse_claude_line(line).unwrap();
        assert_eq!(id, "m1");
        assert_eq!(entry.tokens.output, 20);
        assert!(entry.has_stop);
        assert!(!is_today);

        let zero = r#"{"type":"assistant","message":{"id":"m2","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        assert!(parse_claude_line(zero).is_none());
        let user = r#"{"type":"user","message":{"id":"m3","usage":{"output_tokens":9}}}"#;
        assert!(parse_claude_line(user).is_none());
    }

    #[test]
    fn claude_dedup_prefers_stop_then_larger_output() {
        let base = ClaudeEntry {
            model: "claude-fable-5".into(),
            tokens: Tokens { input: 1, output: 5, cache_read: 0, cache_creation: 0 },
            has_stop: false,
        };
        let mut current = base.clone();
        let final_entry = ClaudeEntry {
            tokens: Tokens { input: 1, output: 3, cache_read: 0, cache_creation: 0 },
            has_stop: true,
            ..base.clone()
        };
        merge_claude_entry(&mut current, final_entry);
        assert!(current.has_stop);
        assert_eq!(current.tokens.output, 3);

        // A later snapshot without stop must not replace the final entry
        let snapshot = ClaudeEntry { has_stop: false, ..base.clone() };
        merge_claude_entry(&mut current, snapshot);
        assert!(current.has_stop);
    }

    #[test]
    fn codex_delta_sums_diffs_not_cumulative() {
        let mut st = CodexFileState::default();
        let d1 = codex_delta(&mut st, (100, 40, 10)).unwrap();
        assert_eq!((d1.input, d1.cache_read, d1.output), (100, 40, 10));
        let d2 = codex_delta(&mut st, (250, 90, 25)).unwrap();
        assert_eq!((d2.input, d2.cache_read, d2.output), (150, 50, 15));
        // Anomalous decrease must not panic or produce garbage
        let d3 = codex_delta(&mut st, (200, 80, 30)).unwrap();
        assert_eq!((d3.input, d3.cache_read, d3.output), (0, 0, 5));
    }

    #[test]
    fn codex_token_count_parses_line_timestamp() {
        let line = r#"{"timestamp":"2099-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":3}}}}"#;
        let Some(CodexEvent::TokenCount { input, is_today, .. }) = parse_codex_line(line) else {
            panic!("expected TokenCount");
        };
        assert_eq!(input, 10);
        assert_eq!(is_today, Some(false));

        let no_ts = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":0}}}}"#;
        let Some(CodexEvent::TokenCount { is_today, .. }) = parse_codex_line(no_ts) else {
            panic!("expected TokenCount");
        };
        assert_eq!(is_today, None);
    }

    #[test]
    fn codex_fork_first_count_is_baseline_only() {
        let mut st = CodexFileState { baseline_pending: true, ..Default::default() };
        assert!(codex_delta(&mut st, (5000, 4000, 900)).is_none());
        let d = codex_delta(&mut st, (5100, 4050, 920)).unwrap();
        assert_eq!((d.input, d.cache_read, d.output), (100, 50, 20));
    }

    #[test]
    fn gemini_thoughts_bill_as_output_and_cache_only_rows_kept() {
        let text = r#"{"messages":[
            {"type":"gemini","model":"gemini-3-pro-preview","tokens":{"input":100,"output":50,"cached":10,"thoughts":30}},
            {"type":"gemini","model":"gemini-3-pro-preview","tokens":{"input":0,"output":0,"cached":25,"thoughts":0}},
            {"type":"user","tokens":{"input":9,"output":9}}
        ]}"#;
        let totals = parse_gemini_session(text);
        let t = totals.get("gemini-3-pro-preview").unwrap();
        assert_eq!(t.output, 80); // 50 + 30 thoughts
        assert_eq!(t.cache_read, 35);
        assert_eq!(t.input, 100);
    }

    #[test]
    fn cost_subtracts_cache_from_input_for_codex_and_gemini_only() {
        let mut per_model = HashMap::new();
        per_model.insert(
            "gpt-5.5".to_string(),
            Tokens { input: 1_000_000, output: 0, cache_read: 400_000, cache_creation: 0 },
        );
        let codex = build_cli_usage(Provider::Codex, &per_model);
        // (1M - 400K) * $5 + 400K * $0.5 = $3.0 + $0.2
        assert!((codex.cost_usd - 3.2).abs() < 1e-9);
        assert_eq!(codex.billable_input_tokens, 600_000);

        let mut claude_model = HashMap::new();
        claude_model.insert(
            "claude-fable-5".to_string(),
            Tokens { input: 100_000, output: 0, cache_read: 1_000_000, cache_creation: 0 },
        );
        let claude = build_cli_usage(Provider::Claude, &claude_model);
        // 100K * $10 + 1M * $1.0 = $1.0 + $1.0 — input NOT reduced by cache
        assert!((claude.cost_usd - 2.0).abs() < 1e-9);
        assert_eq!(claude.billable_input_tokens, 100_000);
    }

    /// Manual smoke test against this machine's real session files:
    /// `cargo test real_scan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_scan_smoke() {
        let mut state = TokenCostState::default();
        let start = std::time::Instant::now();
        let snap = refresh(&mut state);
        println!("scan took {:?}", start.elapsed());
        println!("claude: {:?}", snap.claude);
        println!("codex:  {:?}", snap.codex);
        println!("gemini: {:?}", snap.gemini);
        let start2 = std::time::Instant::now();
        let _ = refresh(&mut state);
        println!("incremental rescan took {:?}", start2.elapsed());
    }

    #[test]
    fn unpriced_model_flags_but_still_counts_tokens() {
        let mut per_model = HashMap::new();
        per_model.insert(
            "mystery-model".to_string(),
            Tokens { input: 10, output: 5, cache_read: 0, cache_creation: 0 },
        );
        let usage = build_cli_usage(Provider::Claude, &per_model);
        assert!(usage.has_unpriced_model);
        assert_eq!(usage.cost_usd, 0.0);
        assert_eq!(usage.input_tokens, 10);
    }
}
