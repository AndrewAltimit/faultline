/**
 * Tests for the site/scenarios -> ../scenarios symlink introduced in
 * Epic G to deduplicate the WASM-served scenario mirror against the
 * canonical source.
 *
 * The contract worth pinning:
 *
 *   1. `site/scenarios` is a symlink (not a regular directory of
 *      stale copies — that was the pre-Epic-G state, and easy for a
 *      future contributor to accidentally recreate).
 *   2. The symlink target is `../scenarios` (relative — matters because
 *      the GitHub Pages deploy uploads `site/` and a relative target
 *      is what the deploy-time materialization step rewrites correctly).
 *   3. Every file readable through the symlink is byte-identical to
 *      the corresponding file in `scenarios/`. If someone ever
 *      replaces the symlink with a copy that drifts, this catches it.
 *
 * Run with:
 *   node --test tests/integration/site-scenarios-symlink.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lstatSync, readlinkSync, readdirSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const sourceDir = join(repoRoot, 'scenarios');
const mirrorPath = join(repoRoot, 'site', 'scenarios');

test('site/scenarios is a symlink, not a directory', () => {
  const stat = lstatSync(mirrorPath);
  assert.ok(
    stat.isSymbolicLink(),
    'site/scenarios should be a symlink to dedupe against scenarios/. ' +
      'If a contributor replaced it with a real directory, restore the symlink ' +
      'with: rm -rf site/scenarios && ln -s ../scenarios site/scenarios',
  );
});

test('site/scenarios symlink target is "../scenarios"', () => {
  // Relative target is required because the GitHub Pages deploy
  // uploads only site/, so the symlink can't point at an absolute
  // path that won't exist on the deploy host. The deploy workflow's
  // "Materialize site/scenarios" step also assumes a relative target.
  const target = readlinkSync(mirrorPath);
  assert.equal(target, '../scenarios');
});

test('every scenario file is byte-identical via symlink and source', () => {
  // If the symlink ever points at a different snapshot of the
  // scenarios directory (e.g. someone deleted and recreated the link
  // pointing somewhere else), per-file byte comparison surfaces it.
  const sourceFiles = readdirSync(sourceDir).filter((f) => f.endsWith('.toml')).sort();
  assert.ok(sourceFiles.length > 0, 'scenarios/ should contain at least one .toml file');

  for (const filename of sourceFiles) {
    const fromSource = readFileSync(join(sourceDir, filename));
    const fromMirror = readFileSync(join(mirrorPath, filename));
    assert.deepEqual(
      fromMirror,
      fromSource,
      `${filename}: site/scenarios/ resolves to different content than scenarios/`,
    );
  }
});

test('site/scenarios exposes the same file listing as scenarios/', () => {
  // A symlink to a stale directory could happen to byte-match individual
  // files but expose a different *set* of files (e.g. missing a newly
  // added scenario). This guards the directory-membership contract.
  const sourceListing = readdirSync(sourceDir).sort();
  const mirrorListing = readdirSync(mirrorPath).sort();
  assert.deepEqual(mirrorListing, sourceListing);
});
