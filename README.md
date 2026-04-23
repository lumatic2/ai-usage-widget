# AI Usage Widget

Minimal Windows desktop widget for monitoring **Claude** and **Codex** usage side by side.

Runs as a small transparent Electron window — stays on top, draggable, always visible.

Shows for each AI tool:

- `5-HOUR` usage
- `WEEKLY` usage
- Reset timers for both limits

![AI Usage Widget Preview](./assets/widget-screenshot.png)

## Features

- Pixel-style floating widget
- Side-by-side **Claude** + **Codex** panels
- Toggle each panel on/off from settings
- Frameless transparent window
- Draggable and always-on-top
- System tray integration
- Auto-refresh from local auth/session files

## Requirements

- Windows
- Node.js 24+
- Codex panel: active Codex login in `~/.codex/auth.json`
- Claude panel: active claude.ai session (login via the widget)

## Quick Start

```powershell
git clone https://github.com/lumatic2/ai-usage-widget.git
cd ai-usage-widget
npm install
npm start
```

## Run Manually

```powershell
cd ai-usage-widget
npm start
```

Or:

```powershell
.\start_ai_usage_widget.ps1
```

## Build a Windows `.exe`

```powershell
npm install
npm run dist
```

Output:

- `release/AI Usage Widget-win32-x64/AI Usage Widget.exe`
- `release/AI Usage Widget-win32-x64.zip`

Portable build — no installer required.

## Auto Start

Windows startup shortcut:

`C:\Users\1\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\AI Usage Widget.lnk`

After login, the widget starts automatically.

## Settings

Runtime settings are stored at:

`C:\Users\1\AppData\Roaming\ai-usage-widget\widget\settings.json`

| Setting | Description |
|---|---|
| `displayMode` | `used` or `left` (default: `used`) |
| `refreshIntervalMs` | Refresh interval in milliseconds |
| `enableUsageAlerts` | `true` / `false` |
| `usageAlertThresholds` | Array of percentages, e.g. `[30,60,80,90]` |
| `openOnStartup` | Open widget at Windows login |
| `showClaude` | Show/hide Claude panel |
| `showCodex` | Show/hide Codex panel |
| `fetchTimeoutMs` | Request timeout |
| `fetchRetries` | Retry count on failure |
| `sessionScanTtlMs` | Session scan cache TTL |

## How It Works

**Codex panel**
- Reads usage from `https://chatgpt.com/backend-api/wham/usage`
- Uses local auth file at `~/.codex/auth.json`
- Reads recent session data from `~/.codex/sessions`

**Claude panel**
- Reads usage from the Claude API via Bearer token
- Fetches org UUID dynamically from your claude.ai session
- Prompts login if no session is found

No tokens are stored in this repository. Credentials stay on the local machine.

## Security

- This repository does **not** contain your tokens or auth files
- The app reads local auth files at runtime only
- Credentials are never written back to disk
- Only run code you trust — anyone with local machine access could read the same auth files

## Development

Important files:

- `main.js`
- `preload.js`
- `renderer/index.html`
- `renderer/styles.css`
- `renderer/renderer.js`
- `scripts/capture-preview.js`

## License

MIT
