# AI Usage Widget

Minimal Windows desktop widget for monitoring **Claude** and **Codex** usage side by side.

A small transparent Tauri window that stays on top — draggable, always visible, ~13MB exe.

Shows for each AI tool:

- `5-HOUR` usage
- `WEEKLY` usage
- Reset timers for both limits

![AI Usage Widget Preview](./assets/widget-screenshot.png)

## Why

I run Claude Code and Codex side by side every day, and the answer to "do I have headroom for one more big task?" lived in two different web tabs and a CLI command. This widget pulls both 5-hour and weekly windows into one always-visible 410/780-pixel surface, so I can glance at a corner of the monitor instead of context-switching.

## Features

- Pixel-style floating widget
- Side-by-side **Claude** + **Codex** panels
- Toggle each panel on/off from settings
- Frameless transparent window, drag-to-move
- Always-on-top, auto-resizes when toggling panels (single 410 / both 780)
- Threshold-crossing native Windows toasts (30 / 60 / 80 / 90% by default)
- Optional auto-launch at Windows login
- Last-good 5-min cache — transient network blips don't blank the widget

## Requirements

- Windows 10/11 (WebView2 runtime — preinstalled on modern Windows)
- For development: Node.js 24+ and Rust 1.95+ (MSVC toolchain)
- **Codex panel**: active Codex login in `~/.codex/auth.json`
- **Claude panel**: active claude.ai session (sign in via the widget)

> **macOS / Linux**: not supported in v0.1.x. The Rust/Tauri side is mostly portable, but the Windows-only pieces (NSIS+MSI bundle, `tauri-plugin-autostart` LaunchAgent path, `%APPDATA%` config dir, native toast surface) need rework. PRs welcome.

## Quick Start (prebuilt)

Grab the latest installer from [Releases](https://github.com/lumatic2/ai-usage-widget/releases):

| File | Use when |
|---|---|
| `AI Usage Widget_*_x64-setup.exe` (NSIS, ~3MB) | Standard install |
| `AI Usage Widget_*_x64_en-US.msi` (~4MB) | Group-policy / silent install |

> **Windows SmartScreen note**: The build is **not code-signed**, so the first launch may show *"Windows protected your PC"*. Click **More info** → **Run anyway**. To verify yourself, build from source.

## Build from source

```powershell
git clone https://github.com/lumatic2/ai-usage-widget.git
cd ai-usage-widget
npm install
npm --prefix tauri-poc install
npm run dev          # tauri dev — hot-reload frontend, full Rust rebuild on backend change
```

Production build:

```powershell
npm run build        # produces tauri-poc/src-tauri/target/release/{exe, bundle/msi/, bundle/nsis/}
```

The root `package.json` is a thin wrapper; the actual project lives under `tauri-poc/`.

## Settings

Runtime settings are stored at:

`%APPDATA%\com.lumatic2.ai-usage-widget\settings.json`

| Setting | Description |
|---|---|
| `displayMode` | `used` (current %) or `remaining` (default: `used`) |
| `refreshIntervalMs` | Refresh interval in ms (10s–10min, clamped) |
| `enableUsageAlerts` | `true` / `false` |
| `usageAlertThresholds` | Array of percentages, e.g. `[30,60,80,90]` (1–100, deduped) |
| `openOnStartup` | Open widget at Windows login (writes a Run-key entry) |
| `showClaude` / `showCodex` | Show/hide each panel |
| `fetchTimeoutMs` | Request timeout (2s–60s) |
| `fetchRetries` | Retry count on transient failures (0–5) |
| `sessionScanTtlMs` | Codex session-label rescan interval (30s–1h) |
| `cachedOrgUuid` / `claudeSessionKey` | Auto-managed by the widget; do not hand-edit |

The settings file is sanitized on every load and update — out-of-range values are clamped or replaced.

## How It Works

**Codex panel**
- Bearer token from `~/.codex/auth.json` → `https://chatgpt.com/backend-api/wham/usage`
- 401/403 = expired (immediate); 429/5xx = bounded backoff retry
- Recent session label scanned from `~/.codex/sessions/**/rollout-*.jsonl` (256KB tail, mtime-cached)

**Claude panel**
- Primary: Bearer token from `~/.claude/.credentials.json` (`claudeAiOauth.accessToken`)
- Fallback: when bearer fails, uses `sessionKey` cookie captured during in-app login
- Org UUID resolved from `/api/organizations` and cached in settings

No tokens are stored in this repository. Credentials stay on the local machine.

## Security

- This repository does **not** contain your tokens or auth files
- The app reads local auth files at runtime only
- Bearer tokens are never written back to disk; the `sessionKey` cookie *is* persisted to settings to avoid forcing re-login on every refresh
- TLS uses `rustls` with the Windows native trust store (compatible with corporate CAs)
- Anyone with local machine access can read the same auth files — same trust boundary as Claude Code / Codex CLI themselves

## Development layout

```
tauri-poc/
├── package.json           # tauri scripts (cli)
├── src/                   # renderer (vanilla JS/CSS)
│   ├── index.html
│   ├── styles.css
│   └── main.js
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    └── src/
        ├── lib.rs         # builder, commands, refresh loop, alerts, autostart sync
        ├── codex.rs       # ChatGPT usage fetch
        ├── claude.rs      # Claude usage fetch (bearer + cookie)
        └── session.rs     # rollout scan / label
```

## License

MIT
