// === Tauri shim — bridges legacy window.codexWidget API to Tauri invoke/event ===
(function () {
  const invoke = window.__TAURI__.core.invoke;
  const listen = window.__TAURI__.event.listen;

  // Wire 4-edge + 4-corner resize handles to Tauri's window-drag-resize API.
  document.addEventListener('mousedown', (e) => {
    const handle = e.target.closest && e.target.closest('[data-resize]');
    if (!handle) return;
    e.preventDefault();
    const dir = handle.dataset.resize;
    invoke('plugin:window|start_resize_dragging', { value: dir }).catch((err) => {
      console.error('startResizeDragging failed', err);
    });
  });
  window.codexWidget = {
    getInitialState: () => invoke('get_initial_state'),
    getSettings: () => invoke('get_settings'),
    updateSettings: (partial) => invoke('update_settings', { partial }),
    setDisplayMode: (mode) => invoke('set_display_mode', { mode }),
    refreshNow: () => invoke('refresh_now'),
    claudeLogin: () => invoke('claude_login'),
    claudeLogout: () => invoke('claude_logout'),
    acceptConsent: (showClaude, showCodex) => invoke('accept_consent', { showClaude, showCodex }),
    hide: () => invoke('hide_widget'),
    onState: (cb) => {
      let unlistenFn = null;
      listen('widget-state', (e) => cb(e.payload)).then((u) => { unlistenFn = u; });
      return () => unlistenFn && unlistenFn();
    }
  };
})();

const primaryValue = document.getElementById('primaryValue');
const secondaryValue = document.getElementById('secondaryValue');
const primaryProgress = document.getElementById('primaryProgress');
const secondaryProgress = document.getElementById('secondaryProgress');
const primaryReset = document.getElementById('primaryReset');
const secondaryReset = document.getElementById('secondaryReset');
const codexSection = document.getElementById('codexSection');
const codexPrimaryBar = document.getElementById('codexPrimaryBar');
const codexSecondaryBar = document.getElementById('codexSecondaryBar');
const claudeSection = document.getElementById('claudeSection');
const claudePrimaryBar = document.getElementById('claudePrimaryBar');
const claudeSecondaryBar = document.getElementById('claudeSecondaryBar');
const claudePrimaryValue = document.getElementById('claudePrimaryValue');
const claudeSecondaryValue = document.getElementById('claudeSecondaryValue');
const claudePrimaryProgress = document.getElementById('claudePrimaryProgress');
const claudeSecondaryProgress = document.getElementById('claudeSecondaryProgress');
const claudePrimaryReset = document.getElementById('claudePrimaryReset');
const claudeSecondaryReset = document.getElementById('claudeSecondaryReset');
const claudeStatusPill = document.getElementById('claudeStatusPill');
const claudeAccountTag = document.getElementById('claudeAccountTag');
const codexAccountTag = document.getElementById('codexAccountTag');
const claudeLoginWrap = document.getElementById('claudeLoginWrap');
const claudeLoginBtn = document.getElementById('claudeLoginBtn');
const hideButton = document.getElementById('hideButton');
const errorBanner = document.getElementById('errorBanner');
const errorText = document.getElementById('errorText');
const errorCloseButton = document.getElementById('errorCloseButton');
const claudeErrorBanner = document.getElementById('claudeErrorBanner');
const claudeErrorText = document.getElementById('claudeErrorText');
const claudeErrorCloseButton = document.getElementById('claudeErrorCloseButton');
const settingsToggleButton = document.getElementById('settingsToggleButton');
const settingsPanel = document.getElementById('settingsPanel');
const twoCol = document.querySelector('.two-col');
const claudeColumn = document.querySelector('.col--claude');
const codexColumn = document.querySelector('.col--codex');
const languageSelect = document.getElementById('languageSelect');
const displayModeSelect = document.getElementById('displayModeSelect');
const refreshSecondsInput = document.getElementById('refreshSecondsInput');
const uiScaleSelect = document.getElementById('uiScaleSelect');
const alertsEnabledInput = document.getElementById('alertsEnabledInput');
const alertThresholdsInput = document.getElementById('alertThresholdsInput');
const openOnStartupInput = document.getElementById('openOnStartupInput');
const showClaudeInput = document.getElementById('showClaudeInput');
const showCodexInput = document.getElementById('showCodexInput');
const settingsSaveButton = document.getElementById('settingsSaveButton');
const settingsRefreshButton = document.getElementById('settingsRefreshButton');
const claudeLogoutButton = document.getElementById('claudeLogoutButton');
const geminiColumn = document.querySelector('.col--gemini');
const geminiSection = document.getElementById('geminiSection');
const geminiQuotaList = document.getElementById('geminiQuotaList');
const geminiFallback = document.getElementById('geminiFallback');
const geminiTodayTag = document.getElementById('geminiTodayTag');
const geminiErrorBanner = document.getElementById('geminiErrorBanner');
const geminiErrorText = document.getElementById('geminiErrorText');
const geminiErrorCloseButton = document.getElementById('geminiErrorCloseButton');
const showGeminiInput = document.getElementById('showGeminiInput');
const I18N = {
  en: {
    'settings.language': 'Language',
    'settings.display': 'Display',
    'settings.displayUsed': 'USED',
    'settings.displayLeft': 'LEFT',
    'settings.refresh': 'Refresh(s)',
    'settings.alerts': 'Usage alerts',
    'settings.thresholds': 'Thresholds',
    'settings.openAtLogin': 'Open at login',
    'settings.showClaude': 'Show Claude',
    'settings.showCodex': 'Show Codex',
    'settings.save': 'Save',
    'settings.refreshNow': 'Refresh',
    'settings.claudeLogout': 'Claude Logout',
    'settings.autostartNote': 'Auto-start applies to packaged app.',
    'firstRun.title': 'AI Usage Widget — first run',
    'firstRun.lede': 'This widget reads your local AI usage data.',
    'firstRun.detailIntro': 'On every refresh it reads:',
    'firstRun.itemCodex': '<code>~/.codex/auth.json</code> (Codex bearer token)',
    'firstRun.itemClaude': 'your claude.ai session cookie (after you sign in via the widget)',
    'firstRun.detailPromise': 'These tokens are used only to call the official Claude / Codex usage APIs. They are never written back to disk by this app, never sent anywhere else, and never leave your machine.',
    'firstRun.quit': 'Quit',
    'firstRun.continue': 'Continue',
    'firstRun.panelsTitle': 'Which panels?',
    'firstRun.panelsDetail': 'You can change this later from the gear icon (⚙) on the widget.',
    'firstRun.bothPanels': 'Claude + Codex',
    'firstRun.claudeOnly': 'Claude only',
    'firstRun.codexOnly': 'Codex only',
    'codex.notConfigured': 'not configured',
    'codex.runCli': 'run codex CLI',
    'codex.expired': 'session expired',
    'claude.notSignedIn': 'not signed in',
    'claude.clickLoginBelow': 'click LOGIN below',
    'claude.loginRequired': 'login required',
    'claude.session': 'claude.ai session',
    'claudeLoginHint': 'Click to sign in at claude.ai',
    'reset.dashes': 'reset --',
    'reset.fmt': (h, m) => (h > 0 ? `reset ${h}h ${m}m` : `reset ${m}m`)
  },
  ko: {
    'settings.language': '언어',
    'settings.display': '표시',
    'settings.displayUsed': '사용',
    'settings.displayLeft': '잔여',
    'settings.refresh': '갱신(초)',
    'settings.alerts': '사용량 알림',
    'settings.thresholds': '임계값',
    'settings.openAtLogin': '로그인 시 시작',
    'settings.showClaude': 'Claude 표시',
    'settings.showCodex': 'Codex 표시',
    'settings.save': '저장',
    'settings.refreshNow': '새로고침',
    'settings.claudeLogout': 'Claude 로그아웃',
    'settings.autostartNote': '자동 시작은 설치본에서 동작합니다.',
    'firstRun.title': 'AI Usage Widget — 첫 실행',
    'firstRun.lede': '이 위젯은 로컬 AI 사용량 데이터를 읽습니다.',
    'firstRun.detailIntro': '갱신할 때마다 다음을 읽습니다:',
    'firstRun.itemCodex': '<code>~/.codex/auth.json</code> (Codex bearer 토큰)',
    'firstRun.itemClaude': 'claude.ai 세션 쿠키 (위젯에서 로그인 후 사용)',
    'firstRun.detailPromise': '이 토큰들은 Claude / Codex 공식 사용량 API를 호출할 때만 쓰입니다. 디스크에 다시 쓰이거나 외부로 전송되지 않으며, 기기를 떠나지 않습니다.',
    'firstRun.quit': '종료',
    'firstRun.continue': '계속',
    'firstRun.panelsTitle': '어떤 패널을 보여드릴까요?',
    'firstRun.panelsDetail': '나중에 위젯의 ⚙ 아이콘에서 바꿀 수 있습니다.',
    'firstRun.bothPanels': 'Claude + Codex',
    'firstRun.claudeOnly': 'Claude만',
    'firstRun.codexOnly': 'Codex만',
    'codex.notConfigured': '미설정',
    'codex.runCli': 'codex CLI 실행',
    'codex.expired': '세션 만료',
    'claude.notSignedIn': '로그인 필요',
    'claude.clickLoginBelow': '아래 LOGIN 클릭',
    'claude.loginRequired': '로그인 필요',
    'claude.session': 'claude.ai 세션',
    'claudeLoginHint': 'claude.ai에 로그인하려면 클릭',
    'reset.dashes': '리셋 --',
    'reset.fmt': (h, m) => (h > 0 ? `리셋 ${h}시 ${m}분` : `리셋 ${m}분`)
  }
};

let currentLanguage = 'en';
function t(key) {
  return (I18N[currentLanguage] && I18N[currentLanguage][key]) || I18N.en[key] || key;
}
function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll('[data-i18n-html]').forEach((el) => {
    el.innerHTML = t(el.dataset.i18nHtml);
  });
}

let currentDisplayMode = 'used';
let currentErrorKey = null;
let dismissedErrorKey = null;
let currentClaudeErrorKey = null;
let dismissedClaudeErrorKey = null;
let settingsPanelOpen = false;
let claudeLoginErrorTimeout = null;

function normalizePanelVisibility(value, fallback = true) {
  return typeof value === 'boolean' ? value : fallback;
}

function applyUiScale(scale) {
  const clamped = Math.max(0.7, Math.min(1.5, Number(scale) || 1));
  document.documentElement.style.zoom = String(clamped);
}

function pickClosestScaleOption(scale) {
  const opts = [0.85, 1, 1.15, 1.3];
  let best = opts[0];
  let bestDiff = Math.abs(scale - best);
  for (const o of opts) {
    const d = Math.abs(scale - o);
    if (d < bestDiff) { best = o; bestDiff = d; }
  }
  return String(best);
}

function applyPanelVisibility(showClaude, showCodex, showGemini) {
  const nextShowClaude = normalizePanelVisibility(showClaude, true);
  const nextShowCodex = normalizePanelVisibility(showCodex, true);
  const nextShowGemini = normalizePanelVisibility(showGemini, false);
  claudeColumn.classList.toggle('col--hidden', !nextShowClaude);
  codexColumn.classList.toggle('col--hidden', !nextShowCodex);
  if (geminiColumn) geminiColumn.classList.toggle('col--hidden', !nextShowGemini);
}

function enforcePanelToggleRule(changedInput) {
  if (!showClaudeInput.checked && !showCodexInput.checked && !showGeminiInput.checked) {
    changedInput.checked = true;
  }
}

function syncSettingsInputs(settings) {
  const lang = settings.language === 'ko' ? 'ko' : 'en';
  if (currentLanguage !== lang) {
    currentLanguage = lang;
    applyI18n();
  }
  languageSelect.value = lang;
  displayModeSelect.value = normalizeDisplayMode(settings.displayMode);
  refreshSecondsInput.value = Math.max(10, Math.round((settings.refreshIntervalMs || 60000) / 1000));
  const scale = Number(settings.uiScale);
  applyUiScale(Number.isFinite(scale) ? scale : 1);
  uiScaleSelect.value = pickClosestScaleOption(Number.isFinite(scale) ? scale : 1);
  alertsEnabledInput.checked = Boolean(settings.enableUsageAlerts);
  alertThresholdsInput.value = Array.isArray(settings.usageAlertThresholds) ? settings.usageAlertThresholds.join(',') : '30,60,80,90';
  openOnStartupInput.checked = Boolean(settings.openOnStartup);
  showClaudeInput.checked = normalizePanelVisibility(settings.showClaude, true);
  showCodexInput.checked = normalizePanelVisibility(settings.showCodex, true);
  showGeminiInput.checked = normalizePanelVisibility(settings.showGemini, false);
  enforcePanelToggleRule(showClaudeInput.checked ? showCodexInput : showClaudeInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked, showGeminiInput.checked);
}

function render(state) {
  const displayMode = normalizeDisplayMode(state.displayMode);
  currentDisplayMode = displayMode;
  renderCodexSection(state, displayMode);

  const codexState = state.codex || {};
  const isCodexConfigured = codexState.isConfigured !== false;
  const hasError = Boolean(state.error) && isCodexConfigured && !codexState.isCached;
  currentErrorKey = hasError ? String(state.error).trim() : null;
  const shouldShowError = hasError && currentErrorKey !== dismissedErrorKey;
  errorBanner.hidden = !shouldShowError;
  if (hasError) {
    errorText.textContent = currentErrorKey;
  } else {
    dismissedErrorKey = null;
  }

  renderClaudeSection(state.claude, displayMode);
  renderGeminiSection(state.gemini);
}

function renderGeminiSection(state) {
  const s = state || {};
  const cloudAvailable = Boolean(s.cloudAvailable);
  const hasCloud = cloudAvailable && Array.isArray(s.quotas) && s.quotas.length > 0;

  geminiSection.classList.toggle('stack--disabled', !hasCloud);

  if (hasCloud) {
    geminiQuotaList.hidden = false;
    geminiFallback.hidden = true;
    renderGeminiQuotaList(s.quotas);
    return;
  }

  geminiQuotaList.hidden = true;
  geminiFallback.hidden = false;
  geminiTodayTag.textContent = s.cloudError ? 'cloud quota unavailable' : 'no quota data';
}

function renderGeminiQuotaList(quotas) {
  geminiQuotaList.innerHTML = '';
  for (const q of quotas) {
    const wrap = document.createElement('div');
    wrap.className = 'pixel-card-wrap';

    const bar = document.createElement('div');
    bar.className = 'pixel-bar pixel-bar--blue';

    const left = document.createElement('div');
    left.className = 'bar-left';
    const icon = document.createElement('img');
    icon.className = 'bar-icon';
    icon.src = './icon-5h.svg';
    icon.draggable = false;
    icon.alt = '';
    const label = document.createElement('span');
    label.className = 'bar-label';
    label.textContent = q.label || q.model;
    left.append(icon, label);

    const usedPct = clampPercentForBar(q.usedPercent);
    const remaining = 100 - usedPct;
    const value = document.createElement('span');
    value.className = 'bar-value';
    value.textContent = `${remaining.toFixed(usedPct < 0.1 ? 0 : 1)}%`;

    const progress = document.createElement('div');
    progress.className = 'bar-progress';
    const fill = document.createElement('span');
    fill.className = 'bar-progress-fill bar-progress-fill--blue';
    fill.style.width = `${usedPct}%`;
    progress.appendChild(fill);

    bar.append(left, value, progress);

    const resetTag = document.createElement('div');
    resetTag.className = 'reset-tag';
    const resetText = document.createElement('span');
    resetText.className = 'reset-tag-text';
    resetText.textContent = q.resetAt ? `reset ${formatResetAtIso(q.resetAt)}` : '';
    resetTag.appendChild(resetText);

    wrap.append(bar, resetTag);
    geminiQuotaList.appendChild(wrap);
  }
}

function formatResetAtIso(iso) {
  try {
    const target = new Date(iso).getTime();
    const diffSec = Math.max(0, Math.round((target - Date.now()) / 1000));
    if (diffSec === 0) return 'now';
    const h = Math.floor(diffSec / 3600);
    const m = Math.floor((diffSec % 3600) / 60);
    return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
  } catch (_) { return ''; }
}

function renderAccountTag(el, label) {
  const text = typeof label === 'string' ? label.trim() : '';
  el.hidden = !text;
  el.textContent = text;
  el.title = text;
}

function renderCodexSection(state, displayMode) {
  const codexState = state.codex || {};
  const isConfigured = codexState.isConfigured !== false;
  const needsLogin = Boolean(codexState.needsLogin);
  const isCached = Boolean(codexState.isCached);
  const disable = !isConfigured || needsLogin;

  const planSuffix = state.planType && state.planType !== 'CODEX' && state.planType !== 'UNKNOWN'
    ? ` · ${String(state.planType).toLowerCase()}`
    : '';
  renderAccountTag(codexAccountTag, codexState.accountEmail ? codexState.accountEmail + planSuffix : '');

  codexSection.classList.toggle('stack--disabled', disable);
  codexPrimaryBar.classList.toggle('pixel-bar--disabled', disable);
  codexSecondaryBar.classList.toggle('pixel-bar--disabled', disable);
  codexPrimaryBar.classList.toggle('pixel-bar--stale', isCached);
  codexSecondaryBar.classList.toggle('pixel-bar--stale', isCached);

  if (!isConfigured) {
    primaryValue.textContent = 'OFF';
    secondaryValue.textContent = '--';
    primaryProgress.style.width = '0%';
    secondaryProgress.style.width = '0%';
    primaryReset.textContent = t('codex.notConfigured');
    secondaryReset.textContent = t('codex.runCli');
    return;
  }

  if (needsLogin) {
    primaryValue.textContent = '--%';
    secondaryValue.textContent = '--%';
    primaryProgress.style.width = '0%';
    secondaryProgress.style.width = '0%';
    primaryReset.textContent = t('codex.expired');
    secondaryReset.textContent = t('codex.runCli');
    return;
  }

  renderUsageSection(
    { primaryValue, secondaryValue, primaryProgress, secondaryProgress, primaryReset, secondaryReset },
    state,
    displayMode
  );
}

function renderUsageSection(elements, state, displayMode) {
  const primary = resolveDisplayPercent(state.primary?.usedPercent, displayMode);
  const secondary = resolveDisplayPercent(state.secondary?.usedPercent, displayMode);

  elements.primaryValue.textContent = formatPercent(primary);
  elements.secondaryValue.textContent = formatPercent(secondary);
  elements.primaryProgress.style.width = `${clampPercentForBar(primary)}%`;
  elements.secondaryProgress.style.width = `${clampPercentForBar(secondary)}%`;
  elements.primaryReset.textContent = formatReset(state.primary?.resetAfterSeconds);
  elements.secondaryReset.textContent = formatReset(state.secondary?.resetAfterSeconds);
}

function renderClaudeSection(claudeState, displayMode) {
  const state = claudeState || {};
  const isConfigured = Boolean(state.isConfigured);
  const needsLogin = Boolean(state.needsLogin);
  const disableBars = !isConfigured || needsLogin;
  const isCached = Boolean(state.isCached);
  claudeSection.classList.toggle('stack--disabled', disableBars);
  claudePrimaryBar.classList.toggle('pixel-bar--disabled', disableBars);
  claudeSecondaryBar.classList.toggle('pixel-bar--disabled', disableBars);
  claudePrimaryBar.classList.toggle('pixel-bar--stale', isCached && !disableBars);
  claudeSecondaryBar.classList.toggle('pixel-bar--stale', isCached && !disableBars);
  claudeLoginWrap.hidden = !disableBars;
  renderAccountTag(claudeAccountTag, state.accountLabel);

  if (!isConfigured) {
    claudePrimaryValue.textContent = '--%';
    claudeSecondaryValue.textContent = '--%';
    claudePrimaryProgress.style.width = '0%';
    claudeSecondaryProgress.style.width = '0%';
    claudePrimaryReset.textContent = t('claude.notSignedIn');
    claudeSecondaryReset.textContent = t('claude.clickLoginBelow');
    claudeStatusPill.hidden = true;
    claudeErrorBanner.hidden = true;
    dismissedClaudeErrorKey = null;
    currentClaudeErrorKey = null;
    return;
  }

  if (needsLogin) {
    claudePrimaryValue.textContent = '--%';
    claudeSecondaryValue.textContent = '--%';
    claudePrimaryProgress.style.width = '0%';
    claudeSecondaryProgress.style.width = '0%';
    claudePrimaryReset.textContent = t('claude.loginRequired');
    claudeSecondaryReset.textContent = t('claude.session');
    claudeStatusPill.hidden = true;
    claudeErrorBanner.hidden = true;
    dismissedClaudeErrorKey = null;
    currentClaudeErrorKey = null;
    return;
  }

  renderUsageSection(
    {
      primaryValue: claudePrimaryValue,
      secondaryValue: claudeSecondaryValue,
      primaryProgress: claudePrimaryProgress,
      secondaryProgress: claudeSecondaryProgress,
      primaryReset: claudePrimaryReset,
      secondaryReset: claudeSecondaryReset
    },
    state,
    displayMode
  );

  claudeStatusPill.hidden = true;

  const hasClaudeError = Boolean(state.error) && !state.isCached;
  currentClaudeErrorKey = hasClaudeError ? String(state.error).trim() : null;
  const shouldShowClaudeError = hasClaudeError && currentClaudeErrorKey !== dismissedClaudeErrorKey;
  claudeErrorBanner.hidden = !shouldShowClaudeError;
  if (hasClaudeError) {
    claudeErrorText.textContent = currentClaudeErrorKey;
  } else {
    dismissedClaudeErrorKey = null;
  }
}

function formatReset(totalSeconds) {
  if (typeof totalSeconds !== 'number' || !Number.isFinite(totalSeconds)) {
    return t('reset.dashes');
  }
  const seconds = Math.max(0, Math.round(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return t('reset.fmt')(hours, minutes);
}

function formatPercent(value) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return '--%';
  }
  return `${Math.round(value)}%`;
}

function normalizeDisplayMode(mode) {
  return String(mode || '').toLowerCase() === 'left' ? 'left' : 'used';
}

function parseThresholds(text) {
  if (!text || !String(text).trim()) {
    return [];
  }
  return String(text)
    .split(',')
    .map((item) => Number(item.trim()))
    .filter((item) => Number.isFinite(item))
    .map((item) => Math.round(item));
}

function resolveDisplayPercent(value, mode) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  const rounded = Math.max(0, Math.min(Math.round(value), 100));
  if (mode === 'left') {
    return 100 - rounded;
  }
  return rounded;
}

function clampPercentForBar(value) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(Math.round(value), 100));
}

const firstRunOverlay = document.getElementById('firstRunOverlay');
const firstRunConsent = document.getElementById('firstRunConsent');
const firstRunPanels = document.getElementById('firstRunPanels');
const firstRunContinueBtn = document.getElementById('firstRunContinueBtn');
const firstRunQuitBtn = document.getElementById('firstRunQuitBtn');

function showFirstRunStep(step) {
  firstRunConsent.hidden = step !== 'consent';
  firstRunPanels.hidden = step !== 'panels';
}

function hideFirstRun() {
  firstRunOverlay.hidden = true;
}

firstRunQuitBtn.addEventListener('click', () => {
  window.codexWidget.hide();
});

firstRunContinueBtn.addEventListener('click', () => {
  showFirstRunStep('panels');
});

firstRunPanels.querySelectorAll('[data-panel-pick]').forEach((btn) => {
  btn.addEventListener('click', async () => {
    const pick = btn.dataset.panelPick;
    const showClaude = pick !== 'codex';
    const showCodex = pick !== 'claude';
    btn.disabled = true;
    try {
      const next = await window.codexWidget.acceptConsent(showClaude, showCodex);
      syncSettingsInputs(next);
      hideFirstRun();
    } finally {
      btn.disabled = false;
    }
  });
});

window.codexWidget.getInitialState().then(render);
window.codexWidget.onState(render);
languageSelect.addEventListener('change', () => {
  currentLanguage = languageSelect.value === 'ko' ? 'ko' : 'en';
  applyI18n();
});

applyI18n();
window.codexWidget.getSettings().then((settings) => {
  syncSettingsInputs(settings);
  if (!settings.consentAccepted) {
    showFirstRunStep('consent');
    firstRunOverlay.hidden = false;
  }
});

hideButton.addEventListener('click', () => {
  window.codexWidget.hide();
});

errorCloseButton.addEventListener('click', () => {
  if (currentErrorKey) {
    dismissedErrorKey = currentErrorKey;
  }
  errorBanner.hidden = true;
  window.codexWidget.refreshNow();
});

claudeErrorCloseButton.addEventListener('click', () => {
  if (currentClaudeErrorKey) {
    dismissedClaudeErrorKey = currentClaudeErrorKey;
  }
  claudeErrorBanner.hidden = true;
  window.codexWidget.refreshNow();
});

settingsToggleButton.addEventListener('click', () => {
  settingsPanelOpen = !settingsPanelOpen;
  settingsPanel.hidden = !settingsPanelOpen;
});

showClaudeInput.addEventListener('change', () => {
  enforcePanelToggleRule(showClaudeInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked, showGeminiInput.checked);
});

showCodexInput.addEventListener('change', () => {
  enforcePanelToggleRule(showCodexInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked, showGeminiInput.checked);
});

showGeminiInput.addEventListener('change', () => {
  enforcePanelToggleRule(showGeminiInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked, showGeminiInput.checked);
});

uiScaleSelect.addEventListener('change', () => {
  applyUiScale(Number(uiScaleSelect.value) || 1);
});

settingsSaveButton.addEventListener('click', async () => {
  enforcePanelToggleRule(showClaudeInput.checked ? showCodexInput : showClaudeInput);
  const payload = {
    language: languageSelect.value === 'ko' ? 'ko' : 'en',
    displayMode: normalizeDisplayMode(displayModeSelect.value),
    refreshIntervalMs: Math.max(10, Number(refreshSecondsInput.value || 60)) * 1000,
    enableUsageAlerts: Boolean(alertsEnabledInput.checked),
    usageAlertThresholds: parseThresholds(alertThresholdsInput.value),
    openOnStartup: Boolean(openOnStartupInput.checked),
    showClaude: Boolean(showClaudeInput.checked),
    showCodex: Boolean(showCodexInput.checked),
    showGemini: Boolean(showGeminiInput.checked),
    uiScale: Number(uiScaleSelect.value) || 1
  };

  settingsSaveButton.disabled = true;
  try {
    const next = await window.codexWidget.updateSettings(payload);
    syncSettingsInputs(next);
    settingsPanel.hidden = true;
    settingsPanelOpen = false;
  } finally {
    settingsSaveButton.disabled = false;
  }
});

settingsRefreshButton.addEventListener('click', () => {
  window.codexWidget.refreshNow();
});

claudeLoginBtn.addEventListener('click', async () => {
  claudeLoginBtn.disabled = true;
  claudeLoginBtn.textContent = 'OPENING...';
  try {
    // claude_login resolves with no payload on success and rejects on failure.
    await window.codexWidget.claudeLogin();
  } catch (err) {
    currentClaudeErrorKey = String(err || 'Claude login failed.');
    claudeErrorText.textContent = currentClaudeErrorKey;
    claudeErrorBanner.hidden = false;
    if (claudeLoginErrorTimeout) {
      clearTimeout(claudeLoginErrorTimeout);
    }
    claudeLoginErrorTimeout = setTimeout(() => {
      claudeErrorBanner.hidden = true;
    }, 4000);
  } finally {
    claudeLoginBtn.disabled = false;
    claudeLoginBtn.textContent = 'LOGIN';
  }
});

claudeLogoutButton.addEventListener('click', async () => {
  claudeLogoutButton.disabled = true;
  try {
    await window.codexWidget.claudeLogout();
  } finally {
    claudeLogoutButton.disabled = false;
  }
});

geminiErrorCloseButton.addEventListener('click', () => {
  geminiErrorBanner.hidden = true;
});
