# Codex Pixel Widget

Minimal Windows desktop widget for OpenAI Codex usage.

It runs as a small transparent Electron window, stays on top, can be dragged anywhere, and shows:

- `CODEX`
- `5-HOUR` usage
- `WEEKLY` usage
- reset time tags for both limits

## Preview

- Pixel-style floating widget
- Frameless transparent window
- System tray integration
- Auto-refresh from local Codex auth/session files

## Requirements

- Windows
- Node.js 24+
- An active Codex login in `~/.codex/auth.json`

## Install

```powershell
git clone https://github.com/Mod41529/codex-pixel-widget.git
cd codex-pixel-widget
npm install
npm start
```

## Manual Start

```powershell
cd codex-pixel-widget
npm start
```

Or run:

```powershell
.\start_codex_widget.ps1
```

## Auto Start

Windows startup shortcut:

`C:\Users\1\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\Codex Widget.lnk`

After login, the widget should start automatically.

## Settings

Runtime settings are stored at:

`C:\Users\1\AppData\Roaming\codex-widget-electron\widget\settings.json`

Current settings include:

- window width / height
- window x / y position
- always-on-top
- refresh interval

## How It Works

- Reads Codex usage from `https://chatgpt.com/backend-api/wham/usage`
- Uses the local Codex auth file at `~/.codex/auth.json`
- Reads recent rollout/session data from `~/.codex/sessions`

No tokens are stored in this repository. Credentials stay on the local machine.

## Development

Project structure:

- `main.js`
- `preload.js`
- `renderer/index.html`
- `renderer/styles.css`
- `renderer/renderer.js`

## License

MIT
