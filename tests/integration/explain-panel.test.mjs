/**
 * Unit tests for site/js/app/explain-panel.js — the pure render helpers
 * powering the editor's Explain button and inline advisory-warnings panel
 * (Epic P).
 *
 * These functions are deliberately DOM-free and WASM-free: they take the
 * plain-object shapes the WASM exports return and emit HTML strings, so
 * they can be exercised headlessly. We assert on the structural signals
 * (panel title text, per-warning markers, escaping) rather than exact
 * whitespace, which can shift for cosmetic reasons.
 *
 * Run with:
 *   node --test tests/integration/explain-panel.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const mod = await import(join(repoRoot, 'site', 'js', 'app', 'explain-panel.js'));
const { renderWarnings, warningsClean, renderExplain, warningKindLabel, escapeHtml } = mod;

test('warningKindLabel maps known kinds, falls back for unknown', () => {
  assert.equal(warningKindLabel('faction_no_objective'), 'Faction has no objective');
  assert.equal(warningKindLabel('unreferenced_region'), 'Unreferenced region');
  assert.equal(warningKindLabel('unreachable_phase'), 'Unreachable kill-chain phase');
  // Unknown kind degrades gracefully to the raw tag.
  assert.equal(warningKindLabel('future_kind'), 'future_kind');
});

test('warningsClean reflects emptiness', () => {
  assert.equal(warningsClean({ warnings: [] }), true);
  assert.equal(warningsClean({ warnings: [{ kind: 'x', subject: 'y', message: 'z' }] }), false);
  assert.equal(warningsClean(null), true);
  assert.equal(warningsClean({}), true);
});

test('renderWarnings on a clean report shows the no-warnings state', () => {
  const html = renderWarnings({ warnings: [] });
  assert.match(html, /No advisory warnings/);
  // No per-warning items rendered.
  assert.equal(/class="warning-item"/.test(html), false);
});

test('renderWarnings lists each finding with kind, subject, message', () => {
  const report = {
    warnings: [
      {
        kind: 'faction_no_objective',
        subject: 'bravo',
        message: 'Faction `bravo` is named by no victory condition.',
      },
      {
        kind: 'unreachable_phase',
        subject: 'kc/island',
        message: 'phase `island` is unreachable.',
      },
    ],
  };
  const html = renderWarnings(report);
  // Count says 2; plural form.
  assert.match(html, /2 advisory warnings/);
  // Two item blocks.
  const items = html.match(/class="warning-item"/g) || [];
  assert.equal(items.length, 2);
  // Labels resolved from kinds.
  assert.match(html, /Faction has no objective/);
  assert.match(html, /Unreachable kill-chain phase/);
  // Subjects present.
  assert.match(html, /bravo/);
  assert.match(html, /kc\/island/);
});

test('renderWarnings uses singular for one finding', () => {
  const html = renderWarnings({
    warnings: [{ kind: 'unreferenced_region', subject: 'r1', message: 'm' }],
  });
  assert.match(html, /1 advisory warning(?!s)/);
});

test('renderWarnings escapes HTML in subject and message', () => {
  const html = renderWarnings({
    warnings: [
      {
        kind: 'unreferenced_region',
        subject: '<img src=x>',
        message: 'a & b < c > "d"',
      },
    ],
  });
  // No raw tag survives.
  assert.equal(html.includes('<img src=x>'), false);
  assert.match(html, /&lt;img src=x&gt;/);
  assert.match(html, /a &amp; b &lt; c &gt; &quot;d&quot;/);
});

test('renderExplain wraps markdown verbatim in a pre block, escaped', () => {
  const md = '# Scenario\n\n- factions: 2\n<script>alert(1)</script>';
  const html = renderExplain(md);
  assert.match(html, /^<pre>/);
  assert.match(html, /<\/pre>$/);
  // Markdown content preserved (heading sigil, list).
  assert.match(html, /# Scenario/);
  assert.match(html, /factions: 2/);
  // Embedded markup is escaped, not executable.
  assert.equal(html.includes('<script>'), false);
  assert.match(html, /&lt;script&gt;/);
});

test('renderExplain tolerates empty / missing markdown', () => {
  assert.equal(renderExplain(''), '<pre></pre>');
  assert.equal(renderExplain(undefined), '<pre></pre>');
});

test('escapeHtml covers the five breakout characters', () => {
  assert.equal(escapeHtml(`& < > " '`), '&amp; &lt; &gt; &quot; &#39;');
});
