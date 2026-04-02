function normalizeDisplayMode(mode) {
  return String(mode || '').toLowerCase() === 'left' ? 'left' : 'used';
}

function clampPercent(value) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(Math.round(value), 100));
}

function computeDisplayPercent(usedPercent, mode) {
  const normalized = normalizeDisplayMode(mode);
  const used = clampPercent(usedPercent);
  return normalized === 'left' ? 100 - used : used;
}

function sanitizeThresholds(input) {
  const source = Array.isArray(input) ? input : [];
  const unique = new Set();
  for (const raw of source) {
    const value = Number(raw);
    if (!Number.isFinite(value)) {
      continue;
    }
    const normalized = Math.round(value);
    if (normalized >= 1 && normalized <= 99) {
      unique.add(normalized);
    }
  }
  return [...unique].sort((a, b) => a - b);
}

function getCrossedThresholds(previous, current, thresholds) {
  if (typeof previous !== 'number' || !Number.isFinite(previous)) {
    return [];
  }
  if (typeof current !== 'number' || !Number.isFinite(current)) {
    return [];
  }
  return thresholds.filter((threshold) => previous < threshold && current >= threshold);
}

function didUsageWindowReset(previous, current) {
  if (typeof previous !== 'number' || !Number.isFinite(previous)) {
    return false;
  }
  if (typeof current !== 'number' || !Number.isFinite(current)) {
    return false;
  }
  // Usage should generally increase, so a meaningful drop implies reset/new window.
  return current + 5 < previous;
}

function modeLabel(mode) {
  return normalizeDisplayMode(mode).toUpperCase();
}

module.exports = {
  clampPercent,
  computeDisplayPercent,
  didUsageWindowReset,
  getCrossedThresholds,
  modeLabel,
  normalizeDisplayMode,
  sanitizeThresholds
};
