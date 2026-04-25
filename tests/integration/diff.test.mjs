/**
 * Unit tests for site/js/app/diff.js — the LCS-based unified-diff
 * renderer powering the editor's Diff modal.
 *
 * The contract worth pinning here is correctness of the *operations*
 * the LCS walk emits, not the exact HTML structure (which can change
 * for cosmetic reasons). We assert by inspecting the rendered HTML
 * for marker classes (.diff-add, .diff-del, .diff-eq) and the lines
 * those markers wrap — that's a stable enough signal without
 * over-specifying the markup.
 *
 * Run with:
 *   node --test tests/integration/diff.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const mod = await import(join(repoRoot, 'site', 'js', 'app', 'diff.js'));
const { renderDiff, hasDifferences } = mod;

// Count occurrences of a class marker in the rendered HTML. Using the
// class instead of the literal "+"/"-" sigil avoids false matches from
// scenario content that happens to start with those characters.
function countLines(html, cls) {
  const re = new RegExp(`class="diff-line ${cls}"`, 'g');
  return (html.match(re) || []).length;
}

// ---------------------------------------------------------------------------
// hasDifferences
// ---------------------------------------------------------------------------

test('hasDifferences: identical strings are equal', () => {
  assert.equal(hasDifferences('a\nb\nc', 'a\nb\nc'), false);
});

test('hasDifferences: any change is different', () => {
  assert.equal(hasDifferences('a\nb\nc', 'a\nb\nd'), true);
});

test('hasDifferences: null/undefined coerce to empty string', () => {
  assert.equal(hasDifferences(null, ''), false);
  assert.equal(hasDifferences(undefined, ''), false);
  assert.equal(hasDifferences(null, 'x'), true);
});

// ---------------------------------------------------------------------------
// renderDiff: identical-input fast path
// ---------------------------------------------------------------------------

test('renderDiff: identical text returns the no-difference message', () => {
  const html = renderDiff('foo\nbar\n', 'foo\nbar\n', {
    baselineLabel: 'old',
    variantLabel: 'new',
  });
  assert.ok(html.includes('No differences'));
  assert.ok(html.includes('old'));
  assert.ok(html.includes('new'));
  // The fast path should not emit any actual diff lines.
  assert.equal(countLines(html, 'diff-add'), 0);
  assert.equal(countLines(html, 'diff-del'), 0);
});

// ---------------------------------------------------------------------------
// renderDiff: basic LCS correctness
// ---------------------------------------------------------------------------

test('renderDiff: a single line change emits one add and one del', () => {
  const html = renderDiff('foo\nbar\nbaz\n', 'foo\nQUX\nbaz\n');
  assert.equal(countLines(html, 'diff-add'), 1);
  assert.equal(countLines(html, 'diff-del'), 1);
  assert.ok(html.includes('bar'));
  assert.ok(html.includes('QUX'));
});

test('renderDiff: pure insertion produces only adds', () => {
  // Baseline is empty — every line in variant is new. The walk should
  // emit only `add` ops, no `del`. Regression test: an off-by-one in
  // the LCS backtrack could leak phantom `del` rows.
  const html = renderDiff('', 'a\nb\nc\n');
  assert.equal(countLines(html, 'diff-del'), 0);
  assert.ok(countLines(html, 'diff-add') >= 3);
});

test('renderDiff: pure deletion produces only dels', () => {
  const html = renderDiff('a\nb\nc\n', '');
  assert.equal(countLines(html, 'diff-add'), 0);
  assert.ok(countLines(html, 'diff-del') >= 3);
});

test('renderDiff: completely disjoint inputs produce all-add + all-del', () => {
  // Use inputs without trailing newlines — `'x\ny\nz\n'.split('\n')`
  // produces a phantom trailing empty string that *does* match between
  // both inputs, which would (correctly) leave one diff-eq context line
  // in the rendered hunk. The intent of this test is "no shared lines
  // → all add/del", so we strip the noise here.
  const html = renderDiff('x\ny\nz', 'a\nb\nc');
  assert.ok(countLines(html, 'diff-add') >= 3);
  assert.ok(countLines(html, 'diff-del') >= 3);
  assert.equal(countLines(html, 'diff-eq'), 0);
});

test('renderDiff: equal lines render with the diff-eq class for context', () => {
  // 3 lines of context is the default; the unchanged middle line should
  // appear in the rendered hunk.
  const html = renderDiff('a\nb\nc\nd\ne\n', 'a\nb\nC\nd\ne\n');
  assert.ok(countLines(html, 'diff-eq') >= 1);
  assert.equal(countLines(html, 'diff-add'), 1);
  assert.equal(countLines(html, 'diff-del'), 1);
});

// ---------------------------------------------------------------------------
// renderDiff: HTML escaping
// ---------------------------------------------------------------------------

test('renderDiff: escapes HTML special chars in line content', () => {
  // TOML string literals can contain arbitrary text including angle
  // brackets. Without escaping, a line like `name = "<img onerror=...>"`
  // would inject markup into the modal. This test pins the escaping
  // contract.
  const html = renderDiff('a = "<old>"\n', 'a = "<new>"\n');
  assert.ok(html.includes('&lt;old&gt;'));
  assert.ok(html.includes('&lt;new&gt;'));
  assert.ok(!html.includes('<old>'));
  assert.ok(!html.includes('<new>'));
});

test('renderDiff: escapes HTML special chars in labels', () => {
  const html = renderDiff('a\n', 'b\n', {
    baselineLabel: '<script>',
    variantLabel: '<img>',
  });
  assert.ok(html.includes('&lt;script&gt;'));
  assert.ok(html.includes('&lt;img&gt;'));
  assert.ok(!html.includes('<script>'));
});

// ---------------------------------------------------------------------------
// renderDiff: hunk grouping
// ---------------------------------------------------------------------------

test('renderDiff: distant changes produce multiple hunk headers', () => {
  // Two single-line changes separated by a long run of identical lines
  // (more than 2*context) should split into two hunks. Each hunk gets
  // its own `@@ ... @@` header.
  const baseline = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j'].join('\n') + '\n';
  const variant = ['A', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'J'].join('\n') + '\n';
  const html = renderDiff(baseline, variant);
  const hunkCount = (html.match(/diff-hunk-header/g) || []).length;
  assert.equal(hunkCount, 2, 'two distant changes should produce two hunks');
});

test('renderDiff: nearby changes merge into a single hunk', () => {
  // Two changes with only one identical line between them must merge —
  // splitting would print that single line twice as overlapping context.
  const baseline = 'a\nb\nc\n';
  const variant = 'A\nb\nC\n';
  const html = renderDiff(baseline, variant);
  const hunkCount = (html.match(/diff-hunk-header/g) || []).length;
  assert.equal(hunkCount, 1, 'adjacent changes should merge into one hunk');
});

test('renderDiff: hunk header line counts match the contained ops', () => {
  // `@@ -aStart,aLen +bStart,bLen @@` — aLen is the count of eq+del
  // ops in the hunk, bLen is eq+add. Inputs without trailing newlines
  // so the line counts equal `n`, not `n+1`.
  const html = renderDiff('a\nb\nc\nd', 'a\nB\nc\nD');
  const m = html.match(/@@ -(\d+),(\d+) \+(\d+),(\d+) @@/);
  assert.ok(m, 'hunk header should be present');
  const aLen = parseInt(m[2], 10);
  const bLen = parseInt(m[4], 10);
  // 2 changed + 2 unchanged on each side -> 4 each.
  assert.equal(aLen, 4);
  assert.equal(bLen, 4);
});

// ---------------------------------------------------------------------------
// renderDiff: edge cases
// ---------------------------------------------------------------------------

test('renderDiff: trailing newline difference is detected', () => {
  // `'a\nb'.split('\n')` -> ['a', 'b']
  // `'a\nb\n'.split('\n')` -> ['a', 'b', ''] — a phantom empty string.
  // The diff should report this as an added empty line, not silently
  // hide a real difference.
  const html = renderDiff('a\nb', 'a\nb\n');
  assert.notEqual(html.includes('No differences'), true);
});

test('renderDiff: empty inputs produce no-difference output', () => {
  const html = renderDiff('', '');
  assert.ok(html.includes('No differences'));
});

test('renderDiff: handles null/undefined inputs by coercing to empty', () => {
  // Editor flows can call renderDiff before the user has loaded
  // anything. Crashing the modal on null input would be a regression.
  const html = renderDiff(null, undefined);
  assert.ok(html.includes('No differences'));
});

test('renderDiff: respects context option', () => {
  // With context=0 the unchanged neighbors should not appear in any
  // hunk. Confirms the `Math.max(0, opts.context ?? 3)` clamp in
  // renderDiff actually flows through to hunk extraction.
  const html = renderDiff('a\nb\nc\nd\ne\n', 'a\nb\nC\nd\ne\n', { context: 0 });
  // Only the change should be emitted as a diff-line; no diff-eq rows.
  assert.equal(countLines(html, 'diff-eq'), 0);
});
