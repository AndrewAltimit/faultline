/**
 * Scenario sharing — encode/decode TOML into a URL hash fragment.
 *
 * Uses the browser's CompressionStream (gzip) to keep links manageable
 * for non-trivial scenarios, then base64url-encodes the bytes so the
 * payload is URL-safe and survives copy/paste.
 */

const HASH_KEY = 'scenario';

/**
 * Compress and base64url-encode a TOML string.
 * @param {string} toml
 * @returns {Promise<string>}
 */
export async function encodeScenario(toml) {
  const stream = new Blob([toml]).stream().pipeThrough(new CompressionStream('gzip'));
  const compressed = new Uint8Array(await new Response(stream).arrayBuffer());
  return base64UrlEncode(compressed);
}

/**
 * Decode a base64url string back into TOML.
 * @param {string} encoded
 * @returns {Promise<string>}
 */
export async function decodeScenario(encoded) {
  const bytes = base64UrlDecode(encoded);
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'));
  return await new Response(stream).text();
}

/**
 * Build a shareable URL for the given TOML, anchored at the current
 * page (so it works regardless of where the site is hosted).
 * @param {string} toml
 * @returns {Promise<string>}
 */
export async function buildShareUrl(toml) {
  const encoded = await encodeScenario(toml);
  const url = new URL(window.location.href);
  url.hash = `${HASH_KEY}=${encoded}`;
  return url.toString();
}

/**
 * If the current URL hash contains a shared scenario, return its
 * decoded TOML. Otherwise return null.
 * @returns {Promise<string|null>}
 */
export async function readScenarioFromHash() {
  const hash = window.location.hash.replace(/^#/, '');
  if (!hash) return null;
  const params = new URLSearchParams(hash);
  const encoded = params.get(HASH_KEY);
  if (!encoded) return null;
  try {
    return await decodeScenario(encoded);
  } catch (e) {
    console.warn('Failed to decode shared scenario from URL:', e);
    return null;
  }
}

/** Strip the scenario fragment from the current URL without reloading. */
export function clearScenarioHash() {
  if (window.location.hash.includes(`${HASH_KEY}=`)) {
    history.replaceState(null, '', window.location.pathname + window.location.search);
  }
}

// ---------------------------------------------------------------------------
// Base64URL helpers (no padding, '-'/'_' instead of '+'/'/').
// ---------------------------------------------------------------------------

function base64UrlEncode(bytes) {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function base64UrlDecode(str) {
  const padded = str.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((str.length + 3) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
