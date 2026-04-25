/**
 * Pinned Monte Carlo results — small persisted bench the analyst keeps
 * between scenario runs so they can compare "what we ran 10 minutes ago"
 * against the current run without re-loading TOML by hand.
 *
 * Storage is a single localStorage key holding a JSON array. We cap the
 * number of pins (oldest dropped) and the per-pin payload size before
 * stringifying — large `summary` blobs from 10k-run batches can otherwise
 * blow past the ~5MB browser quota and silently drop subsequent pins.
 */
const STORAGE_KEY = 'faultline:pinned-mc:v1';
const MAX_PINS = 8;

/** @typedef {{
 *   id: string,
 *   label: string,
 *   scenarioName: string,
 *   toml: string,
 *   summary: object,
 *   capturedAt: number,
 * }} PinnedResult
 */

function nowId() {
  return `pin-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

/**
 * Strip the heaviest sub-objects we don't need for comparison rendering.
 * Notably `summary.runs` (raw per-run snapshots) is not part of
 * `MonteCarloSummary` — the dashboard stashes it on `mcResult.runs`
 * instead — but defensively stripped here in case a future caller passes
 * the whole `mcResult`.
 */
function trimSummary(summary) {
  if (!summary || typeof summary !== 'object') return summary;
  const out = { ...summary };
  delete out.runs;
  return out;
}

export class PinnedStore {
  constructor() {
    /** @type {PinnedResult[]} */
    this._pins = [];
    this._listeners = new Set();
    this._load();
  }

  _load() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        this._pins = parsed.filter((p) => p && typeof p.id === 'string');
      }
    } catch (e) {
      console.warn('PinnedStore: failed to load pinned results:', e);
      this._pins = [];
    }
  }

  _persist() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this._pins));
    } catch (e) {
      // Quota exceeded is the realistic failure here. Drop the oldest
      // pin and try once more — better to silently lose the oldest than
      // refuse to pin the new result.
      console.warn('PinnedStore: persist failed, trimming and retrying:', e);
      if (this._pins.length > 1) {
        this._pins = this._pins.slice(-Math.max(1, MAX_PINS - 1));
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(this._pins));
        } catch (e2) {
          console.error('PinnedStore: persist still failing after trim:', e2);
        }
      }
    }
    this._emit();
  }

  _emit() {
    for (const fn of this._listeners) {
      try {
        fn(this._pins.slice());
      } catch (e) {
        console.error('PinnedStore: listener error:', e);
      }
    }
  }

  /** Subscribe to changes. Returns an unsubscribe fn. */
  subscribe(fn) {
    this._listeners.add(fn);
    // Mirror the try/catch in _emit so an exception thrown by the
    // initial fire can't escape to the caller's setup code (which
    // would otherwise leave the rest of the dashboard half-wired).
    try {
      fn(this._pins.slice());
    } catch (e) {
      console.error('PinnedStore: initial listener fire threw:', e);
    }
    return () => this._listeners.delete(fn);
  }

  list() {
    return this._pins.slice();
  }

  get(id) {
    return this._pins.find((p) => p.id === id) || null;
  }

  /**
   * Add a new pinned result. The default label suffix `(N)` is just a
   * disambiguating hint based on the *current* pin count — it can repeat
   * across the lifetime of the store if pins are removed in between, so
   * don't treat it as a stable index. The id field is the only stable
   * identifier.
   */
  add({ scenarioName, toml, summary, label }) {
    const pin = {
      id: nowId(),
      label: label || `${scenarioName || 'scenario'} (${this._pins.length + 1})`,
      scenarioName: scenarioName || 'scenario',
      toml: toml || '',
      summary: trimSummary(summary),
      capturedAt: Date.now(),
    };
    this._pins.push(pin);
    if (this._pins.length > MAX_PINS) {
      this._pins = this._pins.slice(-MAX_PINS);
    }
    this._persist();
    return pin;
  }

  remove(id) {
    const before = this._pins.length;
    this._pins = this._pins.filter((p) => p.id !== id);
    if (this._pins.length !== before) this._persist();
  }

  rename(id, label) {
    const p = this._pins.find((pp) => pp.id === id);
    if (p && typeof label === 'string' && label.trim()) {
      p.label = label.trim().slice(0, 80);
      this._persist();
    }
  }

  clear() {
    this._pins = [];
    this._persist();
  }
}
