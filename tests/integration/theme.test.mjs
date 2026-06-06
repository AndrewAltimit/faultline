/**
 * Unit tests for the pure theme-resolution helpers in
 * site/js/app/theme.js. The ThemeController itself touches the DOM /
 * localStorage and is not exercised here.
 *
 * Run with: node --test tests/integration/theme.test.mjs
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

const { normalizeTheme, nextTheme, resolveInitialTheme, THEMES, STORAGE_KEY } = await import(
  join(repoRoot, 'site', 'js', 'app', 'theme.js')
);

test('THEMES are dark and light; storage key is namespaced', () => {
  assert.deepEqual(THEMES, ['dark', 'light']);
  assert.equal(STORAGE_KEY, 'faultline.theme');
});

test('normalizeTheme defaults anything non-light to dark', () => {
  assert.equal(normalizeTheme('light'), 'light');
  assert.equal(normalizeTheme('dark'), 'dark');
  assert.equal(normalizeTheme('bogus'), 'dark');
  assert.equal(normalizeTheme(undefined), 'dark');
  assert.equal(normalizeTheme(null), 'dark');
});

test('nextTheme flips between dark and light', () => {
  assert.equal(nextTheme('dark'), 'light');
  assert.equal(nextTheme('light'), 'dark');
  // Invalid current normalizes to dark first → flips to light.
  assert.equal(nextTheme('garbage'), 'light');
});

test('resolveInitialTheme prefers an explicit stored choice', () => {
  assert.equal(resolveInitialTheme('light', false), 'light');
  assert.equal(resolveInitialTheme('dark', true), 'dark');
});

test('resolveInitialTheme falls back to prefers-color-scheme', () => {
  assert.equal(resolveInitialTheme(null, true), 'light');
  assert.equal(resolveInitialTheme(null, false), 'dark');
  assert.equal(resolveInitialTheme(undefined, true), 'light');
});

test('resolveInitialTheme ignores an unrecognized stored value', () => {
  assert.equal(resolveInitialTheme('purple', true), 'light');
  assert.equal(resolveInitialTheme('purple', false), 'dark');
});
