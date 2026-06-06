/**
 * Self-describing `[meta]` block builder.
 *
 * The scenario `[meta]` block carries analyst-facing provenance and
 * intent fields (`analytical_purpose`, `scenario_type`, `osint_sources`,
 * `red_team_profile`, `blue_team_posture`, `sensitivity_parameters`).
 * These are validated at scenario load by
 * `faultline_types::scenario::validate_meta`: a present-but-empty string
 * or a whitespace-only list entry is rejected as a silent-no-op shape.
 *
 * This module is the browser-side authoring counterpart: it takes the
 * raw form-field values, applies the *same* present-but-empty rejection
 * rules client-side (so the analyst gets immediate feedback instead of a
 * load-time error), and serializes a `[meta]` fragment ready to merge
 * into a scenario TOML.
 *
 * Pure logic — no DOM, no `window`. The `MetaFields` form component in
 * `site/js/app/meta-form.js` wires these helpers to inputs; the
 * functions here are independently unit-tested in
 * `tests/integration/meta-builder.test.mjs`.
 */

/** The `scenario_type` enum values accepted by the schema (snake_case). */
export const SCENARIO_TYPES = [
  'tutorial',
  'red_team',
  'calibration',
  'demo',
  'reference',
];

/**
 * @typedef {object} MetaFields
 * @property {string} [analytical_purpose]
 * @property {string} [scenario_type]            One of SCENARIO_TYPES.
 * @property {string} [red_team_profile]
 * @property {string} [blue_team_posture]
 * @property {string[]} [osint_sources]
 * @property {string[]} [sensitivity_parameters]
 */

/**
 * Validate a `MetaFields` object against the same rules the Rust loader
 * enforces. Returns an array of human-readable error strings; an empty
 * array means the fields are valid.
 *
 * Rules (mirroring `validate_meta`):
 *   - Optional strings that are *present but whitespace-only* are
 *     rejected. (Absent / undefined is fine — the field is just omitted.)
 *   - List entries that are empty or whitespace-only are rejected.
 *   - `scenario_type`, when present, must be one of SCENARIO_TYPES.
 *
 * @param {MetaFields} fields
 * @returns {string[]}
 */
export function validateMeta(fields) {
  const errors = [];
  const f = fields || {};

  for (const key of ['analytical_purpose', 'red_team_profile', 'blue_team_posture']) {
    const v = f[key];
    if (v !== undefined && v !== null && String(v).trim() === '') {
      errors.push(`meta.${key} is present but empty; remove it or give it content.`);
    }
  }

  if (f.scenario_type !== undefined && f.scenario_type !== null) {
    const scenarioType = String(f.scenario_type).trim();
    if (scenarioType !== '' && !SCENARIO_TYPES.includes(scenarioType)) {
      errors.push(
        `meta.scenario_type "${f.scenario_type}" is not one of: ${SCENARIO_TYPES.join(', ')}.`,
      );
    }
  }

  for (const key of ['osint_sources', 'sensitivity_parameters']) {
    const list = f[key];
    if (!Array.isArray(list)) continue;
    list.forEach((entry, idx) => {
      if (String(entry).trim() === '') {
        errors.push(`meta.${key}[${idx}] is empty or whitespace-only; remove it or give it content.`);
      }
    });
  }

  return errors;
}

/**
 * Serialize a single TOML string value with the same escaping the rest
 * of the app uses (`JSON.stringify` produces a valid TOML basic string
 * for the characters we emit here).
 *
 * @param {string} s
 * @returns {string}
 */
function tomlString(s) {
  return JSON.stringify(String(s));
}

/**
 * Serialize a TOML inline array of strings.
 *
 * @param {string[]} list
 * @returns {string}
 */
function tomlStringArray(list) {
  return `[${list.map((s) => tomlString(s)).join(', ')}]`;
}

/**
 * Build the self-describing `[meta]` field lines from a `MetaFields`
 * object. Only fields that carry content are emitted — an absent or
 * empty-list field is omitted entirely (matching the schema's
 * `skip_serializing_if` behavior), so the result never contains a
 * present-but-empty shape.
 *
 * The returned string is the *field lines only* (no `[meta]` header), so
 * callers can splice it into an existing `[meta]` table. Throws if the
 * fields fail {@link validateMeta} — the caller should validate and
 * surface errors before serializing.
 *
 * @param {MetaFields} fields
 * @returns {string}  Newline-joined `key = value` lines (no trailing newline).
 */
export function buildMetaLines(fields) {
  const errors = validateMeta(fields);
  if (errors.length > 0) {
    throw new Error(`invalid meta fields:\n${errors.join('\n')}`);
  }

  const f = fields || {};
  const lines = [];

  if (f.scenario_type !== undefined && f.scenario_type !== null) {
    const scenarioType = String(f.scenario_type).trim();
    if (scenarioType !== '') {
      lines.push(`scenario_type = ${tomlString(scenarioType)}`);
    }
  }
  for (const key of ['analytical_purpose', 'red_team_profile', 'blue_team_posture']) {
    const v = f[key];
    if (v !== undefined && v !== null && String(v).trim() !== '') {
      lines.push(`${key} = ${tomlString(v)}`);
    }
  }
  for (const key of ['osint_sources', 'sensitivity_parameters']) {
    const list = f[key];
    if (Array.isArray(list) && list.length > 0) {
      lines.push(`${key} = ${tomlStringArray(list)}`);
    }
  }

  return lines.join('\n');
}
