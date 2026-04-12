/**
 * Unit tests for site/js/app/sharing.js — round-trip TOML through the
 * gzip+base64url URL hash encoding used by the Share button.
 *
 * Run with: node --test tests/integration/sharing.test.mjs
 *
 * The sharing module touches `window` and `URL` for the URL builder,
 * but the pure encode/decode primitives only need the global
 * CompressionStream / DecompressionStream APIs that landed in Node 18+.
 * We stub the minimal globals needed and import the module directly.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

// Stub a minimal `window` so the URL builder doesn't crash. The tests
// only use encode/decode, but importing sharing.js evaluates the file
// top-to-bottom — and `buildShareUrl` references `window.location`.
globalThis.window = {
  location: { href: 'https://example.com/app.html' },
};

const sharing = await import(
  join(repoRoot, 'site', 'js', 'app', 'sharing.js')
);

test('encodeScenario / decodeScenario round-trip preserves TOML exactly', async () => {
  const original = `# Tutorial: Symmetric Conflict
[meta]
name = "Round-trip test"
description = """multi
line
description"""
tags = ["a", "b", "c"]

[map.source]
type = "Grid"
width = 4
height = 3
`;

  const encoded = await sharing.encodeScenario(original);
  assert.equal(typeof encoded, 'string');
  // Base64URL alphabet: A–Z, a–z, 0–9, '-', '_'. No '+', '/', '=', or whitespace.
  assert.match(encoded, /^[A-Za-z0-9_-]+$/);

  const decoded = await sharing.decodeScenario(encoded);
  assert.equal(decoded, original, 'TOML must round-trip byte-identical');
});

test('encodeScenario shrinks large repetitive payloads', async () => {
  // Real scenarios are repetitive (region tables, faction blocks).
  // Compression should pay for the base64 overhead at any reasonable
  // size. Use the actual tutorial scenario as the most realistic
  // input we have.
  const tomlPath = join(
    repoRoot,
    'scenarios',
    'tutorial_symmetric.toml',
  );
  const original = readFileSync(tomlPath, 'utf8');
  const encoded = await sharing.encodeScenario(original);

  // Base64URL inflates by ~4/3 over raw bytes. We require the
  // compressed+encoded payload to be smaller than the raw text — i.e.
  // gzip won enough to overcome the encoding overhead.
  assert.ok(
    encoded.length < original.length,
    `encoded length (${encoded.length}) should be < original length (${original.length})`,
  );

  // And the round-trip must still work.
  const decoded = await sharing.decodeScenario(encoded);
  assert.equal(decoded, original);
});

test('encodeScenario handles unicode correctly', async () => {
  // Scenario names contain em dashes and other non-ASCII characters
  // (see scenarios/tutorial_symmetric.toml). gzip is byte-oriented but
  // the Blob constructor must encode the string as UTF-8 first — make
  // sure that handshake doesn't drop characters.
  const original = 'name = "Tutorial \u2014 Symmetric \u00e9\u00e8 \u4e2d\u6587"';
  const encoded = await sharing.encodeScenario(original);
  const decoded = await sharing.decodeScenario(encoded);
  assert.equal(decoded, original);
});

test('encodeScenario handles empty string', async () => {
  // Edge case: an empty editor shouldn't crash the share path.
  const encoded = await sharing.encodeScenario('');
  const decoded = await sharing.decodeScenario(encoded);
  assert.equal(decoded, '');
});

test('decodeScenario rejects malformed input', async () => {
  // Garbage that survives base64 decoding but isn't valid gzip should
  // surface as an error rather than silently producing empty output.
  await assert.rejects(
    async () => {
      await sharing.decodeScenario('not_a_real_gzip_payload');
    },
    'malformed payload should reject',
  );
});

test('buildShareUrl produces a hash containing the encoded scenario', async () => {
  // The bootstrap path reads the hash via `URLSearchParams(hash)`
  // looking for a `scenario=` key. Verify buildShareUrl produces a URL
  // with exactly that shape so the integration loop closes.
  const toml = '[meta]\nname = "x"\n';
  const url = await sharing.buildShareUrl(toml);
  const parsed = new URL(url);
  assert.match(parsed.hash, /^#scenario=/);

  const params = new URLSearchParams(parsed.hash.slice(1));
  const encoded = params.get('scenario');
  assert.ok(encoded, 'hash must contain a scenario= entry');
  const decoded = await sharing.decodeScenario(encoded);
  assert.equal(decoded, toml);
});
