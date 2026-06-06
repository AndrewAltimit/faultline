/**
 * Unit tests for site/js/app/autocomplete.js — the pure TOML-context parsing
 * and schema-driven completion-source logic behind the editor's autocomplete
 * popover.
 *
 * Like the field-docs / explain-panel tests, these are DOM-free and WASM-free:
 * the functions take strings + offsets and emit plain objects, so they run
 * headlessly under `node --test`. The DOM popover lives in editor.js and is
 * exercised in the browser, not here.
 *
 * Run with:
 *   node --test tests/integration/autocomplete.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const acMod = await import(join(repoRoot, 'site', 'js', 'app', 'autocomplete.js'));
const docsMod = await import(join(repoRoot, 'site', 'js', 'app', 'field-docs.js'));

const { completionContext, keyCompletions, valueCompletions, completionsAt } = acMod;
const { FIELD_DOCS } = docsMod;

// ── completionContext: key vs value vs none ──────────────────────────

test('completionContext reports a key position with the typed prefix and section', () => {
  const text = '[factions.alpha]\nini';
  const ctx = completionContext(text, text.length);
  assert.equal(ctx.position, 'key');
  assert.equal(ctx.prefix, 'ini');
  assert.equal(ctx.section, 'factions');
  // tokenStart is the column where "ini" begins (column 0 of line 2).
  assert.equal(ctx.tokenStart, 0);
});

test('completionContext reports a value position with the assignment key', () => {
  const text = '[factions.alpha]\ndoctrine = Gue';
  const ctx = completionContext(text, text.length);
  assert.equal(ctx.position, 'value');
  assert.equal(ctx.key, 'doctrine');
  assert.equal(ctx.prefix, 'Gue');
  assert.equal(ctx.section, 'factions');
});

test('completionContext strips an opening quote from a value prefix', () => {
  const text = '[factions.alpha]\ndoctrine = "Gue';
  const ctx = completionContext(text, text.length);
  assert.equal(ctx.position, 'value');
  assert.equal(ctx.key, 'doctrine');
  assert.equal(ctx.prefix, 'Gue');
});

test('completionContext returns none on comment and header lines', () => {
  const header = '[factions.alpha]';
  assert.equal(completionContext(header, header.length).position, 'none');
  const comment = '# this is a comment';
  assert.equal(completionContext(comment, comment.length).position, 'none');
});

test('completionContext resolves the nested-section family for the caret', () => {
  const text = '[political_climate.media_landscape]\nstate';
  const ctx = completionContext(text, text.length);
  assert.equal(ctx.section, 'political_climate.media_landscape');
  assert.equal(ctx.prefix, 'state');
});

test('completionContext on an empty key token yields an empty prefix, still key', () => {
  const text = '[simulation]\n';
  const ctx = completionContext(text, text.length);
  assert.equal(ctx.position, 'key');
  assert.equal(ctx.prefix, '');
});

// ── keyCompletions: section filtering + prefix matching ──────────────

test('keyCompletions only offers keys valid in the caret section', () => {
  const sim = keyCompletions('simulation', '');
  const labels = sim.map((c) => c.label);
  assert.ok(labels.includes('max_ticks'), 'max_ticks belongs to [simulation]');
  assert.ok(labels.includes('attrition_model'), 'attrition_model belongs to [simulation]');
  // A faction-only key must not leak into the simulation section.
  assert.ok(!labels.includes('initial_morale'), 'initial_morale is faction-only');
});

test('keyCompletions includes generic (any-section) keys everywhere', () => {
  // `name` / `description` / `id` are generic (section "(any)") so they should
  // be offered in any concrete section.
  const labels = keyCompletions('events', '').map((c) => c.label);
  assert.ok(labels.includes('name'));
  assert.ok(labels.includes('description'));
});

test('keyCompletions filters by prefix, prefix-matches before substring-matches', () => {
  const res = keyCompletions('simulation', 'max');
  assert.ok(res.length > 0);
  assert.equal(res[0].label, 'max_ticks');
  for (const c of res) {
    assert.match(c.label.toLowerCase(), /max/);
  }
});

test('keyCompletions carries the engine-effect flag, type, and summary from the catalog', () => {
  const res = keyCompletions('simulation', 'max_ticks');
  const item = res.find((c) => c.label === 'max_ticks');
  assert.ok(item);
  assert.equal(item.kind, 'key');
  assert.equal(item.type, 'u32');
  assert.equal(item.engineEffect, true);
  assert.match(item.summary, /tick/i);
});

test('keyCompletions resolves the section-specific variant of a multi-meaning key', () => {
  // `effects` under [technology] vs [events] — completion should describe the
  // right one in each section.
  const tech = keyCompletions('technology', 'effects').find((c) => c.label === 'effects');
  assert.ok(tech);
  assert.match(tech.summary, /tech card/i);
  const evt = keyCompletions('events', 'effects').find((c) => c.label === 'effects');
  assert.ok(evt);
  assert.match(evt.summary, /event fires/i);
});

test('keyCompletions offers top-level keys only at the top level', () => {
  const top = keyCompletions('', '').map((c) => c.label);
  assert.ok(top.includes('attacker_budget'), 'attacker_budget is a top-level key');
  // A section-specific key must not show at the top level (no section context).
  assert.ok(!top.includes('max_ticks'));
});

// ── valueCompletions: enum variants ──────────────────────────────────

test('valueCompletions offers enum variants for a known enum field', () => {
  const res = valueCompletions('doctrine', 'factions', '');
  const labels = res.map((c) => c.label);
  assert.ok(labels.includes('Guerrilla'));
  assert.ok(labels.includes('Conventional'));
  for (const c of res) {
    assert.equal(c.kind, 'value');
    assert.equal(c.type, 'enum value');
  }
});

test('valueCompletions filters enum variants by typed prefix (case-insensitive)', () => {
  const res = valueCompletions('doctrine', 'factions', 'gue');
  assert.deepEqual(res.map((c) => c.label), ['Guerrilla']);
});

test('valueCompletions returns nothing for a non-enum or unknown field', () => {
  assert.deepEqual(valueCompletions('max_ticks', 'simulation', ''), []);
  assert.deepEqual(valueCompletions('not_a_field', 'simulation', ''), []);
});

test('valueCompletions resolves the right multi-variant enum by section', () => {
  // `metric` is an enum only in the [kill_chains] variant (the analogue
  // variant is a tagged enum without enumValues).
  const km = valueCompletions('metric', 'kill_chains', '').map((c) => c.label);
  assert.ok(km.includes('CoercionPressure'));
  // Under the historical-analogue section the metric variant has no enumValues.
  assert.deepEqual(valueCompletions('metric', 'meta.historical_analogue', ''), []);
});

// ── completionsAt: end-to-end resolver ───────────────────────────────

test('completionsAt returns key completions at a key position', () => {
  const text = '[simulation]\nmax';
  const { context, items } = completionsAt(text, text.length);
  assert.equal(context.position, 'key');
  assert.ok(items.some((c) => c.label === 'max_ticks'));
});

test('completionsAt returns enum value completions at a value position', () => {
  const text = '[factions.alpha]\ndoctrine = "Gue';
  const { context, items } = completionsAt(text, text.length);
  assert.equal(context.position, 'value');
  assert.deepEqual(items.map((c) => c.label), ['Guerrilla']);
});

test('completionsAt yields no items on a header or comment line', () => {
  const { items } = completionsAt('[factions.alpha]', 5);
  assert.deepEqual(items, []);
});

// ── catalog integrity: enumValues stay in sync with the summary prose ──

test('every enumValues entry appears verbatim in its catalog summary', () => {
  // Guards against the structured variant list drifting from the human-readable
  // summary text the hover tooltip shows. (The reverse — every prose variant
  // having an enumValues entry — is intentionally not required, since not every
  // enum field carries value completions yet.)
  const visit = (doc, key) => {
    if (!Array.isArray(doc.enumValues)) return;
    assert.ok(doc.enumValues.length > 0, `${key}: enumValues must be non-empty`);
    for (const v of doc.enumValues) {
      assert.ok(
        doc.summary.includes(v),
        `${key}: enum value "${v}" must appear in the summary "${doc.summary}"`,
      );
    }
  };
  for (const [key, entry] of Object.entries(FIELD_DOCS)) {
    if (Array.isArray(entry)) entry.forEach((v) => visit(v, key));
    else visit(entry, key);
  }
});
