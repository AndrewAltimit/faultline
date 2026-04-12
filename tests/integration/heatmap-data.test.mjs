/**
 * Unit tests for site/js/app/heatmap-data.js — the pure aggregation
 * functions that feed the dashboard's regional control heatmap and
 * sensitivity tornado chart.
 *
 * These functions are dependency-free, so we can exercise them
 * directly under Node without a DOM. Run with:
 *
 *   node --test tests/integration/heatmap-data.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const heatmapModule = await import(
  join(repoRoot, 'site', 'js', 'app', 'heatmap-data.js')
);
const { buildRegionalHeatmap, buildTornadoRanges } = heatmapModule;

// ---------------------------------------------------------------------------
// buildRegionalHeatmap
// ---------------------------------------------------------------------------

test('buildRegionalHeatmap returns null when no runs', () => {
  assert.equal(buildRegionalHeatmap(null), null);
  assert.equal(buildRegionalHeatmap([]), null);
});

test('buildRegionalHeatmap returns null when runs have no snapshots', () => {
  // The default MC run path skips snapshot collection — the heatmap
  // helper must signal "nothing to draw" so the dashboard hides the
  // chart cleanly instead of rendering an empty rectangle.
  const runs = [{ snapshots: [] }, { snapshots: undefined }];
  assert.equal(buildRegionalHeatmap(runs), null);
});

test('buildRegionalHeatmap aggregates plurality across runs', () => {
  // Two regions, two ticks, three runs:
  //   tick 5: north -> {alpha:2, bravo:1},  south -> {bravo:3}
  //   tick 10: north -> {alpha:3},          south -> {alpha:1, bravo:2}
  // Expected dominant:
  //   north@5  = alpha (2/3),  north@10 = alpha (3/3)
  //   south@5  = bravo (3/3),  south@10 = bravo (2/3)
  const runs = [
    {
      snapshots: [
        { tick: 5,  region_control: { north: 'alpha', south: 'bravo' } },
        { tick: 10, region_control: { north: 'alpha', south: 'alpha' } },
      ],
    },
    {
      snapshots: [
        { tick: 5,  region_control: { north: 'alpha', south: 'bravo' } },
        { tick: 10, region_control: { north: 'alpha', south: 'bravo' } },
      ],
    },
    {
      snapshots: [
        { tick: 5,  region_control: { north: 'bravo', south: 'bravo' } },
        { tick: 10, region_control: { north: 'alpha', south: 'bravo' } },
      ],
    },
  ];

  const heat = buildRegionalHeatmap(runs);
  assert.ok(heat, 'heatmap should be non-null');
  assert.deepEqual(heat.ticks, [5, 10]);
  assert.deepEqual(heat.regions, ['north', 'south']);

  assert.equal(heat.dominant.north[0].faction, 'alpha');
  assert.ok(Math.abs(heat.dominant.north[0].prob - 2 / 3) < 1e-9);
  assert.equal(heat.dominant.north[1].faction, 'alpha');
  assert.equal(heat.dominant.north[1].prob, 1.0);

  assert.equal(heat.dominant.south[0].faction, 'bravo');
  assert.equal(heat.dominant.south[0].prob, 1.0);
  assert.equal(heat.dominant.south[1].faction, 'bravo');
  assert.ok(Math.abs(heat.dominant.south[1].prob - 2 / 3) < 1e-9);
});

test('buildRegionalHeatmap handles neutral (null) regions as a synthetic faction', () => {
  // Snapshots may carry null values for uncontrolled regions. The
  // helper buckets them under '__neutral__' so the dashboard can color
  // them distinctly. Verify that bucketing works and that a region
  // dominated by null is reported with the synthetic id.
  const runs = [
    { snapshots: [{ tick: 1, region_control: { r1: null } }] },
    { snapshots: [{ tick: 1, region_control: { r1: null } }] },
    { snapshots: [{ tick: 1, region_control: { r1: 'alpha' } }] },
  ];
  const heat = buildRegionalHeatmap(runs);
  assert.equal(heat.dominant.r1[0].faction, '__neutral__');
  assert.ok(Math.abs(heat.dominant.r1[0].prob - 2 / 3) < 1e-9);
});

test('buildRegionalHeatmap returns null when no region_control entries', () => {
  // Edge case: snapshots exist but every region_control map is empty.
  // We treat this as "nothing to draw".
  const runs = [{ snapshots: [{ tick: 1, region_control: {} }] }];
  assert.equal(buildRegionalHeatmap(runs), null);
});

test('buildRegionalHeatmap tolerates runs with mixed snapshot counts', () => {
  // Real Monte Carlo batches end at different ticks because runs
  // terminate when victory fires. The aggregator must align everything
  // by tick, not by index, and not crash when one run has no snapshot
  // for a tick that another run does.
  const runs = [
    {
      snapshots: [
        { tick: 5, region_control: { r1: 'alpha' } },
        { tick: 10, region_control: { r1: 'alpha' } },
      ],
    },
    {
      snapshots: [
        { tick: 5, region_control: { r1: 'bravo' } },
        // No tick 10 — this run ended early.
      ],
    },
  ];
  const heat = buildRegionalHeatmap(runs);
  assert.deepEqual(heat.ticks, [5, 10]);
  // tick 5: alpha=1, bravo=1 → first encountered wins; alpha appears
  //         first because tickCounts is built in run order. But the
  //         exact tiebreaker is not contractual — what matters is
  //         that prob is 1/2 (count / total runs).
  assert.equal(heat.dominant.r1[0].prob, 0.5);
  // tick 10: only one run reported, so prob is 1/2 (one of two runs).
  assert.equal(heat.dominant.r1[1].faction, 'alpha');
  assert.equal(heat.dominant.r1[1].prob, 0.5);
});

// ---------------------------------------------------------------------------
// buildTornadoRanges
// ---------------------------------------------------------------------------

test('buildTornadoRanges returns empty for empty sweep', () => {
  assert.deepEqual(buildTornadoRanges({ outcomes: [] }), []);
  assert.deepEqual(buildTornadoRanges({}), []);
});

test('buildTornadoRanges computes per-faction min/max/range', () => {
  // Three sweep steps. Alpha swings from 0.2 → 0.8 (range 0.6),
  // Bravo swings from 0.3 → 0.4 (range 0.1). Tornado convention is
  // widest range first, so the result must be [alpha, bravo].
  const sens = {
    outcomes: [
      { win_rates: { alpha: 0.2, bravo: 0.3 } },
      { win_rates: { alpha: 0.5, bravo: 0.4 } },
      { win_rates: { alpha: 0.8, bravo: 0.35 } },
    ],
  };
  const ranges = buildTornadoRanges(sens);
  assert.equal(ranges.length, 2);
  assert.equal(ranges[0].fid, 'alpha');
  assert.ok(Math.abs(ranges[0].min - 0.2) < 1e-9);
  assert.ok(Math.abs(ranges[0].max - 0.8) < 1e-9);
  assert.ok(Math.abs(ranges[0].range - 0.6) < 1e-9);
  assert.equal(ranges[1].fid, 'bravo');
  assert.ok(Math.abs(ranges[1].range - 0.1) < 1e-9);
});

test('buildTornadoRanges treats missing entries as zero', () => {
  // If a faction is absent from one step's win_rates (because it had
  // no wins that step), the helper must treat it as 0.0 — otherwise
  // the range would be artificially compressed.
  const sens = {
    outcomes: [
      { win_rates: { alpha: 0.7 } },           // bravo absent → 0
      { win_rates: { alpha: 0.6, bravo: 0.3 } },
    ],
  };
  const ranges = buildTornadoRanges(sens);
  const bravo = ranges.find((r) => r.fid === 'bravo');
  assert.ok(bravo, 'bravo should appear once it has any nonzero step');
  assert.equal(bravo.min, 0);
  assert.ok(Math.abs(bravo.max - 0.3) < 1e-9);
  assert.ok(Math.abs(bravo.range - 0.3) < 1e-9);
});
