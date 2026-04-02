const primaryValue = document.getElementById('primaryValue');
const secondaryValue = document.getElementById('secondaryValue');
const primaryProgress = document.getElementById('primaryProgress');
const secondaryProgress = document.getElementById('secondaryProgress');
const primaryReset = document.getElementById('primaryReset');
const secondaryReset = document.getElementById('secondaryReset');
const hideButton = document.getElementById('hideButton');
const errorBanner = document.getElementById('errorBanner');
const errorText = document.getElementById('errorText');
const errorCloseButton = document.getElementById('errorCloseButton');
const settingsToggleButton = document.getElementById('settingsToggleButton');
const settingsPanel = document.getElementById('settingsPanel');
const displayModeSelect = document.getElementById('displayModeSelect');
const refreshSecondsInput = document.getElementById('refreshSecondsInput');
const alertsEnabledInput = document.getElementById('alertsEnabledInput');
const alertThresholdsInput = document.getElementById('alertThresholdsInput');
const openOnStartupInput = document.getElementById('openOnStartupInput');
const settingsSaveButton = document.getElementById('settingsSaveButton');
const settingsRefreshButton = document.getElementById('settingsRefreshButton');
let currentDisplayMode = 'used';
let currentErrorKey = null;
let dismissedErrorKey = null;
let settingsPanelOpen = false;

function render(state) {
  const displayMode = normalizeDisplayMode(state.displayMode);
  currentDisplayMode = displayMode;
  const primary = resolveDisplayPercent(state.primary?.usedPercent, displayMode);
  const secondary = resolveDisplayPercent(state.secondary?.usedPercent, displayMode);

  primaryValue.textContent = formatPercent(primary);
  secondaryValue.textContent = formatPercent(secondary);
  primaryProgress.style.width = `${clampPercentForBar(primary)}%`;
  secondaryProgress.style.width = `${clampPercentForBar(secondary)}%`;
  primaryReset.textContent = formatReset(state.primary?.resetAfterSeconds);
  secondaryReset.textContent = formatReset(state.secondary?.resetAfterSeconds);

  const hasError = Boolean(state.error);
  currentErrorKey = hasError ? String(state.error).trim() : null;
  const shouldShowError = hasError && currentErrorKey !== dismissedErrorKey;
  errorBanner.hidden = !shouldShowError;
  if (hasError) {
    errorText.textContent = currentErrorKey;
  } else {
    dismissedErrorKey = null;
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
window.codexWidget.getSettings().then((settings) => {
  displayModeSelect.value = normalizeDisplayMode(settings.displayMode);
  refreshSecondsInput.value = Math.max(10, Math.round((settings.refreshIntervalMs || 60000) / 1000));
  alertsEnabledInput.checked = Boolean(settings.enableUsageAlerts);
  alertThresholdsInput.value = Array.isArray(settings.usageAlertThresholds) ? settings.usageAlertThresholds.join(',') : '30,60,80,90';
  openOnStartupInput.checked = Boolean(settings.openOnStartup);
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

settingsToggleButton.addEventListener('click', () => {
  settingsPanelOpen = !settingsPanelOpen;
  settingsPanel.hidden = !settingsPanelOpen;
});

settingsSaveButton.addEventListener('click', async () => {
  const payload = {
    displayMode: normalizeDisplayMode(displayModeSelect.value),
    refreshIntervalMs: Math.max(10, Number(refreshSecondsInput.value || 60)) * 1000,
    enableUsageAlerts: Boolean(alertsEnabledInput.checked),
    usageAlertThresholds: parseThresholds(alertThresholdsInput.value),
    openOnStartup: Boolean(openOnStartupInput.checked)
  };

  settingsSaveButton.disabled = true;
  try {
    const next = await window.codexWidget.updateSettings(payload);
    displayModeSelect.value = normalizeDisplayMode(next.displayMode);
    refreshSecondsInput.value = Math.round((next.refreshIntervalMs || 60000) / 1000);
    alertsEnabledInput.checked = Boolean(next.enableUsageAlerts);
    alertThresholdsInput.value = Array.isArray(next.usageAlertThresholds) ? next.usageAlertThresholds.join(',') : '';
    openOnStartupInput.checked = Boolean(next.openOnStartup);
    settingsPanel.hidden = true;
    settingsPanelOpen = false;
  } finally {
    settingsSaveButton.disabled = false;
  }
});

settingsRefreshButton.addEventListener('click', () => {
  window.codexWidget.refreshNow();
});
