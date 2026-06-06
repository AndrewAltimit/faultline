/**
 * Unit tests for the pure charting helpers used by the Monte Carlo
 * dashboard: the colorblind-safe palette (site/js/app/palette.js) and the
 * statistics / KDE / axis-tick math (site/js/app/chart-helpers.js).
 *
 * These modules are dependency-free and DOM-free, so they run directly
 * under Node. The Canvas drawing primitives in chart-helpers.js are NOT
 * exercised here (they need a 2D context); only the math is.
 *
 *   node --test tests/integration/chart-helpers.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const palette = await import(join(repoRoot, 'site', 'js', 'app', 'palette.js'));
const helpers = await import(join(repoRoot, 'site', 'js', 'app', 'chart-helpers.js'));

const { QUALITATIVE, qualitative, factionColorMap, withAlpha, sequential } = palette;
const { percentile, summaryStats, silvermanBandwidth, kde, niceTicks } = helpers;

const HEX = /^#[0-9a-f]{6}$/i;

// ---------------------------------------------------------------------------
// palette
// ---------------------------------------------------------------------------

test('qualitative cycles the ramp and handles negative indices', () => {
  for (let i = 0; i < QUALITATIVE.length; i++) {
    assert.equal(qualitative(i), QUALITATIVE[i]);
  }
  // Wraps modulo length.
  assert.equal(qualitative(QUALITATIVE.length), QUALITATIVE[0]);
  // Negative indices wrap into range, never undefined.
  assert.match(qualitative(-1), HEX);
  assert.equal(qualitative(-QUALITATIVE.length), QUALITATIVE[0]);
});

test('factionColorMap assigns stable distinct colors in declaration order', () => {
  const map = factionColorMap(['red', 'blue', 'green']);
  assert.equal(map.get('red'), QUALITATIVE[0]);
  assert.equal(map.get('blue'), QUALITATIVE[1]);
  assert.equal(map.get('green'), QUALITATIVE[2]);
  // Duplicate ids do not consume a new slot.
  const dup = factionColorMap(['a', 'a', 'b']);
  assert.equal(dup.get('a'), QUALITATIVE[0]);
  assert.equal(dup.get('b'), QUALITATIVE[1]);
  assert.equal(dup.size, 2);
});

test('withAlpha produces rgba and passes through non-hex input', () => {
  assert.equal(withAlpha('#0072b2', 0.5), 'rgba(0, 114, 178, 0.500)');
  // Clamps alpha.
  assert.equal(withAlpha('#000000', 5), 'rgba(0, 0, 0, 1.000)');
  assert.equal(withAlpha('#000000', -1), 'rgba(0, 0, 0, 0.000)');
  // Non-hex input returned unchanged.
  assert.equal(withAlpha('not-a-color', 0.5), 'not-a-color');
});

test('sequential ramp is monotone-ish and clamps endpoints', () => {
  assert.match(sequential(0), HEX);
  assert.match(sequential(1), HEX);
  // Out-of-range clamps rather than NaN.
  assert.equal(sequential(-1), sequential(0));
  assert.equal(sequential(2), sequential(1));
});

// ---------------------------------------------------------------------------
// statistics
// ---------------------------------------------------------------------------

test('percentile interpolates and handles edges', () => {
  const xs = [10, 20, 30, 40, 50];
  assert.equal(percentile(xs, 0), 10);
  assert.equal(percentile(xs, 100), 50);
  assert.equal(percentile(xs, 50), 30);
  // 25th percentile linear-interpolated.
  assert.equal(percentile(xs, 25), 20);
  assert.ok(Number.isNaN(percentile([], 50)));
  // Unsorted input is handled.
  assert.equal(percentile([50, 10, 30, 40, 20], 50), 30);
});

test('summaryStats computes mean/std/percentiles', () => {
  const s = summaryStats([2, 4, 4, 4, 5, 5, 7, 9]);
  assert.equal(s.n, 8);
  assert.equal(s.min, 2);
  assert.equal(s.max, 9);
  assert.equal(s.mean, 5);
  // Sample std dev of this textbook set is ~2.138.
  assert.ok(Math.abs(s.std - 2.13809) < 1e-3);
  assert.equal(s.p50, 4.5);
  // Non-finite values are filtered out.
  const s2 = summaryStats([1, NaN, 3, Infinity]);
  assert.equal(s2.n, 2);
  assert.equal(s2.mean, 2);
});

test('summaryStats on empty array is all-NaN, n=0', () => {
  const s = summaryStats([]);
  assert.equal(s.n, 0);
  assert.ok(Number.isNaN(s.mean));
});

test('silvermanBandwidth is positive and finite', () => {
  const bw = silvermanBandwidth([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  assert.ok(bw > 0 && Number.isFinite(bw));
  // Degenerate input never returns 0 (would divide-by-zero downstream).
  assert.ok(silvermanBandwidth([5, 5, 5, 5]) > 0);
  assert.ok(silvermanBandwidth([42]) > 0);
});

// ---------------------------------------------------------------------------
// KDE
// ---------------------------------------------------------------------------

test('kde returns null for fewer than two points', () => {
  assert.equal(kde([]), null);
  assert.equal(kde([7]), null);
});

test('kde grid is non-negative and integrates to ~1', () => {
  // Sample a known-ish distribution; the density should integrate to ~1
  // over the (padded) support via the trapezoid rule.
  const data = [];
  for (let i = 0; i < 200; i++) data.push(Math.sin(i) * 5 + 50);
  const d = kde(data, { samples: 200 });
  assert.ok(d);
  assert.equal(d.xs.length, 200);
  assert.equal(d.ys.length, 200);
  for (const y of d.ys) assert.ok(y >= 0);
  // Trapezoidal integral over the grid.
  let area = 0;
  for (let i = 1; i < d.xs.length; i++) {
    const dx = d.xs[i] - d.xs[i - 1];
    area += ((d.ys[i] + d.ys[i - 1]) / 2) * dx;
  }
  assert.ok(Math.abs(area - 1) < 0.05, `density integrated to ${area}`);
});

test('kde handles all-identical input without NaN', () => {
  const d = kde([3, 3, 3, 3, 3]);
  assert.ok(d);
  for (const y of d.ys) assert.ok(Number.isFinite(y) && y >= 0);
});

// ---------------------------------------------------------------------------
// nice ticks
// ---------------------------------------------------------------------------

test('niceTicks produces round values within range', () => {
  const ticks = niceTicks(0, 100, 5);
  assert.ok(ticks.length >= 3);
  // Spacing is a 1-2-5 round number.
  const step = ticks[1] - ticks[0];
  assert.ok([1, 2, 5, 10, 20, 25, 50].includes(step), `step was ${step}`);
  for (const t of ticks) assert.ok(t >= 0 && t <= 100);
});

test('niceTicks degenerate and invalid ranges are safe', () => {
  assert.deepEqual(niceTicks(5, 5), [5]);
  assert.deepEqual(niceTicks(NaN, 10), []);
  // Reversed range is handled (treated as a span).
  const t = niceTicks(100, 0, 4);
  assert.ok(t.length >= 2);
});
