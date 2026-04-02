const test = require('node:test');
const assert = require('node:assert/strict');
const {
  clampPercent,
  computeDisplayPercent,
  didUsageWindowReset,
  getCrossedThresholds,
  modeLabel,
  normalizeDisplayMode,
  sanitizeThresholds
} = require('../lib/widget-core');

test('normalizeDisplayMode defaults to used', () => {
  assert.equal(normalizeDisplayMode('used'), 'used');
  assert.equal(normalizeDisplayMode('left'), 'left');
  assert.equal(normalizeDisplayMode('LEFT'), 'left');
  assert.equal(normalizeDisplayMode('invalid'), 'used');
});

test('clampPercent keeps range 0..100', () => {
  assert.equal(clampPercent(-10), 0);
  assert.equal(clampPercent(53.6), 54);
  assert.equal(clampPercent(150), 100);
});

test('computeDisplayPercent switches used and left mode', () => {
  assert.equal(computeDisplayPercent(35, 'used'), 35);
  assert.equal(computeDisplayPercent(35, 'left'), 65);
  assert.equal(computeDisplayPercent(null, 'left'), 100);
});

test('sanitizeThresholds filters and sorts values', () => {
  assert.deepEqual(sanitizeThresholds([80, 30, 30, 120, -1, '60']), [30, 60, 80]);
});

test('getCrossedThresholds detects upward crossings only', () => {
  assert.deepEqual(getCrossedThresholds(25, 81, [30, 60, 80, 90]), [30, 60, 80]);
  assert.deepEqual(getCrossedThresholds(81, 82, [30, 60, 80, 90]), []);
  assert.deepEqual(getCrossedThresholds(null, 82, [30, 60, 80, 90]), []);
});

test('didUsageWindowReset detects meaningful drop', () => {
  assert.equal(didUsageWindowReset(75, 10), true);
  assert.equal(didUsageWindowReset(75, 72), false);
});

test('modeLabel returns uppercase mode name', () => {
  assert.equal(modeLabel('left'), 'LEFT');
  assert.equal(modeLabel('used'), 'USED');
});
