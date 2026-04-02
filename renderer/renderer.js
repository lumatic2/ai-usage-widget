const primaryValue = document.getElementById('primaryValue');
const secondaryValue = document.getElementById('secondaryValue');
const primaryProgress = document.getElementById('primaryProgress');
const secondaryProgress = document.getElementById('secondaryProgress');
const primaryReset = document.getElementById('primaryReset');
const secondaryReset = document.getElementById('secondaryReset');
const hideButton = document.getElementById('hideButton');
const errorBanner = document.getElementById('errorBanner');
const errorText = document.getElementById('errorText');

function render(state) {
  const primary = state.primary?.usedPercent;
  const secondary = state.secondary?.usedPercent;

  primaryValue.textContent = formatPercent(primary);
  secondaryValue.textContent = formatPercent(secondary);
  primaryProgress.style.width = `${clampPercentForBar(primary)}%`;
  secondaryProgress.style.width = `${clampPercentForBar(secondary)}%`;
  primaryReset.textContent = formatReset(state.primary?.resetAfterSeconds);
  secondaryReset.textContent = formatReset(state.secondary?.resetAfterSeconds);

  const hasError = Boolean(state.error);
  errorBanner.hidden = !hasError;
  if (hasError) {
    errorText.textContent = state.error;
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

function clampPercentForBar(value) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(Math.round(value), 100));
}

window.codexWidget.getInitialState().then(render);
window.codexWidget.onState(render);

hideButton.addEventListener('click', () => {
  window.codexWidget.hide();
});
