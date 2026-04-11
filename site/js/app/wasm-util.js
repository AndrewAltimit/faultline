/**
 * Utility for converting WASM-serialized data structures.
 *
 * serde_wasm_bindgen serializes Rust BTreeMap/HashMap as JS Map objects,
 * not plain objects. It also serializes newtype wrappers (e.g., FactionId(String))
 * as objects with a single "0" key. This module normalizes both.
 */

/**
 * Check if an object is a Rust newtype ID wrapper (e.g., FactionId("alpha")
 * serialized as { "0": "alpha" }).
 */
function isNewtypeWrapper(obj) {
  const keys = Object.keys(obj);
  return keys.length === 1 && keys[0] === '0' && typeof obj['0'] === 'string';
}

/**
 * Recursively convert all Map instances to plain objects and unwrap
 * newtype ID wrappers to plain strings.
 * @param {*} value
 * @returns {*}
 */
export function mapsToObjects(value) {
  if (value === null || value === undefined) {
    return value;
  }

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

  if (typeof value === 'object') {
    // Unwrap Rust newtype wrappers like FactionId("alpha") -> "alpha".
    if (isNewtypeWrapper(value)) {
      return value['0'];
    }

    const obj = {};
    for (const key of Object.keys(value)) {
      obj[key] = mapsToObjects(value[key]);
    }
    return obj;
  }

  return value;
}
