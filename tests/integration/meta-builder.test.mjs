/**
 * Unit tests for site/js/app/meta-builder.js — the browser-side
 * authoring helper for the self-describing scenario `[meta]` block.
 *
 * These mirror the Rust `validate_meta` rules: present-but-empty strings
 * and whitespace-only list entries are rejected; absent fields are
 * simply omitted from the serialized output.
 *
 * Run with: node --test tests/integration/meta-builder.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

const { validateMeta, buildMetaLines, SCENARIO_TYPES } = await import(
  join(repoRoot, 'site', 'js', 'app', 'meta-builder.js')
);

test('validateMeta accepts a fully-populated meta object', () => {
  const errors = validateMeta({
    analytical_purpose: 'Does early isolation shift the win rate?',
    scenario_type: 'red_team',
    red_team_profile: 'Two-vector convergence.',
    blue_team_posture: 'Layered controls.',
    osint_sources: ['CISA advisory', 'GAO report'],
    sensitivity_parameters: ['propagation detection'],
  });
  assert.deepEqual(errors, []);
});

test('validateMeta accepts an empty object (all fields absent)', () => {
  assert.deepEqual(validateMeta({}), []);
});

test('validateMeta rejects a present-but-empty analytical_purpose', () => {
  const errors = validateMeta({ analytical_purpose: '   ' });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /analytical_purpose/);
});

test('validateMeta rejects an empty red_team_profile', () => {
  const errors = validateMeta({ red_team_profile: '' });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /red_team_profile/);
});

test('validateMeta rejects a whitespace-only osint_sources entry', () => {
  const errors = validateMeta({ osint_sources: ['good', '  '] });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /osint_sources\[1\]/);
});

test('validateMeta rejects an unknown scenario_type', () => {
  const errors = validateMeta({ scenario_type: 'not_a_type' });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /scenario_type/);
});

test('validateMeta accepts a whitespace-padded valid scenario_type', () => {
  const errors = validateMeta({ scenario_type: '  red_team  ' });
  assert.equal(errors.length, 0);
});

test('buildMetaLines trims a whitespace-padded scenario_type', () => {
  const out = buildMetaLines({ scenario_type: '  red_team  ' });
  assert.ok(out.split('\n').includes('scenario_type = "red_team"'));
});

test('SCENARIO_TYPES lists the schema enum values', () => {
  assert.deepEqual(SCENARIO_TYPES, [
    'tutorial',
    'red_team',
    'calibration',
    'demo',
    'reference',
  ]);
});

test('buildMetaLines emits only populated fields, no header', () => {
  const out = buildMetaLines({
    analytical_purpose: 'Quantify the convergence window.',
    scenario_type: 'red_team',
    osint_sources: ['CISA advisory', 'GAO report'],
  });
  const lines = out.split('\n');
  assert.ok(lines.includes('scenario_type = "red_team"'));
  assert.ok(lines.includes('analytical_purpose = "Quantify the convergence window."'));
  assert.ok(lines.includes('osint_sources = ["CISA advisory", "GAO report"]'));
  // Absent fields must not appear.
  assert.ok(!out.includes('red_team_profile'));
  assert.ok(!out.includes('blue_team_posture'));
  assert.ok(!out.includes('sensitivity_parameters'));
  // No header — caller splices into an existing [meta] table.
  assert.ok(!out.includes('[meta]'));
});

test('buildMetaLines omits empty lists entirely', () => {
  const out = buildMetaLines({
    analytical_purpose: 'x',
    osint_sources: [],
    sensitivity_parameters: [],
  });
  assert.equal(out, 'analytical_purpose = "x"');
});

test('buildMetaLines escapes embedded quotes', () => {
  const out = buildMetaLines({
    analytical_purpose: 'Probe the "isolation" window.',
  });
  assert.equal(out, 'analytical_purpose = "Probe the \\"isolation\\" window."');
});

test('buildMetaLines throws on invalid fields', () => {
  assert.throws(
    () => buildMetaLines({ analytical_purpose: '   ' }),
    /invalid meta fields/,
  );
});
