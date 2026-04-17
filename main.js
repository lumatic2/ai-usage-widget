const { app, BrowserWindow, Menu, Tray, nativeImage, ipcMain, screen, Notification, session } = require('electron');
const fs = require('fs');
const path = require('path');
const https = require('https');
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
const CLAUDE_CACHE_TTL_MS = 5 * 60 * 1000;
const DEFAULT_SETTINGS = {
  width: 780,
  height: 320,
  x: 40,
  y: 40,
  alwaysOnTop: true,
  openOnStartup: true,
  showClaude: true,
  showCodex: true,
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
let claudeUsageAlertState = {
  primary: { lastValue: null, notifiedThresholds: new Set() },
  secondary: { lastValue: null, notifiedThresholds: new Set() }
};
let lastGoodState = null;
let claudeLastGoodState = null;

function getCodexHome() {
  const envRoot = (process.env.CODEX_HOME || '').trim();
  return envRoot ? envRoot : path.join(process.env.USERPROFILE || app.getPath('home'), '.codex');
}

function getClaudeHome() {
  return path.join(process.env.USERPROFILE || app.getPath('home'), '.claude');
}

function getAppDataDir() {
  const dir = path.join(app.getPath('userData'), 'widget');
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function getSettingsPath() {
  return path.join(getAppDataDir(), 'settings.json');
}

function getLastErrorPath() {
  return path.join(getAppDataDir(), 'last-error.txt');
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
  merged.showClaude = Boolean(merged.showClaude);
  merged.showCodex = Boolean(merged.showCodex);
  if (!merged.showClaude && !merged.showCodex) {
    merged.showClaude = true;
  }
  return merged;
}

function clampInt(value, min, max, fallback) {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return fallback;
  }
  return Math.min(Math.max(Math.round(num), min), max);
}

function getPublicSettings(settings) {
  return {
    displayMode: settings.displayMode,
    showClaude: settings.showClaude,
    showCodex: settings.showCodex,
    enableUsageAlerts: settings.enableUsageAlerts,
    usageAlertThresholds: settings.usageAlertThresholds,
    refreshIntervalMs: settings.refreshIntervalMs,
    openOnStartup: settings.openOnStartup,
    alwaysOnTop: settings.alwaysOnTop,
    fetchTimeoutMs: settings.fetchTimeoutMs,
    fetchRetries: settings.fetchRetries,
    sessionScanTtlMs: settings.sessionScanTtlMs
  };
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

function createEmptyClaudeState(overrides = {}) {
  return {
    planType: 'CLAUDE',
    primary: { usedPercent: null, resetAfterSeconds: null },
    secondary: { usedPercent: null, resetAfterSeconds: null },
    isConfigured: false,
    needsLogin: false,
    isCached: false,
    error: null,
    ...overrides
  };
}

function loadClaudeCredentials() {
  const credentialsPath = path.join(getClaudeHome(), '.credentials.json');
  if (!fs.existsSync(credentialsPath)) {
    return null;
  }

  const payload = JSON.parse(fs.readFileSync(credentialsPath, 'utf8'));
  const oauth = payload.claudeAiOauth || {};
  const accessToken = String(oauth.accessToken || '').trim();
  if (!accessToken) {
    return null;
  }
  const organizationUuid = String(payload.organizationUuid || '').trim() || null;

  return { accessToken, organizationUuid };
}

function nodeHttpsGet(url, headers) {
  return new Promise((resolve, reject) => {
    const req = https.request(url, { headers }, (res) => {
      let body = '';
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => resolve({ status: res.statusCode, body }));
    });
    req.on('error', reject);
    req.end();
  });
}

async function fetchOrgUuidWithToken(accessToken) {
  try {
    const cached = runtimeSettings?.cachedOrgUuid || null;
    if (cached) return cached;

    const { status, body } = await nodeHttpsGet('https://claude.ai/api/organizations', {
      Authorization: `Bearer ${accessToken}`,
      Accept: 'application/json',
      'User-Agent': CLAUDE_CHROME_USER_AGENT
    });
    if (status < 200 || status >= 300) return null;
    const data = JSON.parse(body);
    const orgId = Array.isArray(data)
      ? String(data[0]?.uuid || '').trim()
      : String(data?.uuid || '').trim();
    if (!orgId) return null;

    const settings = runtimeSettings || loadSettings();
    const next = sanitizeSettings({ ...settings, cachedOrgUuid: orgId });
    runtimeSettings = next;
    saveSettings(next);
    return orgId;
  } catch {
    return null;
  }
}

async function fetchClaudeUsageWithToken(accessToken, orgId) {
  const usageUrl = `https://claude.ai/api/organizations/${encodeURIComponent(orgId)}/usage`;
  const timeoutMs = Math.max(15000, runtimeSettings?.fetchTimeoutMs ?? DEFAULT_SETTINGS.fetchTimeoutMs);
  const { status, body } = await Promise.race([
    nodeHttpsGet(usageUrl, {
      Authorization: `Bearer ${accessToken}`,
      Accept: 'application/json',
      'User-Agent': CLAUDE_CHROME_USER_AGENT
    }),
    new Promise((_, reject) => setTimeout(() => reject(new Error('Claude usage request timed out.')), timeoutMs))
  ]);
  let payload = null;
  try { payload = body ? JSON.parse(body) : null; } catch { payload = null; }
  const errorType = String(payload?.error?.type || payload?.type || '').toLowerCase();
  const hasPermissionError = errorType === 'permission_error';
  if (status === 401 || status === 403 || hasPermissionError) {
    const error = new Error('Claude session expired. Please log in again.');
    error.code = 'SESSION_EXPIRED';
    throw error;
  }
  if (status < 200 || status >= 300) {
    throw new Error(`Claude usage request failed: ${status || 'unknown'}`);
  }
  if (!payload || typeof payload !== 'object') {
    throw new Error('Claude usage response was invalid.');
  }
  return payload;
}

function loadClaudeSessionKey() {
  try {
    const settingsPath = getSettingsPath();
    if (!fs.existsSync(settingsPath)) {
      return null;
    }
    const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
    const sessionKey = String(settings.claudeSessionKey || '').trim();
    return sessionKey || null;
  } catch {
    return null;
  }
}

function saveClaudeSessionKey(key) {
  const sessionKey = String(key || '').trim();
  const settings = runtimeSettings || loadSettings();
  const nextSettings = { ...settings, claudeSessionKey: sessionKey };
  saveSettings(nextSettings);
  runtimeSettings = sanitizeSettings(nextSettings);
}

function clearClaudeSessionKey() {
  const settings = runtimeSettings || loadSettings();
  if (!Object.prototype.hasOwnProperty.call(settings, 'claudeSessionKey')) {
    return;
  }
  const nextSettings = { ...settings };
  delete nextSettings.claudeSessionKey;
  saveSettings(nextSettings);
  runtimeSettings = sanitizeSettings(nextSettings);
}

function normalizeClaudeUtilization(value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return null;
  }
  // Claude usage API returns utilization on a 0–100 percentage scale
  // (e.g. five_hour.utilization = 8.0 for 8%). An earlier heuristic
  // multiplied values ≤ 1 by 100, which incorrectly flipped real
  // low-percentage readings (e.g. 1.0% → 100%).
  return Math.max(0, Math.min(Math.round(numeric), 100));
}

function parseClaudeWindow(windowPayload) {
  if (!windowPayload || typeof windowPayload !== 'object') {
    return { usedPercent: null, resetAfterSeconds: null };
  }

  const usedPercent = normalizeClaudeUtilization(windowPayload.utilization);
  const resetTimestamp = Date.parse(windowPayload.resets_at || '');
  const resetAfterSeconds = Number.isFinite(resetTimestamp)
    ? Math.max(0, Math.round((resetTimestamp - Date.now()) / 1000))
    : null;

  return { usedPercent, resetAfterSeconds };
}

const CLAUDE_LOGIN_PARTITION = 'persist:claude-login';
const CLAUDE_LOGIN_URL = 'https://claude.ai';
const CLAUDE_CHROME_USER_AGENT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';

async function openClaudeLoginWindow() {
  const loginSession = session.fromPartition(CLAUDE_LOGIN_PARTITION);
  const loginWindow = new BrowserWindow({
    width: 800,
    height: 700,
    show: true,
    autoHideMenuBar: true,
    webPreferences: {
      partition: CLAUDE_LOGIN_PARTITION
    }
  });

  loginWindow.webContents.setUserAgent(CLAUDE_CHROME_USER_AGENT);

  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (handler, value) => {
      if (settled) {
        return;
      }
      settled = true;
      clearInterval(cookiePoller);
      clearTimeout(timeoutId);
      loginWindow.removeAllListeners('closed');
      if (!loginWindow.isDestroyed()) {
        loginWindow.close();
      }
      handler(value);
    };

    const cookiePoller = setInterval(async () => {
      try {
        const cookies = await loginSession.cookies.get({ url: CLAUDE_LOGIN_URL, name: 'sessionKey' });
        const sessionKey = String(cookies[0]?.value || '').trim();
        if (sessionKey) {
          saveClaudeSessionKey(sessionKey);
          // Also try to capture org UUID from the page context.
          try {
            const orgResult = await loginWindow.webContents.executeJavaScript(
              `fetch('https://claude.ai/api/organizations', { credentials: 'include', headers: { Accept: 'application/json' } }).then(r => r.json()).catch(() => null)`,
              true
            );
            const orgId = Array.isArray(orgResult)
              ? String(orgResult[0]?.uuid || '').trim()
              : String(orgResult?.uuid || '').trim();
            if (orgId) {
              const settings = runtimeSettings || loadSettings();
              const next = sanitizeSettings({ ...settings, cachedOrgUuid: orgId });
              runtimeSettings = next;
              saveSettings(next);
            }
          } catch {
            // Non-fatal: org UUID capture failed, will retry on next usage fetch.
          }
          finish(resolve, sessionKey);
        }
      } catch {
        // Ignore transient cookie access errors while login window is active.
      }
    }, 1000);

    const timeoutId = setTimeout(() => {
      finish(reject, new Error('Claude login timed out'));
    }, 3 * 60 * 1000);

    loginWindow.on('closed', () => {
      if (!settled) {
        finish(reject, new Error('Claude login window closed.'));
      }
    });

    loginWindow.loadURL(CLAUDE_LOGIN_URL).catch((error) => {
      finish(reject, error);
    });
  });
}

async function fetchClaudeUsageWithCookie(sessionKey, organizationUuid) {
  const usageUrl = `https://claude.ai/api/organizations/${encodeURIComponent(organizationUuid)}/usage`;
  const hiddenWindow = new BrowserWindow({
    show: false,
    webPreferences: {
      partition: CLAUDE_LOGIN_PARTITION
    }
  });
  hiddenWindow.webContents.setUserAgent(CLAUDE_CHROME_USER_AGENT);

  const loginSession = session.fromPartition(CLAUDE_LOGIN_PARTITION);
  const timeoutMs = Math.max(15000, runtimeSettings?.fetchTimeoutMs ?? DEFAULT_SETTINGS.fetchTimeoutMs);

  try {
    await loginSession.cookies.set({
      url: CLAUDE_LOGIN_URL,
      name: 'sessionKey',
      value: sessionKey,
      domain: 'claude.ai',
      path: '/',
      secure: true,
      httpOnly: true
    });

    await hiddenWindow.loadURL(CLAUDE_LOGIN_URL);

    const result = await Promise.race([
      hiddenWindow.webContents.executeJavaScript(
        `
          fetch(${JSON.stringify(usageUrl)}, {
            method: 'GET',
            credentials: 'include',
            headers: { Accept: 'application/json' }
          }).then(async (response) => ({
            status: response.status,
            body: await response.text()
          }))
        `,
        true
      ),
      new Promise((_, reject) => {
        setTimeout(() => reject(new Error('Claude usage request timed out.')), timeoutMs);
      })
    ]);

    const status = Number(result?.status || 0);
    const body = String(result?.body || '');
    let payload = null;
    try {
      payload = body ? JSON.parse(body) : null;
    } catch {
      payload = null;
    }

    const errorType = String(payload?.error?.type || payload?.type || '').toLowerCase();
    const errorMessage = String(payload?.error?.message || payload?.message || '').toLowerCase();
    const hasPermissionError = errorType === 'permission_error' || errorMessage.includes('permission_error');

    if (status === 401 || status === 403 || hasPermissionError) {
      const error = new Error('Claude session expired. Please log in again.');
      error.code = 'SESSION_EXPIRED';
      throw error;
    }
    if (status < 200 || status >= 300) {
      throw new Error(`Claude usage request failed: ${status || 'unknown'}`);
    }
    if (!payload || typeof payload !== 'object') {
      throw new Error('Claude usage response was invalid.');
    }

    return payload;
  } finally {
    if (!hiddenWindow.isDestroyed()) {
      hiddenWindow.destroy();
    }
  }
}

async function fetchClaudeUsage() {
  const credentials = loadClaudeCredentials();
  if (!credentials) {
    claudeLastGoodState = null;
    return createEmptyClaudeState();
  }

  const { accessToken } = credentials;
  let orgId = credentials.organizationUuid;

  // If orgId is missing from credentials, check settings cache then try Bearer fetch.
  if (!orgId) {
    orgId = await fetchOrgUuidWithToken(accessToken);
  }
  if (!orgId) {
    // Credentials exist but org UUID unavailable — prompt web login to resolve.
    return createEmptyClaudeState({ isConfigured: true, needsLogin: true });
  }

  // Invalidate the cache early if the 5-hour window has already reset since
  // the state was fetched — prevents stale 100% readings after a window rollover.
  if (claudeLastGoodState) {
    const elapsedSec = (Date.now() - claudeLastGoodState.fetchedAt) / 1000;
    const primaryResetAfter = claudeLastGoodState.state?.primary?.resetAfterSeconds;
    if (typeof primaryResetAfter === 'number' && primaryResetAfter >= 0 && elapsedSec >= primaryResetAfter) {
      console.log('[Claude] Cache invalidated: 5-hour window has reset.');
      claudeLastGoodState = null;
    }
  }

  const now = Date.now();
  const cachedState = claudeLastGoodState;
  if (cachedState && now - cachedState.fetchedAt < CLAUDE_CACHE_TTL_MS) {
    return { ...cachedState.state, isConfigured: true, isCached: true };
  }

  try {
    console.log('[Claude] Fetching usage (bearer)...');
    const payload = await fetchClaudeUsageWithToken(accessToken, orgId);
    console.log('[Claude] Got payload:', JSON.stringify(payload).substring(0, 300));
    const state = createEmptyClaudeState({
      planType: 'CLAUDE',
      primary: parseClaudeWindow(payload.five_hour),
      secondary: parseClaudeWindow(payload.seven_day),
      isConfigured: true,
      isCached: false
    });
    claudeLastGoodState = { fetchedAt: now, state };
    return state;
  } catch (bearerError) {
    console.warn('[Claude] Bearer fetch failed, trying cookie fallback:', bearerError.message);

    // Fall back to cookie-based approach.
    const sessionKey = loadClaudeSessionKey();
    if (!sessionKey) {
      if (bearerError.code === 'SESSION_EXPIRED') {
        return createEmptyClaudeState({ isConfigured: true, needsLogin: true });
      }
      return createEmptyClaudeState({ isConfigured: true, needsLogin: true });
    }

    try {
      console.log('[Claude] Fetching usage (cookie)...');
      const payload = await fetchClaudeUsageWithCookie(sessionKey, orgId);
      console.log('[Claude] Got payload:', JSON.stringify(payload).substring(0, 300));
      const state = createEmptyClaudeState({
        planType: 'CLAUDE',
        primary: parseClaudeWindow(payload.five_hour),
        secondary: parseClaudeWindow(payload.seven_day),
        isConfigured: true,
        isCached: false
      });
      claudeLastGoodState = { fetchedAt: now, state };
      return state;
    } catch (cookieError) {
      console.error('[Claude] Cookie fetch error:', cookieError instanceof Error ? cookieError.message : String(cookieError));
      if (cookieError.code === 'SESSION_EXPIRED') {
        clearClaudeSessionKey();
        return createEmptyClaudeState({ isConfigured: true, needsLogin: true, error: 'Claude session expired. Please log in again.' });
      }
      if (claudeLastGoodState) {
        return { ...claudeLastGoodState.state, isConfigured: true, isCached: true, error: cookieError instanceof Error ? cookieError.message : String(cookieError) };
      }
      return createEmptyClaudeState({ isConfigured: true, error: cookieError instanceof Error ? cookieError.message : String(cookieError) });
    }
  }
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
  const displayMode = runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode;
  let sessionLabel = 'Recent session';
  try {
    sessionLabel = loadSessionLabel();
  } catch (error) {
    // Session parsing failure should not block usage display.
    sessionLabel = 'Recent session';
  }
  const [usageResult, claudeResult] = await Promise.allSettled([fetchUsage(), fetchClaudeUsage()]);
  if (usageResult.status !== 'fulfilled' && claudeResult.status !== 'fulfilled') {
    throw usageResult.reason || claudeResult.reason || new Error('Unable to refresh usage.');
  }

  const usage = usageResult.status === 'fulfilled'
    ? usageResult.value
    : {
        planType: 'CODEX',
        primary: { usedPercent: null, resetAfterSeconds: null },
        secondary: { usedPercent: null, resetAfterSeconds: null },
        error: usageResult.reason instanceof Error ? usageResult.reason.message : String(usageResult.reason)
      };

  const claude = claudeResult.status === 'fulfilled'
    ? claudeResult.value
    : createEmptyClaudeState({
        isConfigured: true,
        error: claudeResult.reason instanceof Error ? claudeResult.reason.message : String(claudeResult.reason)
      });

  return {
    ...usage,
    claude,
    sessionLabel,
    displayMode,
    error: usage.error || null
  };
}

function sendState(state) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('widget-state', state);
  }
  if (tray) {
    const displayMode = state.displayMode || runtimeSettings?.displayMode || DEFAULT_SETTINGS.displayMode;
    const codexPrimaryPercent = computeDisplayPercent(state.primary?.usedPercent, displayMode);
    const codexSecondaryPercent = computeDisplayPercent(state.secondary?.usedPercent, displayMode);
    const claudePrimaryPercent = computeDisplayPercent(state.claude?.primary?.usedPercent, displayMode);
    const claudeSecondaryPercent = computeDisplayPercent(state.claude?.secondary?.usedPercent, displayMode);
    const lines = [
      'Codex Widget',
      state.error
        ? `CODEX ${state.error}`
        : `CODEX ${modeLabel(displayMode)} 5H ${codexPrimaryPercent}% | WEEK ${codexSecondaryPercent}%`
    ];
    if (state.claude?.isConfigured) {
      lines.push(
        state.claude.error && !state.claude.isCached
          ? `CLAUDE ${state.claude.error}`
          : `CLAUDE ${modeLabel(displayMode)} 5H ${Number.isFinite(claudePrimaryPercent) ? claudePrimaryPercent : '--'}% | WEEK ${Number.isFinite(claudeSecondaryPercent) ? claudeSecondaryPercent : '--'}%${state.claude.isCached ? ' (cached)' : ''}`
      );
    } else {
      lines.push('CLAUDE Not configured');
    }
    tray.setToolTip(lines.join('\n'));
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

function applyUpdatedSettings(partial) {
  const previous = runtimeSettings || loadSettings();
  const next = sanitizeSettings({ ...previous, ...partial });
  runtimeSettings = next;
  saveSettings(next);
  applyOpenOnStartupSetting(next);

  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.setAlwaysOnTop(next.alwaysOnTop);
  }
  if (refreshTimer && previous.refreshIntervalMs !== next.refreshIntervalMs) {
    clearInterval(refreshTimer);
    refreshTimer = setInterval(refreshState, next.refreshIntervalMs);
  }
  return getPublicSettings(next);
}

function notifyUsageThresholds(state) {
  if (!runtimeSettings?.enableUsageAlerts || !Array.isArray(runtimeSettings?.usageAlertThresholds)) {
    return;
  }

  if (!state.error) {
    checkWindowThreshold(usageAlertState, 'primary', state.primary?.usedPercent, 'CODEX', '5-HOUR');
    checkWindowThreshold(usageAlertState, 'secondary', state.secondary?.usedPercent, 'CODEX', 'WEEKLY');
  }

  if (state.claude?.isConfigured && !state.claude.error) {
    checkWindowThreshold(claudeUsageAlertState, 'primary', state.claude.primary?.usedPercent, 'CLAUDE', '5-HOUR');
    checkWindowThreshold(claudeUsageAlertState, 'secondary', state.claude.secondary?.usedPercent, 'CLAUDE', 'WEEKLY');
  }
}

function checkWindowThreshold(alertState, key, value, providerLabel, windowLabel) {
  const currentValue = Number(value);
  const windowState = alertState[key];
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
        title: `${providerLabel} Usage Alert (${windowLabel})`,
        body: `Usage reached ${threshold}% (${Math.round(currentValue)}%).`
      }).show();
    }
  }

  windowState.lastValue = currentValue;
}

async function refreshState() {
  try {
    const state = await buildWidgetState();
    lastGoodState = state;
    try {
      if (fs.existsSync(getLastErrorPath())) {
        fs.unlinkSync(getLastErrorPath());
      }
    } catch {
      // Ignore best-effort cleanup failures.
    }
    notifyUsageThresholds(state);
    sendState(state);
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    try {
      fs.writeFileSync(getLastErrorPath(), `${new Date().toISOString()} ${errorMessage}\n`, 'utf8');
    } catch {
      // Ignore best-effort logging failures.
    }
    if (lastGoodState) {
      sendState({
        ...lastGoodState,
        displayMode: runtimeSettings?.displayMode ?? lastGoodState.displayMode ?? DEFAULT_SETTINGS.displayMode,
        error: errorMessage
      });
      return;
    }
    sendState({
      planType: 'CODEX',
      primary: { usedPercent: null, resetAfterSeconds: null },
      secondary: { usedPercent: null, resetAfterSeconds: null },
      claude: createEmptyClaudeState(),
      sessionLabel: 'Offline',
      displayMode: runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode,
      error: errorMessage
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
    const state = await buildWidgetState();
    lastGoodState = state;
    return state;
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    if (lastGoodState) {
      return {
        ...lastGoodState,
        displayMode: runtimeSettings?.displayMode ?? lastGoodState.displayMode ?? DEFAULT_SETTINGS.displayMode,
        error: errorMessage
      };
    }
    return {
      planType: 'CODEX',
      primary: { usedPercent: null, resetAfterSeconds: null },
      secondary: { usedPercent: null, resetAfterSeconds: null },
      claude: createEmptyClaudeState(),
      sessionLabel: 'Offline',
      displayMode: runtimeSettings?.displayMode ?? DEFAULT_SETTINGS.displayMode,
      error: errorMessage
    };
  }
});

ipcMain.handle('widget:get-settings', async () => {
  const settings = runtimeSettings || loadSettings();
  return getPublicSettings(settings);
});

ipcMain.handle('widget:update-settings', async (_event, partial) => {
  const safePartial = partial && typeof partial === 'object' ? partial : {};
  const nextSettings = applyUpdatedSettings(safePartial);
  await refreshState();
  return nextSettings;
});

ipcMain.handle('widget:set-display-mode', async (_event, mode) => {
  const nextMode = normalizeDisplayMode(mode);
  applyUpdatedSettings({ displayMode: nextMode });
  await refreshState();
  return { displayMode: nextMode };
});

ipcMain.handle('widget:refresh-now', async () => {
  await refreshState();
  return true;
});

ipcMain.handle('widget:claude-login', async () => {
  try {
    const sessionKey = await openClaudeLoginWindow();
    saveClaudeSessionKey(sessionKey);
    claudeLastGoodState = null;
    await refreshState();
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error)
    };
  }
});

ipcMain.handle('widget:claude-logout', async () => {
  clearClaudeSessionKey();
  claudeLastGoodState = null;
  await refreshState();
  return true;
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
