/**
 * Unit tests for site/js/app/field-docs.js — the schema-aware field
 * documentation catalog and the pure lookup logic powering the editor's
 * hover-documentation tooltip, plus the `renderFieldDoc` render helper in
 * explain-panel.js.
 *
 * Like the explain-panel tests, these are deliberately DOM-free and
 * WASM-free: the lookup functions take strings + offsets and emit plain
 * objects / HTML, so they run headlessly under `node --test`.
 *
 * Run with:
 *   node --test tests/integration/field-docs.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const docsMod = await import(join(repoRoot, 'site', 'js', 'app', 'field-docs.js'));
const panelMod = await import(join(repoRoot, 'site', 'js', 'app', 'explain-panel.js'));

const {
  FIELD_DOCS,
  lookupFieldDoc,
  keyAtOffset,
  enclosingSection,
  canonicalizeSection,
  docAtOffset,
  documentedFieldCount,
} = docsMod;
const { renderFieldDoc } = panelMod;

test('catalog documents a broad set of keys across the schema', () => {
  // Breadth guard: assert a healthy minimum so an accidental truncation of
  // the catalog fails loudly rather than silently degrading the feature.
  assert.ok(documentedFieldCount() >= 90, `expected >= 90 documented keys, got ${documentedFieldCount()}`);
  // Spot-check coverage of each major schema family the prompt called out.
  for (const k of [
    'schema_version', // meta
    'population', // map
    'initial_morale', // faction
    'max_ticks', // simulation
    'base_success_probability', // kill_chains
    'enabled', // belief_model
    'condition', // victory_conditions / fracture
    'category', // technology
    'tension', // political_climate
  ]) {
    assert.ok(FIELD_DOCS[k], `catalog should document "${k}"`);
  }
});

test('every catalog entry has the required FieldDoc shape', () => {
  const checkDoc = (d, key) => {
    assert.equal(typeof d.summary, 'string', `${key}: summary must be a string`);
    assert.ok(d.summary.length > 0, `${key}: summary must be non-empty`);
    assert.equal(typeof d.type, 'string', `${key}: type must be a string`);
    assert.equal(typeof d.engineEffect, 'boolean', `${key}: engineEffect must be boolean`);
  };
  for (const [key, entry] of Object.entries(FIELD_DOCS)) {
    if (Array.isArray(entry)) {
      assert.ok(entry.length > 0, `${key}: variant array must be non-empty`);
      entry.forEach((v) => checkDoc(v, key));
    } else {
      checkDoc(entry, key);
    }
  }
});

test('lookupFieldDoc returns null for unknown keys and resolves the key', () => {
  assert.equal(lookupFieldDoc('definitely_not_a_field'), null);
  assert.equal(lookupFieldDoc(''), null);
  const d = lookupFieldDoc('max_ticks');
  assert.equal(d.key, 'max_ticks');
  assert.equal(d.engineEffect, true);
});

test('lookupFieldDoc disambiguates multi-meaning keys by section', () => {
  // `effects` means different things under [technology] vs [events].
  const tech = lookupFieldDoc('effects', 'technology.surveillance_pkg');
  assert.match(tech.summary, /tech card/i);
  const evt = lookupFieldDoc('effects', 'events.power_loss');
  assert.match(evt.summary, /event fires/i);

  // `condition` under fracture vs victory conditions.
  const fracture = lookupFieldDoc('condition', 'factions.gray.alliance_fracture');
  assert.match(fracture.summary, /alliance-fracture/i);
  const victory = lookupFieldDoc('condition', 'victory_conditions.alpha_control');
  assert.match(victory.summary, /victory condition/i);

  // `effectiveness` resolves the most specific section among 3 variants.
  const leader = lookupFieldDoc('effectiveness', 'factions.bravo.leadership');
  assert.match(leader.summary, /leadership rank/i);
  const inst = lookupFieldDoc('effectiveness', 'factions.bravo.faction_type.institutions.fbi');
  assert.match(inst.summary, /institution/i);
});

test('lookupFieldDoc prefers the deepest matching section variant', () => {
  // `criticality` exists under [map.infrastructure] and [networks].
  const infra = lookupFieldDoc('criticality', 'map.infrastructure.grid');
  assert.match(infra.summary, /damage scoring/i);
  const net = lookupFieldDoc('criticality', 'networks.logistics.nodes.depot1');
  assert.match(net.summary, /network node/i);
});

test('lookupFieldDoc falls back gracefully when section is unknown/absent', () => {
  // Multi-variant key, no section context: returns first variant, not null.
  const d = lookupFieldDoc('effects');
  assert.ok(d);
  assert.equal(d.key, 'effects');
});

test('canonicalizeSection elides instance ids under known families', () => {
  assert.equal(canonicalizeSection('factions.alpha'), 'factions');
  assert.equal(canonicalizeSection('factions.alpha.forces.tank'), 'factions.forces');
  assert.equal(
    canonicalizeSection('kill_chains.red.phases.recon.cost'),
    'kill_chains.phases.cost',
  );
  assert.equal(canonicalizeSection('political_climate.media_landscape'), 'political_climate.media_landscape');
  assert.equal(canonicalizeSection(''), '');
});

test('keyAtOffset extracts the key only on the key side of an assignment', () => {
  const line = '  initial_morale = 0.8';
  // Offset on the key word.
  assert.equal(keyAtOffset(line, 4), 'initial_morale');
  // Offset on the value side returns null (it's not a key).
  const valIdx = line.indexOf('0.8');
  assert.equal(keyAtOffset(line, valIdx), null);
});

test('keyAtOffset ignores comments and section headers', () => {
  assert.equal(keyAtOffset('# a comment about tension', 12), null);
  assert.equal(keyAtOffset('[political_climate]', 5), null);
  assert.equal(keyAtOffset('', 0), null);
});

test('keyAtOffset handles a bare key inside a multi-line array context', () => {
  // A line with no `=` whose first token is the key (e.g. inline-table member
  // on its own line). The first identifier is treated as the key.
  assert.equal(keyAtOffset('faction = "alpha"', 2), 'faction');
});

test('enclosingSection walks back to the nearest header', () => {
  const lines = [
    '[meta]',
    'name = "x"',
    '',
    '[factions.alpha]',
    'initial_morale = 0.8',
    'intelligence = 0.5',
  ];
  assert.equal(enclosingSection(lines, 5), 'factions.alpha');
  assert.equal(enclosingSection(lines, 1), 'meta');
  // Above the first header → top-level.
  assert.equal(enclosingSection(['x = 1', 'y = 2'], 1), '');
});

test('enclosingSection handles array-of-tables headers', () => {
  const lines = ['[[map.terrain]]', 'terrain_type = "Urban"'];
  assert.equal(enclosingSection(lines, 1), 'map.terrain');
});

test('docAtOffset resolves the right doc end-to-end with section context', () => {
  const text = [
    '[political_climate]',
    'tension = 0.3',
    '',
    '[political_climate.media_landscape]',
    'state_control = 0.4',
  ].join('\n');

  // Hover the `tension` key.
  const tIdx = text.indexOf('tension') + 2;
  const tDoc = docAtOffset(text, tIdx);
  assert.ok(tDoc);
  assert.equal(tDoc.key, 'tension');
  assert.match(tDoc.summary, /tension/i);

  // Hover `state_control` (nested media_landscape section).
  const sIdx = text.indexOf('state_control') + 3;
  const sDoc = docAtOffset(text, sIdx);
  assert.ok(sDoc);
  assert.equal(sDoc.key, 'state_control');
  assert.match(sDoc.summary, /media/i);
});

test('docAtOffset returns null when the offset is on a value or undocumented key', () => {
  const text = '[simulation]\nmax_ticks = 50\nbogus_field = 1';
  // On the numeric value.
  const valIdx = text.indexOf('50');
  assert.equal(docAtOffset(text, valIdx), null);
  // On an undocumented key.
  const bogusIdx = text.indexOf('bogus_field') + 2;
  assert.equal(docAtOffset(text, bogusIdx), null);
});

test('renderFieldDoc emits key, type, effect badge, and summary; escapes input', () => {
  const html = renderFieldDoc({
    key: 'max_ticks',
    type: 'u32',
    summary: 'Hard cap on simulation length.',
    engineEffect: true,
  });
  assert.match(html, /field-doc-key">max_ticks/);
  assert.match(html, /field-doc-type">u32/);
  assert.match(html, /has-effect/);
  assert.match(html, /engine effect/);
  assert.match(html, /Hard cap on simulation length\./);

  // Descriptive-only fields get the no-effect badge.
  const desc = renderFieldDoc({
    key: 'author',
    type: 'string',
    summary: 'Author handle.',
    engineEffect: false,
  });
  assert.match(desc, /no-effect/);
  assert.match(desc, /descriptive only/);

  // null / missing doc → empty string.
  assert.equal(renderFieldDoc(null), '');
  assert.equal(renderFieldDoc({}), '');
});

test('renderFieldDoc escapes HTML in catalog text and renders range/default', () => {
  const html = renderFieldDoc({
    key: 'x',
    type: 'f64',
    range: '[0, 1]',
    default: '0.05',
    summary: '<script>alert(1)</script> & "quotes"',
    engineEffect: true,
  });
  assert.equal(html.includes('<script>'), false);
  assert.match(html, /&lt;script&gt;/);
  assert.match(html, /field-doc-range">\[0, 1\]/);
  assert.match(html, /default 0\.05/);
});

test('every documented key is resolvable through lookupFieldDoc', () => {
  // Round-trip guard: catalog keys must all resolve (no typo-only entries).
  for (const key of Object.keys(FIELD_DOCS)) {
    const d = lookupFieldDoc(key);
    assert.ok(d, `key "${key}" should resolve`);
    assert.equal(d.key, key);
  }
});
