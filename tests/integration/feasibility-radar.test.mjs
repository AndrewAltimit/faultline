/**
 * Unit tests for site/js/app/feasibility-radar.js — the pure normalization
 * that turns feasibility-matrix rows into a [0, 1] radar vector per axis.
 *
 * Run with: node --test tests/integration/feasibility-radar.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

const { buildFeasibilityAxes, normalizeCostAsymmetry, FEASIBILITY_AXES } = await import(
  join(repoRoot, 'site', 'js', 'app', 'feasibility-radar.js')
);

function row(overrides = {}) {
  return {
    chain_id: 'c1',
    chain_name: 'Chain One',
    technology_readiness: 0.8,
    operational_complexity: 0.4,
    detection_probability: 0.3,
    success_probability: 0.5,
    consequence_severity: 0.7,
    attribution_difficulty: 0.6,
    cost_asymmetry_ratio: 100,
    ...overrides,
  };
}

test('FEASIBILITY_AXES has the seven documented axes', () => {
  assert.equal(FEASIBILITY_AXES.length, 7);
  assert.deepEqual(
    FEASIBILITY_AXES.map((a) => a.key),
    [
      'technology_readiness',
      'operational_complexity',
      'detection_probability',
      'success_probability',
      'consequence_severity',
      'attribution_difficulty',
      'cost_asymmetry',
    ],
  );
});

test('buildFeasibilityAxes returns empty series for no rows', () => {
  assert.deepEqual(buildFeasibilityAxes(null).series, []);
  assert.deepEqual(buildFeasibilityAxes([]).series, []);
});

test('buildFeasibilityAxes passes proportions through unchanged', () => {
  const { series } = buildFeasibilityAxes([row()]);
  assert.equal(series.length, 1);
  const s = series[0];
  // First six axes are the raw proportions (cost is index 6).
  assert.equal(s.values[0], 0.8); // tech
  assert.equal(s.values[1], 0.4); // complexity
  assert.equal(s.values[2], 0.3); // detection
  assert.equal(s.values[3], 0.5); // success
  assert.equal(s.values[4], 0.7); // severity
  assert.equal(s.values[5], 0.6); // attribution
  assert.deepEqual(s.raw.slice(0, 6), [0.8, 0.4, 0.3, 0.5, 0.7, 0.6]);
  assert.equal(s.raw[6], 100); // raw cost ratio preserved
});

test('buildFeasibilityAxes clamps out-of-range proportions to [0,1]', () => {
  const { series } = buildFeasibilityAxes([
    row({ technology_readiness: 1.4, detection_probability: -0.2 }),
  ]);
  assert.equal(series[0].values[0], 1);
  assert.equal(series[0].values[2], 0);
});

test('buildFeasibilityAxes normalizes cost asymmetry across the batch (log scale)', () => {
  const { series } = buildFeasibilityAxes([
    row({ chain_id: 'lo', cost_asymmetry_ratio: 10 }),
    row({ chain_id: 'mid', cost_asymmetry_ratio: 100 }),
    row({ chain_id: 'hi', cost_asymmetry_ratio: 1000 }),
  ]);
  const cost = series.map((s) => s.values[6]);
  // log10 of 10/100/1000 is 1/2/3 → normalized 0 / 0.5 / 1.
  assert.ok(Math.abs(cost[0] - 0) < 1e-9);
  assert.ok(Math.abs(cost[1] - 0.5) < 1e-9);
  assert.ok(Math.abs(cost[2] - 1) < 1e-9);
});

test('buildFeasibilityAxes maps a single chain cost to the midpoint', () => {
  const { series } = buildFeasibilityAxes([row({ cost_asymmetry_ratio: 7 })]);
  assert.equal(series[0].values[6], 0.5);
});

test('normalizeCostAsymmetry handles degenerate / invalid input', () => {
  assert.equal(normalizeCostAsymmetry(0, 0, 2), 0);
  assert.equal(normalizeCostAsymmetry(-5, 0, 2), 0);
  assert.equal(normalizeCostAsymmetry(NaN, 0, 2), 0);
  // No span → midpoint.
  assert.equal(normalizeCostAsymmetry(100, 2, 2), 0.5);
});

test('buildFeasibilityAxes falls back to chain_id when name missing', () => {
  const { series } = buildFeasibilityAxes([row({ chain_name: undefined })]);
  assert.equal(series[0].chainName, 'c1');
});
