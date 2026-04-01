const primaryValue = document.getElementById('primaryValue');
const secondaryValue = document.getElementById('secondaryValue');
const primaryProgress = document.getElementById('primaryProgress');
const secondaryProgress = document.getElementById('secondaryProgress');
const primaryReset = document.getElementById('primaryReset');
const secondaryReset = document.getElementById('secondaryReset');
const hideButton = document.getElementById('hideButton');

function render(state) {
  const primary = Math.round(state.primary?.usedPercent ?? 0);
  const secondary = Math.round(state.secondary?.usedPercent ?? 0);

  primaryValue.textContent = `${primary}%`;
  secondaryValue.textContent = `${secondary}%`;
  primaryProgress.style.width = `${Math.max(0, Math.min(primary, 100))}%`;
  secondaryProgress.style.width = `${Math.max(0, Math.min(secondary, 100))}%`;
  primaryReset.textContent = formatReset(state.primary?.resetAfterSeconds);
  secondaryReset.textContent = formatReset(state.secondary?.resetAfterSeconds);
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

window.codexWidget.getInitialState().then(render);
window.codexWidget.onState(render);

hideButton.addEventListener('click', () => {
  window.codexWidget.hide();
});
