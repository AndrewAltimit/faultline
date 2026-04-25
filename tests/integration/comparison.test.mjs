/**
 * Unit tests for site/js/app/comparison.js — pure delta computation
 * and HTML rendering for the Pin/Compare workflow.
 *
 * The contract worth pinning here is the parity with
 * `faultline_stats::counterfactual::compute_delta`. If a future change
 * to the Rust delta semantics (e.g. how missing factions are handled,
 * or which fields are considered) is not mirrored here, the dashboard
 * will quietly drift from the CLI report. The tests below assert the
 * specific behaviors the Rust side guarantees.
 *
 * Run with:
 *   node --test tests/integration/comparison.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const mod = await import(join(repoRoot, 'site', 'js', 'app', 'comparison.js'));
const { computeDelta, renderComparison } = mod;

// ---------------------------------------------------------------------------
// Helper: build a MonteCarloSummary-shaped object for tests.
// ---------------------------------------------------------------------------

function summary({
  total_runs = 100,
  average_duration = 50,
  win_rates = {},
  win_rate_cis = {},
  campaign_summaries = {},
} = {}) {
  return { total_runs, average_duration, win_rates, win_rate_cis, campaign_summaries };
}

function chain({
  overall_success_rate = 0,
  detection_rate = 0,
  cost_asymmetry_ratio = 0,
  mean_attacker_spend = 0,
  mean_defender_spend = 0,
} = {}) {
  return {
    overall_success_rate,
    detection_rate,
    cost_asymmetry_ratio,
    mean_attacker_spend,
    mean_defender_spend,
  };
}

// ---------------------------------------------------------------------------
// computeDelta
// ---------------------------------------------------------------------------

test('computeDelta: variant - baseline for matched factions', () => {
  // Mirrors the Rust contract: positive delta means variant > baseline.
  const b = summary({ win_rates: { alpha: 0.4 } });
  const v = summary({ win_rates: { alpha: 0.6 } });
  const d = computeDelta(b, v);
  assert.ok(Math.abs(d.win_rate_deltas.alpha - 0.2) < 1e-9);
});

test('computeDelta: missing-on-variant treats variant as 0', () => {
  // Documented in Rust counterfactual.rs:228-238 — the union of faction
  // ids is iterated and the missing side defaults to 0.0. Without this
  // a faction that lost all wins under the counterfactual would be
  // silently dropped from the report.
  const b = summary({ win_rates: { alpha: 0.4, beta: 0.6 } });
  const v = summary({ win_rates: { alpha: 0.6 } });
  const d = computeDelta(b, v);
  assert.ok(Math.abs(d.win_rate_deltas.beta - -0.6) < 1e-9);
});

test('computeDelta: missing-on-baseline treats baseline as 0', () => {
  const b = summary({ win_rates: { alpha: 0.4 } });
  const v = summary({ win_rates: { alpha: 0.6, gamma: 0.1 } });
  const d = computeDelta(b, v);
  assert.ok(Math.abs(d.win_rate_deltas.gamma - 0.1) < 1e-9);
});

test('computeDelta: chain feasibility deltas mirror Rust ChainDelta', () => {
  const b = summary({
    campaign_summaries: {
      'chain.x': chain({
        overall_success_rate: 0.5,
        detection_rate: 0.3,
        cost_asymmetry_ratio: 100,
        mean_attacker_spend: 1000,
        mean_defender_spend: 100000,
      }),
    },
  });
  const v = summary({
    campaign_summaries: {
      'chain.x': chain({
        overall_success_rate: 0.7,
        detection_rate: 0.2,
        cost_asymmetry_ratio: 50,
        mean_attacker_spend: 1500,
        mean_defender_spend: 50000,
      }),
    },
  });
  const d = computeDelta(b, v);
  const cd = d.chain_deltas['chain.x'];
  assert.ok(Math.abs(cd.overall_success_rate_delta - 0.2) < 1e-9);
  assert.ok(Math.abs(cd.detection_rate_delta - -0.1) < 1e-9);
  assert.ok(Math.abs(cd.cost_asymmetry_ratio_delta - -50) < 1e-9);
  assert.ok(Math.abs(cd.attacker_spend_delta - 500) < 1e-9);
  assert.ok(Math.abs(cd.defender_spend_delta - -50000) < 1e-9);
});

test('computeDelta: chain present only on one side defaults the other to 0', () => {
  // Same union semantics as faction win-rate deltas. A scenario that
  // adds a new kill chain in the variant should surface that chain with
  // its full magnitude as the delta, not a silent absence.
  const b = summary({});
  const v = summary({
    campaign_summaries: { 'chain.new': chain({ overall_success_rate: 0.5 }) },
  });
  const d = computeDelta(b, v);
  assert.ok(Math.abs(d.chain_deltas['chain.new'].overall_success_rate_delta - 0.5) < 1e-9);
});

test('computeDelta: mean_duration_delta is variant - baseline', () => {
  const d = computeDelta(
    summary({ average_duration: 10 }),
    summary({ average_duration: 12.5 }),
  );
  assert.ok(Math.abs(d.mean_duration_delta - 2.5) < 1e-9);
});

test('computeDelta: handles entirely empty summaries without throwing', () => {
  // Defensive — early in the user flow the dashboard might wire up
  // comparison rendering before a real summary lands. Returning empty
  // delta maps is much better than crashing the panel.
  const d = computeDelta({}, {});
  assert.deepEqual(d.win_rate_deltas, {});
  assert.deepEqual(d.chain_deltas, {});
  assert.equal(d.mean_duration_delta, 0);
});

test('computeDelta: tolerates null inputs', () => {
  // The dashboard re-renders on subscriber events that may fire before
  // a baseline is selected. Accept null without throwing.
  const d = computeDelta(null, null);
  assert.deepEqual(d.win_rate_deltas, {});
  assert.deepEqual(d.chain_deltas, {});
});

// ---------------------------------------------------------------------------
// renderComparison
// ---------------------------------------------------------------------------

test('renderComparison: emits a panel containing both labels', () => {
  const html = renderComparison(
    { label: 'Baseline-A', summary: summary({ win_rates: { alpha: 0.5 } }) },
    { label: 'Variant-B', summary: summary({ win_rates: { alpha: 0.6 } }) },
  );
  assert.ok(html.includes('Baseline-A'));
  assert.ok(html.includes('Variant-B'));
  assert.ok(html.includes('Win Rates'));
});

test('renderComparison: shows sample-mismatch warning when total_runs differ', () => {
  // Documented UX deviation from the Rust report: pinned results can
  // come from different MC batches, so we surface the sample-size delta
  // as a warning so analysts don't read the deltas as pure scenario
  // effects.
  const html = renderComparison(
    { label: 'A', summary: summary({ total_runs: 100, win_rates: { x: 0.5 } }) },
    { label: 'B', summary: summary({ total_runs: 1000, win_rates: { x: 0.5 } }) },
  );
  assert.ok(html.includes('Sample sizes differ'));
  assert.ok(html.includes('100'));
  assert.ok(html.includes('1000'));
});

test('renderComparison: no warning when sample sizes match', () => {
  const html = renderComparison(
    { label: 'A', summary: summary({ total_runs: 100, win_rates: { x: 0.5 } }) },
    { label: 'B', summary: summary({ total_runs: 100, win_rates: { x: 0.6 } }) },
  );
  assert.ok(!html.includes('Sample sizes differ'));
});

test('renderComparison: shows Wilson CIs when present, omits cleanly when absent', () => {
  const withCis = renderComparison(
    {
      label: 'A',
      summary: summary({
        win_rates: { alpha: 0.5 },
        win_rate_cis: { alpha: [0.4, 0.6] },
      }),
    },
    {
      label: 'B',
      summary: summary({
        win_rates: { alpha: 0.55 },
        win_rate_cis: { alpha: [0.45, 0.65] },
      }),
    },
  );
  assert.ok(withCis.includes('cmp-ci'));
  // Both sides should show the percent sign on each bound — review
  // caught the original `[40.0–60.0%]` shorthand and replaced it.
  assert.ok(/\[\d+\.\d%–\d+\.\d%\]/.test(withCis), 'CI should render with % on both bounds');

  const without = renderComparison(
    { label: 'A', summary: summary({ win_rates: { alpha: 0.5 } }) },
    { label: 'B', summary: summary({ win_rates: { alpha: 0.55 } }) },
  );
  // The .cmp-ci class still appears in chain rows, so we just check
  // there's no Wilson interval bracket anywhere when CIs are absent.
  assert.ok(!/\[\d+\.\d%–/.test(without));
});

test('renderComparison: escapes HTML in faction ids and labels', () => {
  // Defense-in-depth: faction ids can contain any string (the engine
  // newtypes wrap String). If an analyst names a faction
  // `<script>alert(1)</script>` it must render as text, not run.
  const html = renderComparison(
    { label: '<X>', summary: summary({ win_rates: { '<f1>': 0.5 } }) },
    { label: '<Y>', summary: summary({ win_rates: { '<f1>': 0.6 } }) },
  );
  assert.ok(!html.includes('<X>'), 'raw bracket must not appear');
  assert.ok(!html.includes('<f1>'), 'raw faction id must not appear');
  assert.ok(html.includes('&lt;X&gt;'));
  assert.ok(html.includes('&lt;f1&gt;'));
});

test('renderComparison: cost-asymmetry section renders ratio cells', () => {
  const html = renderComparison(
    {
      label: 'A',
      summary: summary({
        campaign_summaries: { 'c.1': chain({ cost_asymmetry_ratio: 100 }) },
      }),
    },
    {
      label: 'B',
      summary: summary({
        campaign_summaries: { 'c.1': chain({ cost_asymmetry_ratio: 50 }) },
      }),
    },
  );
  assert.ok(html.includes('Cost Asymmetry'));
  assert.ok(html.includes('100×'));
  assert.ok(html.includes('50×'));
});

test('renderComparison: zero-or-negative cost-asymmetry renders as em-dash', () => {
  // The Rust report uses 0 as a sentinel "no ratio measurable" because
  // dividing by a zero attacker spend is undefined. Mirror the
  // dashboard's tabular fallback so a 0 doesn't read as "ratio of 0×".
  const html = renderComparison(
    {
      label: 'A',
      summary: summary({
        campaign_summaries: { 'c.1': chain({ cost_asymmetry_ratio: 0 }) },
      }),
    },
    {
      label: 'B',
      summary: summary({
        campaign_summaries: { 'c.1': chain({ cost_asymmetry_ratio: 0 }) },
      }),
    },
  );
  // The em-dash sentinel must appear at least once — both cells use it
  // when ratio <= 0.
  assert.ok(html.includes('—'));
});
