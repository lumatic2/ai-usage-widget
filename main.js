const { app, BrowserWindow, Menu, Tray, nativeImage, ipcMain, screen, Notification } = require('electron');
const fs = require('fs');
const path = require('path');
const {
  computeDisplayPercent,
  didUsageWindowReset,
  getCrossedThresholds,
  modeLabel,
  normalizeDisplayMode,
  sanitizeThresholds
} = require('./lib/widget-core');

const APP_NAME = 'Codex Pixel Widget';
const CHATGPT_USAGE_URL = 'https://chatgpt.com/backend-api/wham/usage';
const ROLLOUT_TAIL_READ_BYTES = 256 * 1024;
const DEFAULT_SETTINGS = {
  width: 420,
  height: 320,
  x: 40,
  y: 40,
  alwaysOnTop: true,
  openOnStartup: true,
  refreshIntervalMs: 60000,
  displayMode: 'used',
  usageAlertThresholds: [30, 60, 80, 90],
  enableUsageAlerts: true,
  fetchTimeoutMs: 8000,
  fetchRetries: 2,
  sessionScanTtlMs: 5 * 60 * 1000
};

let mainWindow = null;
let tray = null;
let refreshTimer = null;
let runtimeSettings = null;
let sessionCache = {
  latestPath: null,
  latestPathMtimeMs: 0,
  latestMessageMtimeMs: 0,
  sessionLabel: 'No recent session',
  lastScanAt: 0
};
let usageAlertState = {
  primary: { lastValue: null, notifiedThresholds: new Set() },
  secondary: { lastValue: null, notifiedThresholds: new Set() }
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
    return sanitizeSettings({ ...DEFAULT_SETTINGS, ...raw });
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings) {
  fs.writeFileSync(getSettingsPath(), JSON.stringify(settings, null, 2), 'utf8');
}

function sanitizeSettings(raw) {
  const merged = { ...DEFAULT_SETTINGS, ...raw };
  merged.width = Math.max(DEFAULT_SETTINGS.width, Number(merged.width || DEFAULT_SETTINGS.width));
  merged.height = Math.max(DEFAULT_SETTINGS.height, Number(merged.height || DEFAULT_SETTINGS.height));
  merged.refreshIntervalMs = clampInt(merged.refreshIntervalMs, 10000, 10 * 60 * 1000, DEFAULT_SETTINGS.refreshIntervalMs);
  merged.fetchTimeoutMs = clampInt(merged.fetchTimeoutMs, 2000, 60000, DEFAULT_SETTINGS.fetchTimeoutMs);
  merged.fetchRetries = clampInt(merged.fetchRetries, 0, 5, DEFAULT_SETTINGS.fetchRetries);
  merged.sessionScanTtlMs = clampInt(merged.sessionScanTtlMs, 30000, 60 * 60 * 1000, DEFAULT_SETTINGS.sessionScanTtlMs);
  merged.displayMode = normalizeDisplayMode(merged.displayMode);
  merged.usageAlertThresholds = sanitizeThresholds(merged.usageAlertThresholds);
  merged.enableUsageAlerts = Boolean(merged.enableUsageAlerts);
  merged.openOnStartup = Boolean(merged.openOnStartup);
  merged.alwaysOnTop = Boolean(merged.alwaysOnTop);
  return merged;
}

function clampInt(value, min, max, fallback) {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return fallback;
  }
  return Math.min(Math.max(Math.round(num), min), max);
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
  const timeoutMs = runtimeSettings?.fetchTimeoutMs ?? DEFAULT_SETTINGS.fetchTimeoutMs;
  const maxRetries = runtimeSettings?.fetchRetries ?? DEFAULT_SETTINGS.fetchRetries;
  let lastError = null;
  for (let attempt = 0; attempt <= maxRetries; attempt += 1) {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetch(CHATGPT_USAGE_URL, { headers, signal: controller.signal });
      if (response.status === 401 || response.status === 403) {
        throw new Error('Codex login is expired. Please login again.');
      }
      if (!response.ok) {
        const canRetryStatus = response.status === 429 || response.status >= 500;
        if (canRetryStatus && attempt < maxRetries) {
          await sleep(400 * (attempt + 1));
          continue;
        }
        throw new Error(`Usage request failed: ${response.status}`);
      }
      return response;
    } catch (error) {
      lastError = error;
      const canRetry = isRetryableFetchError(error) && attempt < maxRetries;
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
  const sessionScanTtlMs = runtimeSettings?.sessionScanTtlMs ?? DEFAULT_SETTINGS.sessionScanTtlMs;
  const needsScan = !sessionCache.latestPath || now - sessionCache.lastScanAt >= sessionScanTtlMs;

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
  const displayMode = runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode;
  return {
    ...usage,
    sessionLabel: loadSessionLabel(),
    displayMode
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
      const displayMode = state.displayMode || runtimeSettings?.displayMode || DEFAULT_SETTINGS.displayMode;
      const primaryPercent = computeDisplayPercent(state.primary?.usedPercent, displayMode);
      const secondaryPercent = computeDisplayPercent(state.secondary?.usedPercent, displayMode);
      tray.setToolTip(`Codex Widget\n${modeLabel(displayMode)} 5H ${primaryPercent}% | WEEK ${secondaryPercent}%`);
    }
  }
}

function applyOpenOnStartupSetting(settings) {
  if (!app.isPackaged) {
    return;
  }
  app.setLoginItemSettings({
    openAtLogin: Boolean(settings.openOnStartup),
    path: process.execPath
  });
}

function notifyUsageThresholds(state) {
  if (!runtimeSettings?.enableUsageAlerts || !Array.isArray(runtimeSettings?.usageAlertThresholds)) {
    return;
  }
  if (state.error) {
    return;
  }

  checkWindowThreshold('primary', state.primary?.usedPercent, '5-HOUR');
  checkWindowThreshold('secondary', state.secondary?.usedPercent, 'WEEKLY');
}

function checkWindowThreshold(key, value, windowLabel) {
  const currentValue = Number(value);
  const windowState = usageAlertState[key];
  if (!Number.isFinite(currentValue)) {
    windowState.lastValue = null;
    windowState.notifiedThresholds.clear();
    return;
  }

  if (didUsageWindowReset(windowState.lastValue, currentValue)) {
    windowState.notifiedThresholds.clear();
  }

  const crossed = getCrossedThresholds(windowState.lastValue, currentValue, runtimeSettings.usageAlertThresholds);
  for (const threshold of crossed) {
    if (windowState.notifiedThresholds.has(threshold)) {
      continue;
    }
    windowState.notifiedThresholds.add(threshold);
    if (Notification.isSupported()) {
      new Notification({
        title: `Codex Usage Alert (${windowLabel})`,
        body: `Usage reached ${threshold}% (${Math.round(currentValue)}%).`
      }).show();
    }
  }

  windowState.lastValue = currentValue;
}

async function refreshState() {
  try {
    const state = await buildWidgetState();
    notifyUsageThresholds(state);
    sendState(state);
  } catch (error) {
    sendState({
      planType: 'CODEX',
      primary: { usedPercent: null, resetAfterSeconds: null },
      secondary: { usedPercent: null, resetAfterSeconds: null },
      sessionLabel: 'Offline',
      displayMode: runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode,
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
  const settings = runtimeSettings || loadSettings();
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
    const nextSettings = sanitizeSettings({ ...(runtimeSettings || settings), x, y });
    runtimeSettings = nextSettings;
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
      displayMode: runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode,
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
  runtimeSettings = loadSettings();
  applyOpenOnStartupSetting(runtimeSettings);
  createWindow();
  createTray();
  refreshState();
  refreshTimer = setInterval(refreshState, runtimeSettings.refreshIntervalMs);
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
