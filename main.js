const { app, BrowserWindow, Menu, Tray, nativeImage, ipcMain, screen } = require('electron');
const fs = require('fs');
const path = require('path');

const APP_NAME = 'Codex Pixel Widget';
const CHATGPT_USAGE_URL = 'https://chatgpt.com/backend-api/wham/usage';
const USAGE_FETCH_TIMEOUT_MS = 8000;
const USAGE_FETCH_RETRIES = 2;
const SESSION_SCAN_TTL_MS = 5 * 60 * 1000;
const ROLLOUT_TAIL_READ_BYTES = 256 * 1024;
const DEFAULT_SETTINGS = {
  width: 420,
  height: 320,
  x: 40,
  y: 40,
  alwaysOnTop: true,
  openOnStartup: true,
  refreshIntervalMs: 60000
};

let mainWindow = null;
let tray = null;
let refreshTimer = null;
let sessionCache = {
  latestPath: null,
  latestPathMtimeMs: 0,
  latestMessageMtimeMs: 0,
  sessionLabel: 'No recent session',
  lastScanAt: 0
};

function getCodexHome() {
  const envRoot = (process.env.CODEX_HOME || '').trim();
  return envRoot ? envRoot : path.join(process.env.USERPROFILE || app.getPath('home'), '.codex');
}

function getAppDataDir() {
  const dir = path.join(app.getPath('userData'), 'widget');
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function getSettingsPath() {
  return path.join(getAppDataDir(), 'settings.json');
}

function loadSettings() {
  const settingsPath = getSettingsPath();
  if (!fs.existsSync(settingsPath)) {
    saveSettings(DEFAULT_SETTINGS);
    return { ...DEFAULT_SETTINGS };
  }

  try {
    const raw = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
    const merged = { ...DEFAULT_SETTINGS, ...raw };
    merged.width = Math.max(DEFAULT_SETTINGS.width, Number(merged.width || DEFAULT_SETTINGS.width));
    merged.height = Math.max(DEFAULT_SETTINGS.height, Number(merged.height || DEFAULT_SETTINGS.height));
    return merged;
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings) {
  fs.writeFileSync(getSettingsPath(), JSON.stringify(settings, null, 2), 'utf8');
}

function clampBounds(bounds, settings) {
  const display = screen.getDisplayNearestPoint({ x: settings.x, y: settings.y });
  const workArea = display.workArea;
  const width = settings.width;
  const height = settings.height;
  const x = Math.min(Math.max(bounds.x, workArea.x), workArea.x + workArea.width - width);
  const y = Math.min(Math.max(bounds.y, workArea.y), workArea.y + workArea.height - height);
  return { x, y };
}

function parseWindow(windowPayload) {
  if (!windowPayload) {
    return { usedPercent: 0, resetAfterSeconds: null };
  }

  return {
    usedPercent: Number(windowPayload.used_percent || 0),
    resetAfterSeconds: windowPayload.reset_after_seconds ?? null
  };
}

function loadAuthPayload() {
  const authPath = path.join(getCodexHome(), 'auth.json');
  if (!fs.existsSync(authPath)) {
    throw new Error(`Codex auth file not found: ${authPath}`);
  }
  return JSON.parse(fs.readFileSync(authPath, 'utf8'));
}

async function fetchUsage() {
  const authPayload = loadAuthPayload();
  const tokens = authPayload.tokens || {};
  const accessToken = tokens.access_token || authPayload.OPENAI_API_KEY;
  if (!accessToken) {
    throw new Error('Codex access token is missing from auth.json');
  }

  const headers = {
    'Accept': 'application/json',
    'Authorization': `Bearer ${accessToken}`,
    'User-Agent': 'CodexPixelWidget/0.1.0'
  };

  if (tokens.account_id) {
    headers['ChatGPT-Account-Id'] = tokens.account_id;
  }

  const response = await fetchUsageResponse(headers);

  const payload = await response.json();
  return {
    planType: String(payload.plan_type || 'unknown').toUpperCase(),
    primary: parseWindow(payload.rate_limit?.primary_window),
    secondary: parseWindow(payload.rate_limit?.secondary_window)
  };
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isRetryableFetchError(error) {
  if (!error || typeof error !== 'object') {
    return false;
  }
  const message = String(error.message || '').toLowerCase();
  return (
    error.name === 'AbortError' ||
    message.includes('fetch failed') ||
    message.includes('network') ||
    message.includes('socket') ||
    message.includes('timed out')
  );
}

async function fetchUsageResponse(headers) {
  let lastError = null;
  for (let attempt = 0; attempt <= USAGE_FETCH_RETRIES; attempt += 1) {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), USAGE_FETCH_TIMEOUT_MS);
    try {
      const response = await fetch(CHATGPT_USAGE_URL, { headers, signal: controller.signal });
      if (response.status === 401 || response.status === 403) {
        throw new Error('Codex login is expired. Please login again.');
      }
      if (!response.ok) {
        const canRetryStatus = response.status === 429 || response.status >= 500;
        if (canRetryStatus && attempt < USAGE_FETCH_RETRIES) {
          await sleep(400 * (attempt + 1));
          continue;
        }
        throw new Error(`Usage request failed: ${response.status}`);
      }
      return response;
    } catch (error) {
      lastError = error;
      const canRetry = isRetryableFetchError(error) && attempt < USAGE_FETCH_RETRIES;
      if (canRetry) {
        await sleep(400 * (attempt + 1));
        continue;
      }
      if (error && error.name === 'AbortError') {
        throw new Error('Usage request timed out. Please try again.');
      }
      throw error;
    } finally {
      clearTimeout(timeoutId);
    }
  }

  if (lastError && lastError.name === 'AbortError') {
    throw new Error('Usage request timed out. Please try again.');
  }
  throw lastError || new Error('Usage request failed');
}

function findLatestRolloutFile() {
  const sessionsRoot = path.join(getCodexHome(), 'sessions');
  if (!fs.existsSync(sessionsRoot)) {
    return null;
  }

  let latestPath = null;
  let latestMtime = 0;
  const stack = [sessionsRoot];

  while (stack.length > 0) {
    const currentDir = stack.pop();
    const entries = fs.readdirSync(currentDir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(currentDir, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
        continue;
      }
      if (!entry.isFile() || !entry.name.startsWith('rollout-') || !entry.name.endsWith('.jsonl')) {
        continue;
      }
      const stat = fs.statSync(fullPath);
      if (stat.mtimeMs > latestMtime) {
        latestMtime = stat.mtimeMs;
        latestPath = fullPath;
      }
    }
  }

  return latestPath;
}

function readRolloutTail(filePath) {
  const stat = fs.statSync(filePath);
  if (stat.size <= 0) {
    return '';
  }
  const bytesToRead = Math.min(stat.size, ROLLOUT_TAIL_READ_BYTES);
  const start = stat.size - bytesToRead;
  const buffer = Buffer.alloc(bytesToRead);
  const fd = fs.openSync(filePath, 'r');
  try {
    fs.readSync(fd, buffer, 0, bytesToRead, start);
  } finally {
    fs.closeSync(fd);
  }
  return buffer.toString('utf8');
}

function findLatestUserMessage(lines) {
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    const line = lines[i];
    if (!line || !line.trim()) {
      continue;
    }
    try {
      const payload = JSON.parse(line);
      const itemPayload = payload.payload || {};
      if (itemPayload.type === 'user_message' && itemPayload.message) {
        return String(itemPayload.message).trim();
      }
    } catch {
      continue;
    }
  }
  return null;
}

function loadSessionLabelFromRollout(rolloutPath) {
  const tail = readRolloutTail(rolloutPath);
  let latestMessage = findLatestUserMessage(tail.split(/\r?\n/));
  if (!latestMessage) {
    // Fallback: if older lines contain the latest user message, scan full file once.
    const allLines = fs.readFileSync(rolloutPath, 'utf8').split(/\r?\n/);
    latestMessage = findLatestUserMessage(allLines);
  }
  return latestMessage;
}

function loadSessionLabel() {
  const now = Date.now();
  const needsScan = !sessionCache.latestPath || now - sessionCache.lastScanAt >= SESSION_SCAN_TTL_MS;

  if (needsScan) {
    const latestPath = findLatestRolloutFile();
    sessionCache.latestPath = latestPath;
    sessionCache.latestPathMtimeMs = latestPath ? fs.statSync(latestPath).mtimeMs : 0;
    sessionCache.lastScanAt = now;
    sessionCache.latestMessageMtimeMs = 0;
  }

  const rolloutPath = sessionCache.latestPath;
  if (!rolloutPath) {
    sessionCache.sessionLabel = 'No recent session';
    return 'No recent session';
  }

  const stat = fs.statSync(rolloutPath);
  const pathChanged = stat.mtimeMs !== sessionCache.latestPathMtimeMs;
  if (pathChanged) {
    sessionCache.latestPathMtimeMs = stat.mtimeMs;
    sessionCache.latestMessageMtimeMs = 0;
  }
  if (stat.mtimeMs === sessionCache.latestMessageMtimeMs) {
    return sessionCache.sessionLabel;
  }

  const latestMessage = loadSessionLabelFromRollout(rolloutPath);
  sessionCache.latestMessageMtimeMs = stat.mtimeMs;

  if (!latestMessage) {
    sessionCache.sessionLabel = 'Recent session';
    return 'Recent session';
  }

  const compact = latestMessage.replace(/\s+/g, ' ');
  sessionCache.sessionLabel = compact.length > 40 ? `${compact.slice(0, 39)}...` : compact;
  return sessionCache.sessionLabel;
}

async function buildWidgetState() {
  const usage = await fetchUsage();
  return {
    ...usage,
    sessionLabel: loadSessionLabel()
  };
}

function sendState(state) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('widget-state', state);
  }
  if (tray) {
    if (state.error) {
      tray.setToolTip(`Codex Widget\n${state.error}`);
    } else {
      const primaryPercent = Number.isFinite(state.primary?.usedPercent) ? Math.round(state.primary.usedPercent) : 0;
      const secondaryPercent = Number.isFinite(state.secondary?.usedPercent) ? Math.round(state.secondary.usedPercent) : 0;
      tray.setToolTip(`Codex Widget\n5H ${primaryPercent}% | WEEK ${secondaryPercent}%`);
    }
  }
}

async function refreshState() {
  try {
    const state = await buildWidgetState();
    sendState(state);
  } catch (error) {
    sendState({
      planType: 'CODEX',
      primary: { usedPercent: null, resetAfterSeconds: null },
      secondary: { usedPercent: null, resetAfterSeconds: null },
      sessionLabel: 'Offline',
      error: error instanceof Error ? error.message : String(error)
    });
  }
}

function createTray() {
  const svg = `
    <svg width="64" height="64" viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
      <rect x="8" y="8" width="48" height="48" rx="10" fill="#1a1620"/>
      <rect x="14" y="14" width="20" height="14" fill="#5d3d31"/>
      <rect x="18" y="28" width="12" height="16" fill="#f5db8b"/>
      <rect x="40" y="20" width="8" height="24" fill="#7ca5cf"/>
      <rect x="14" y="48" width="18" height="4" fill="#7ca5cf"/>
      <rect x="36" y="48" width="18" height="4" fill="#efc57d"/>
    </svg>
  `;
  const icon = nativeImage.createFromDataURL(`data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`);
  tray = new Tray(icon);
  const menu = Menu.buildFromTemplate([
    { label: 'Show Widget', click: () => mainWindow && mainWindow.show() },
    { label: 'Hide Widget', click: () => mainWindow && mainWindow.hide() },
    { type: 'separator' },
    { label: 'Open Settings Folder', click: () => require('electron').shell.openPath(getAppDataDir()) },
    { label: 'Quit', click: () => app.quit() }
  ]);
  tray.setContextMenu(menu);
  tray.setToolTip(APP_NAME);
  tray.on('double-click', () => mainWindow && mainWindow.show());
}

function createWindow() {
  const settings = loadSettings();
  const bounds = clampBounds({ x: settings.x, y: settings.y }, settings);

  mainWindow = new BrowserWindow({
    width: settings.width,
    height: settings.height,
    x: bounds.x,
    y: bounds.y,
    frame: false,
    transparent: true,
    resizable: false,
    hasShadow: false,
    skipTaskbar: true,
    alwaysOnTop: settings.alwaysOnTop,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js')
    }
  });

  mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));
  mainWindow.on('close', (event) => {
    if (!app.isQuiting) {
      event.preventDefault();
      mainWindow.hide();
    }
  });
  mainWindow.on('move', () => {
    const [x, y] = mainWindow.getPosition();
    const nextSettings = { ...loadSettings(), x, y };
    saveSettings(nextSettings);
  });
  mainWindow.on('show', () => refreshState());
}

ipcMain.handle('widget:get-initial-state', async () => {
  try {
    return await buildWidgetState();
  } catch (error) {
    return {
      planType: 'CODEX',
      primary: { usedPercent: null, resetAfterSeconds: null },
      secondary: { usedPercent: null, resetAfterSeconds: null },
      sessionLabel: 'Offline',
      error: error instanceof Error ? error.message : String(error)
    };
  }
});

ipcMain.on('widget:hide', () => {
  if (mainWindow) {
    mainWindow.hide();
  }
});

app.whenReady().then(() => {
  createWindow();
  createTray();
  refreshState();
  const settings = loadSettings();
  refreshTimer = setInterval(refreshState, settings.refreshIntervalMs);
});

app.on('window-all-closed', (event) => {
  event.preventDefault();
});

app.on('before-quit', () => {
  app.isQuiting = true;
  if (refreshTimer) {
    clearInterval(refreshTimer);
  }
});
