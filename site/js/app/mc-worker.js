/**
 * Monte Carlo web worker.
 *
 * Loads the WASM module in its own thread so the main UI thread stays
 * responsive while a batch run executes. The worker exposes a tiny
 * request/response protocol over postMessage:
 *
 *   { id, type: 'run', toml, runs, seed? }  →
 *   { id, type: 'result', result }          // success
 *   { id, type: 'error',  error }           // failure
 *
 * The `id` field lets the dashboard correlate responses with the
 * specific run that triggered them.
 */

let wasmReady = null;

async function ensureWasm() {
  if (!wasmReady) {
    wasmReady = (async () => {
      const mod = await import('../../pkg/faultline_backend_wasm.js');
      await mod.default();
      mod.init();
      return mod;
    })();
  }
  return wasmReady;
}

self.onmessage = async (ev) => {
  const { id, type, toml, runs, seed, collectSnapshots } = ev.data || {};
  if (type !== 'run') return;

  try {
    const wasm = await ensureWasm();
    const seedArg = typeof seed === 'number' ? seed : undefined;
    const snapsArg = collectSnapshots === true ? true : undefined;
    const result = wasm.run_monte_carlo(toml, runs, seedArg, snapsArg);
    // serde_wasm_bindgen returns Maps; convert to plain objects so the
    // structured-clone postMessage delivers an inspectable JSON shape.
    const plain = mapsToObjects(result);
    self.postMessage({ id, type: 'result', result: plain });
  } catch (e) {
    self.postMessage({ id, type: 'error', error: String(e) });
  }
};

/** Recursively convert Map instances to plain objects. */
function mapsToObjects(value) {
  if (value instanceof Map) {
    const out = {};
    for (const [k, v] of value.entries()) out[k] = mapsToObjects(v);
    return out;
  }
  if (Array.isArray(value)) return value.map(mapsToObjects);
  if (value && typeof value === 'object') {
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = mapsToObjects(v);
    return out;
  }
  return value;
}
