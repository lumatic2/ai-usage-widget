const primaryValue = document.getElementById('primaryValue');
const secondaryValue = document.getElementById('secondaryValue');
const primaryProgress = document.getElementById('primaryProgress');
const secondaryProgress = document.getElementById('secondaryProgress');
const primaryReset = document.getElementById('primaryReset');
const secondaryReset = document.getElementById('secondaryReset');
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
const claudeLoginWrap = document.getElementById('claudeLoginWrap');
const claudeLoginBtn = document.getElementById('claudeLoginBtn');
const hideButton = document.getElementById('hideButton');
const hideButtonAlt = document.getElementById('hideButtonAlt');
const claudeActions = document.getElementById('claudeActions');
const errorBanner = document.getElementById('errorBanner');
const errorText = document.getElementById('errorText');
const errorCloseButton = document.getElementById('errorCloseButton');
const claudeErrorBanner = document.getElementById('claudeErrorBanner');
const claudeErrorText = document.getElementById('claudeErrorText');
const claudeErrorCloseButton = document.getElementById('claudeErrorCloseButton');
const settingsToggleButton = document.getElementById('settingsToggleButton');
const settingsToggleButtonAlt = document.getElementById('settingsToggleButtonAlt');
const settingsPanel = document.getElementById('settingsPanel');
const twoCol = document.querySelector('.two-col');
const claudeColumn = document.querySelector('.col--claude');
const codexColumn = document.querySelector('.col--codex');
const displayModeSelect = document.getElementById('displayModeSelect');
const refreshSecondsInput = document.getElementById('refreshSecondsInput');
const alertsEnabledInput = document.getElementById('alertsEnabledInput');
const alertThresholdsInput = document.getElementById('alertThresholdsInput');
const openOnStartupInput = document.getElementById('openOnStartupInput');
const showClaudeInput = document.getElementById('showClaudeInput');
const showCodexInput = document.getElementById('showCodexInput');
const settingsSaveButton = document.getElementById('settingsSaveButton');
const settingsRefreshButton = document.getElementById('settingsRefreshButton');
const claudeLogoutButton = document.getElementById('claudeLogoutButton');
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

function applyPanelVisibility(showClaude, showCodex) {
  const nextShowClaude = normalizePanelVisibility(showClaude, true);
  const nextShowCodex = normalizePanelVisibility(showCodex, true);
  claudeColumn.classList.toggle('col--hidden', !nextShowClaude);
  codexColumn.classList.toggle('col--hidden', !nextShowCodex);
  // 버튼: Codex가 보이면 Codex 행, 숨겨지면 Claude 행으로
  claudeActions.hidden = nextShowCodex;
}

function enforcePanelToggleRule(changedInput) {
  if (!showClaudeInput.checked && !showCodexInput.checked) {
    changedInput.checked = true;
  }
}

function syncSettingsInputs(settings) {
  displayModeSelect.value = normalizeDisplayMode(settings.displayMode);
  refreshSecondsInput.value = Math.max(10, Math.round((settings.refreshIntervalMs || 60000) / 1000));
  alertsEnabledInput.checked = Boolean(settings.enableUsageAlerts);
  alertThresholdsInput.value = Array.isArray(settings.usageAlertThresholds) ? settings.usageAlertThresholds.join(',') : '30,60,80,90';
  openOnStartupInput.checked = Boolean(settings.openOnStartup);
  showClaudeInput.checked = normalizePanelVisibility(settings.showClaude, true);
  showCodexInput.checked = normalizePanelVisibility(settings.showCodex, true);
  enforcePanelToggleRule(showClaudeInput.checked ? showCodexInput : showClaudeInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked);
}

function render(state) {
  const displayMode = normalizeDisplayMode(state.displayMode);
  currentDisplayMode = displayMode;
  renderUsageSection(
    {
      primaryValue,
      secondaryValue,
      primaryProgress,
      secondaryProgress,
      primaryReset,
      secondaryReset
    },
    state,
    displayMode
  );

  const hasError = Boolean(state.error);
  currentErrorKey = hasError ? String(state.error).trim() : null;
  const shouldShowError = hasError && currentErrorKey !== dismissedErrorKey;
  errorBanner.hidden = !shouldShowError;
  if (hasError) {
    errorText.textContent = currentErrorKey;
  } else {
    dismissedErrorKey = null;
  }

  renderClaudeSection(state.claude, displayMode);
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
  claudeSection.classList.toggle('stack--disabled', disableBars);
  claudePrimaryBar.classList.toggle('pixel-bar--disabled', disableBars);
  claudeSecondaryBar.classList.toggle('pixel-bar--disabled', disableBars);
  claudeLoginWrap.hidden = !needsLogin;

  if (!isConfigured) {
    claudePrimaryValue.textContent = 'OFF';
    claudeSecondaryValue.textContent = '--';
    claudePrimaryProgress.style.width = '0%';
    claudeSecondaryProgress.style.width = '0%';
    claudePrimaryReset.textContent = 'not configured';
    claudeSecondaryReset.textContent = 'add ~/.claude';
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
    claudePrimaryReset.textContent = 'login required';
    claudeSecondaryReset.textContent = 'claude.ai session';
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
    return 'reset --';
  }
  const seconds = Math.max(0, Math.round(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) {
    return `reset ${hours}h ${minutes}m`;
  }
  return `reset ${minutes}m`;
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

window.codexWidget.getInitialState().then(render);
window.codexWidget.onState(render);
window.codexWidget.getSettings().then(syncSettingsInputs);

hideButton.addEventListener('click', () => {
  window.codexWidget.hide();
});

hideButtonAlt.addEventListener('click', () => {
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

settingsToggleButtonAlt.addEventListener('click', () => {
  settingsPanelOpen = !settingsPanelOpen;
  settingsPanel.hidden = !settingsPanelOpen;
});

showClaudeInput.addEventListener('change', () => {
  enforcePanelToggleRule(showClaudeInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked);
});

showCodexInput.addEventListener('change', () => {
  enforcePanelToggleRule(showCodexInput);
  applyPanelVisibility(showClaudeInput.checked, showCodexInput.checked);
});

settingsSaveButton.addEventListener('click', async () => {
  enforcePanelToggleRule(showClaudeInput.checked ? showCodexInput : showClaudeInput);
  const payload = {
    displayMode: normalizeDisplayMode(displayModeSelect.value),
    refreshIntervalMs: Math.max(10, Number(refreshSecondsInput.value || 60)) * 1000,
    enableUsageAlerts: Boolean(alertsEnabledInput.checked),
    usageAlertThresholds: parseThresholds(alertThresholdsInput.value),
    openOnStartup: Boolean(openOnStartupInput.checked),
    showClaude: Boolean(showClaudeInput.checked),
    showCodex: Boolean(showCodexInput.checked)
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
    const result = await window.codexWidget.claudeLogin();
    if (!result?.success) {
      currentClaudeErrorKey = String(result?.error || 'Claude login failed.');
      claudeErrorText.textContent = currentClaudeErrorKey;
      claudeErrorBanner.hidden = false;
      if (claudeLoginErrorTimeout) {
        clearTimeout(claudeLoginErrorTimeout);
      }
      claudeLoginErrorTimeout = setTimeout(() => {
        claudeErrorBanner.hidden = true;
      }, 4000);
    }
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
