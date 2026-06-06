/**
 * Unit tests for site/js/app/export.js — the pure data-shaping behind the
 * Monte Carlo result export (JSON / CSV). The download helpers touch the
 * DOM and are not exercised here.
 *
 * Run with: node --test tests/integration/export.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

const {
  summaryToExportObject,
  summaryToJSON,
  summaryToCsv,
  csvEscape,
  rowsToCsv,
  exportFilename,
} = await import(join(repoRoot, 'site', 'js', 'app', 'export.js'));

function sampleSummary() {
  return {
    total_runs: 200,
    win_rates: { attacker: 0.625, defender: 0.375 },
    metric_distributions: {
      Duration: {
        mean: 42.5,
        median: 40,
        std_dev: 8.25,
        min: 20,
        max: 78,
        percentile_5: 28,
        percentile_95: 60,
      },
      FinalTension: { mean: 0.9, median: 0.9, std_dev: 0, min: 0.9, max: 0.9, percentile_5: 0.9, percentile_95: 0.9 },
      TotalCasualties: {
        mean: 1200,
        median: 1100,
        std_dev: 300,
        min: 500,
        max: 2500,
        percentile_5: 700,
        percentile_95: 1900,
      },
    },
    feasibility_matrix: [
      {
        chain_id: 'c1',
        chain_name: 'Grid intrusion',
        technology_readiness: 0.8,
        operational_complexity: 0.4,
        detection_probability: 0.3,
        success_probability: 0.55,
        consequence_severity: 0.7,
        attribution_difficulty: 0.65,
        cost_asymmetry_ratio: 250,
      },
    ],
  };
}

// ---------------------------------------------------------------------------
// summaryToExportObject
// ---------------------------------------------------------------------------

test('summaryToExportObject pulls win rates, distributions, feasibility', () => {
  const obj = summaryToExportObject(sampleSummary(), { scenarioName: 'Demo' });
  assert.equal(obj.scenario_name, 'Demo');
  assert.equal(obj.total_runs, 200);
  assert.deepEqual(obj.win_rates, { attacker: 0.625, defender: 0.375 });
  assert.equal(obj.metric_distributions.Duration.mean, 42.5);
  assert.equal(obj.metric_distributions.TotalCasualties.percentile_95, 1900);
  assert.equal(obj.feasibility_matrix.length, 1);
  assert.equal(obj.feasibility_matrix[0].chain_name, 'Grid intrusion');
  assert.equal(obj.feasibility_matrix[0].cost_asymmetry_ratio, 250);
});

test('summaryToExportObject excludes the tension metric', () => {
  const obj = summaryToExportObject(sampleSummary());
  assert.ok(!('FinalTension' in obj.metric_distributions));
  assert.ok('Duration' in obj.metric_distributions);
});

test('summaryToExportObject omits empty sections', () => {
  const obj = summaryToExportObject({ total_runs: 0 });
  assert.equal(obj.total_runs, 0);
  assert.ok(!('win_rates' in obj));
  assert.ok(!('metric_distributions' in obj));
  assert.ok(!('feasibility_matrix' in obj));
});

test('summaryToExportObject tolerates a null summary', () => {
  assert.deepEqual(summaryToExportObject(null), {});
});

test('summaryToJSON round-trips to valid JSON', () => {
  const parsed = JSON.parse(summaryToJSON(sampleSummary(), { scenarioName: 'X' }));
  assert.equal(parsed.scenario_name, 'X');
  assert.equal(parsed.win_rates.attacker, 0.625);
});

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

test('csvEscape quotes fields with commas, quotes, newlines', () => {
  assert.equal(csvEscape('plain'), 'plain');
  assert.equal(csvEscape('a,b'), '"a,b"');
  assert.equal(csvEscape('she said "hi"'), '"she said ""hi"""');
  assert.equal(csvEscape('line1\nline2'), '"line1\nline2"');
  assert.equal(csvEscape(null), '');
  assert.equal(csvEscape(42), '42');
});

test('rowsToCsv joins with CRLF and comma', () => {
  const csv = rowsToCsv([
    ['a', 'b'],
    [1, 2],
  ]);
  assert.equal(csv, 'a,b\r\n1,2');
});

test('summaryToCsv emits a section column and one row per stat field', () => {
  const csv = summaryToCsv(sampleSummary(), { scenarioName: 'Demo' });
  const lines = csv.split('\r\n');
  assert.equal(lines[0], 'section,key,metric,value');
  assert.ok(lines.includes('meta,scenario_name,,Demo'));
  assert.ok(lines.includes('meta,total_runs,,200'));
  assert.ok(lines.includes('win_rate,attacker,,0.625'));
  // Seven distribution fields for Duration.
  const durRows = lines.filter((l) => l.startsWith('distribution,Duration,'));
  assert.equal(durRows.length, 7);
  // Tension excluded from CSV too.
  assert.ok(!lines.some((l) => l.includes('FinalTension')));
  // Feasibility: seven axes for the one chain.
  const feasRows = lines.filter((l) => l.startsWith('feasibility,Grid intrusion,'));
  assert.equal(feasRows.length, 7);
});

test('summaryToCsv quotes a chain name containing a comma', () => {
  const summary = sampleSummary();
  summary.feasibility_matrix[0].chain_name = 'Grid, water';
  const csv = summaryToCsv(summary);
  assert.ok(csv.includes('feasibility,"Grid, water",technology_readiness,0.8'));
});

// ---------------------------------------------------------------------------
// exportFilename
// ---------------------------------------------------------------------------

test('exportFilename slugifies the scenario name', () => {
  assert.equal(exportFilename('US Institutional Fracture', 'json'), 'us-institutional-fracture.json');
  assert.equal(exportFilename('  Weird/Name!! ', 'csv'), 'weird-name.csv');
  assert.equal(exportFilename('', 'png'), 'faultline-results.png');
  assert.equal(exportFilename(null, 'json'), 'faultline-results.json');
});
