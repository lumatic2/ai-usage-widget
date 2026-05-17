use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentPresence {
    pub claude_count: u32,
    pub codex_count: u32,
    pub gemini_count: u32,
}

pub fn scan() -> AgentPresence {
    let self_pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new()
            .with_cmd(sysinfo::UpdateKind::Always)
            .with_exe(sysinfo::UpdateKind::Always),
    );

    let mut out = AgentPresence::default();
    for (pid, proc_) in sys.processes() {
        if *pid == self_pid {
            continue;
        }
        let exe = proc_
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let name = proc_.name().to_string_lossy().to_ascii_lowercase();
        let cmd_joined: String = proc_
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let hay = format!("{exe} {name} {cmd_joined}").replace('\\', "/");

        // Exclude bundled desktop apps and browser native messaging hosts.
        if hay.contains("/windowsapps/")
            || hay.contains("/appdata/local/packages/")
            || hay.contains("chrome-native-host")
        {
            continue;
        }

        if matches_claude(&hay) {
            out.claude_count = out.claude_count.saturating_add(1);
        } else if matches_codex(&hay) {
            out.codex_count = out.codex_count.saturating_add(1);
        } else if matches_gemini(&hay) {
            out.gemini_count = out.gemini_count.saturating_add(1);
        }
    }
    out
}

fn matches_claude(h: &str) -> bool {
    h.contains("@anthropic-ai/claude-code") || h.contains("/.local/bin/claude.exe") || h.contains("/.local/bin/claude ")
}

fn matches_codex(h: &str) -> bool {
    h.contains("@openai/codex") || h.contains("/.local/bin/codex.exe") || h.contains("/.local/bin/codex ")
}

fn matches_gemini(h: &str) -> bool {
    h.contains("@google/gemini-cli") || h.contains("/.local/bin/gemini.exe") || h.contains("/.local/bin/gemini ")
}
