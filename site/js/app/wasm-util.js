/**
 * Utility for converting WASM-serialized data structures.
 *
 * serde_wasm_bindgen serializes Rust BTreeMap/HashMap as JS Map objects,
 * not plain objects. This module converts them recursively so the rest
 * of the app can use standard Object.entries/keys/values.
 */

/**
 * Recursively convert all Map instances in a value to plain objects.
 * @param {*} value
 * @returns {*}
 */
export function mapsToObjects(value) {
  if (value instanceof Map) {
    const obj = {};
    for (const [k, v] of value) {
      obj[k] = mapsToObjects(v);
    }
    return obj;
  }

  if (Array.isArray(value)) {
    return value.map(mapsToObjects);
  }

  if (value !== null && typeof value === 'object') {
    const obj = {};
    for (const key of Object.keys(value)) {
      obj[key] = mapsToObjects(value[key]);
    }
    return obj;
  }

  return value;
}
