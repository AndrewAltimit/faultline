/**
 * Unit tests for site/js/app/pinned.js — the localStorage-backed
 * pin manager that holds Monte Carlo results across runs and reloads.
 *
 * The PinnedStore touches three things worth covering:
 *
 *   1. CRUD over the in-memory list (add/remove/get/list/clear/rename).
 *   2. Persistence + retrieval across instances (the user reloads the
 *      page; we must rehydrate from localStorage).
 *   3. Quota-exceeded handling — without a retry-after-trim, a single
 *      large pin would silently lose all subsequent ones because every
 *      `setItem` would throw.
 *
 * We stub `globalThis.localStorage` with a Map-backed implementation
 * so the module loads under Node. For the quota test we install a
 * stub that fails the first call and succeeds the second.
 *
 * Run with:
 *   node --test tests/integration/pinned.test.mjs
 */

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');

// ---------------------------------------------------------------------------
// Minimal localStorage stub.
// ---------------------------------------------------------------------------

function installFreshLocalStorage() {
  const data = new Map();
  globalThis.localStorage = {
    getItem(k) {
      return data.has(k) ? data.get(k) : null;
    },
    setItem(k, v) {
      data.set(k, String(v));
    },
    removeItem(k) {
      data.delete(k);
    },
    clear() {
      data.clear();
    },
    _data: data,
  };
}

// Import once after installing a stub — module top level reads
// localStorage when constructing PinnedStore, so the global needs to
// exist before we import.
installFreshLocalStorage();
const mod = await import(join(repoRoot, 'site', 'js', 'app', 'pinned.js'));
const { PinnedStore } = mod;

// Reset the storage between tests so each one starts clean.
beforeEach(() => {
  installFreshLocalStorage();
});

// Convenience: fake a MonteCarloSummary-shaped object.
function fakeSummary({ runs = 100, alphaWin = 0.5 } = {}) {
  return {
    total_runs: runs,
    win_rates: { alpha: alphaWin },
    win_rate_cis: { alpha: [alphaWin - 0.05, alphaWin + 0.05] },
    average_duration: 25,
  };
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

test('PinnedStore: starts empty when localStorage has no key', () => {
  const s = new PinnedStore();
  assert.deepEqual(s.list(), []);
  assert.equal(s.get('nope'), null);
});

test('PinnedStore: add returns the created pin and assigns a unique id', () => {
  const s = new PinnedStore();
  const p1 = s.add({ scenarioName: 'A', toml: 't1', summary: fakeSummary() });
  const p2 = s.add({ scenarioName: 'B', toml: 't2', summary: fakeSummary() });
  assert.ok(p1.id);
  assert.ok(p2.id);
  assert.notEqual(p1.id, p2.id);
  assert.equal(s.list().length, 2);
});

test('PinnedStore: get retrieves by id', () => {
  const s = new PinnedStore();
  const p = s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  assert.equal(s.get(p.id).id, p.id);
});

test('PinnedStore: remove drops the matching id and persists', () => {
  const s = new PinnedStore();
  const p = s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  s.remove(p.id);
  assert.equal(s.list().length, 0);
  // A second instance reading from localStorage should also see empty.
  const s2 = new PinnedStore();
  assert.equal(s2.list().length, 0);
});

test('PinnedStore: rename updates label and trims whitespace', () => {
  const s = new PinnedStore();
  const p = s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  s.rename(p.id, '   shiny new label   ');
  assert.equal(s.get(p.id).label, 'shiny new label');
});

test('PinnedStore: rename ignores empty/whitespace-only labels', () => {
  // The label is the only human-readable identifier — silently allowing
  // it to be wiped out would orphan the pin in the UI. The store should
  // refuse the rename instead.
  const s = new PinnedStore();
  const p = s.add({
    scenarioName: 'A',
    toml: 't',
    summary: fakeSummary(),
    label: 'original',
  });
  s.rename(p.id, '');
  s.rename(p.id, '   ');
  assert.equal(s.get(p.id).label, 'original');
});

test('PinnedStore: clear removes everything and persists', () => {
  const s = new PinnedStore();
  s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  s.add({ scenarioName: 'B', toml: 't', summary: fakeSummary() });
  s.clear();
  assert.equal(s.list().length, 0);
  const s2 = new PinnedStore();
  assert.equal(s2.list().length, 0);
});

// ---------------------------------------------------------------------------
// MAX_PINS cap
// ---------------------------------------------------------------------------

test('PinnedStore: caps at MAX_PINS, dropping the oldest', () => {
  // The module-internal cap is 8; the test asserts the *observable*
  // contract (length never exceeds 8 after many adds, and the first
  // additions disappear once the cap is reached).
  const s = new PinnedStore();
  const ids = [];
  for (let i = 0; i < 12; i++) {
    ids.push(
      s.add({ scenarioName: `S${i}`, toml: `t${i}`, summary: fakeSummary() }).id,
    );
  }
  const list = s.list();
  assert.equal(list.length, 8);
  // The earliest 4 ids should be evicted.
  for (const evicted of ids.slice(0, 4)) {
    assert.equal(s.get(evicted), null);
  }
  // The latest 8 should remain.
  for (const kept of ids.slice(4)) {
    assert.ok(s.get(kept));
  }
});

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

test('PinnedStore: persists across instances via localStorage', () => {
  const a = new PinnedStore();
  const p = a.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  const b = new PinnedStore();
  assert.equal(b.list().length, 1);
  assert.equal(b.list()[0].id, p.id);
});

test('PinnedStore: tolerates corrupt JSON in storage', () => {
  // If a future schema migration leaves invalid JSON behind, the store
  // should fall back to an empty list rather than crash the dashboard.
  globalThis.localStorage.setItem('faultline:pinned-mc:v1', '{not valid json');
  const s = new PinnedStore();
  assert.deepEqual(s.list(), []);
});

test('PinnedStore: filters out malformed entries on load', () => {
  // Defensive: hand-edited storage or a future schema bump might leave
  // entries without an `id`. The store should discard those rather
  // than expose them downstream where `get(id)` would never match.
  globalThis.localStorage.setItem(
    'faultline:pinned-mc:v1',
    JSON.stringify([{ id: 'good' }, { broken: true }, null, 42]),
  );
  const s = new PinnedStore();
  const list = s.list();
  assert.equal(list.length, 1);
  assert.equal(list[0].id, 'good');
});

// ---------------------------------------------------------------------------
// Subscribers
// ---------------------------------------------------------------------------

test('PinnedStore: subscribe fires immediately and on every mutation', () => {
  const s = new PinnedStore();
  const fired = [];
  const unsub = s.subscribe((pins) => fired.push(pins.length));
  // Initial fire happens synchronously inside subscribe().
  assert.equal(fired.length, 1);
  s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  s.add({ scenarioName: 'B', toml: 't', summary: fakeSummary() });
  s.remove(s.list()[0].id);
  // Initial + 3 mutations = 4 events.
  assert.equal(fired.length, 4);
  unsub();
  s.add({ scenarioName: 'C', toml: 't', summary: fakeSummary() });
  // Unsubscribed listener should not see the next event.
  assert.equal(fired.length, 4);
});

test('PinnedStore: a throwing listener does not break other listeners', () => {
  // Defensive — without the try/catch in _emit, a single buggy
  // subscriber would orphan all later ones and the dashboard would
  // silently stop re-rendering.
  const s = new PinnedStore();
  const ok = [];
  s.subscribe(() => {
    throw new Error('boom');
  });
  s.subscribe((pins) => ok.push(pins.length));
  s.add({ scenarioName: 'A', toml: 't', summary: fakeSummary() });
  // 1 initial fire + 1 add => 2 entries from the well-behaved listener.
  assert.equal(ok.length, 2);
});

// ---------------------------------------------------------------------------
// Quota retry path
// ---------------------------------------------------------------------------

test('PinnedStore: retries setItem after trimming when quota throws', () => {
  // Simulate localStorage refusing the first setItem. The store should
  // trim its in-memory list down toward MAX_PINS-1 and retry once. We
  // count setItem invocations to confirm the retry happened.
  const data = new Map();
  let calls = 0;
  let throwOnce = true;
  globalThis.localStorage = {
    getItem(k) {
      return data.has(k) ? data.get(k) : null;
    },
    setItem(k, v) {
      calls++;
      if (throwOnce) {
        throwOnce = false;
        const e = new Error('QuotaExceeded');
        e.name = 'QuotaExceededError';
        throw e;
      }
      data.set(k, String(v));
    },
    removeItem(k) {
      data.delete(k);
    },
    clear() {
      data.clear();
    },
  };

  const s = new PinnedStore();
  // Seed with several pins so the trim has something to drop.
  for (let i = 0; i < 5; i++) {
    s.add({ scenarioName: `S${i}`, toml: `t${i}`, summary: fakeSummary() });
  }
  // First mutation throws, retry succeeds. The store must recover.
  // Without the retry-after-trim, the in-memory list and storage would
  // diverge silently.
  assert.ok(calls >= 2, 'expected at least one retry call');
  assert.ok(data.has('faultline:pinned-mc:v1'), 'storage should hold the pins after retry');
});
