/**
 * Schema-driven autocomplete for the TOML scenario editor.
 *
 * This module is the pure context-parsing + completion-source logic behind the
 * editor's autocomplete popover. It deliberately holds **no DOM state** so it
 * can be unit-tested headlessly under `node --test`, mirroring the
 * `field-docs.js` / `explain-panel.js` pattern. The editor (`editor.js`) owns
 * the popover DOM and merely calls {@link completionsAt} on each keystroke.
 *
 * The completion *source* is the existing field-doc catalog in
 * `field-docs.js` — we do not duplicate the schema. For each documented key we
 * already know its `type`, `range`, `default`, `engineEffect` badge, and the
 * `section` family it belongs to; that is exactly the metadata a completion
 * item wants to show. Section context (which `[table]` the caret is in) is
 * resolved with the same `enclosingSection` / `canonicalizeSection` helpers the
 * hover docs use, so key suggestions are filtered to the keys that are valid
 * *here*.
 *
 * Enum **value** completions are driven by an optional `enumValues` array that
 * a catalog entry may carry (e.g. `doctrine` → Conventional / Guerrilla / …).
 * That keeps the variant list as structured data rather than scraping it out of
 * the prose summary, and it is validated against the summary text by the unit
 * tests so the two never drift.
 */

import {
  FIELD_DOCS,
  canonicalizeSection,
  enclosingSection,
} from './field-docs.js';

/**
 * @typedef {object} Completion
 * @property {'key' | 'value'} kind   Whether this completes a key or an enum value.
 * @property {string}  label          The text inserted / shown (the key or value).
 * @property {string}  type           The field type (for keys) or 'enum value' (for values).
 * @property {string} [range]         Valid range, if known (keys only).
 * @property {string} [default]       Default value, if known (keys only).
 * @property {boolean} engineEffect   Engine-effect badge (keys reuse the doc flag;
 *                                     enum values inherit their field's flag).
 * @property {string}  summary        One-line meaning (keys only; '' for values).
 */

/**
 * @typedef {object} CompletionContext
 * @property {'key' | 'value' | 'none'} position  Where the caret is.
 * @property {string} section   Canonical dotted section family the caret is in.
 * @property {string} prefix    The partial token already typed (may be '').
 * @property {string} [key]     For a value position, the key being assigned.
 * @property {number} tokenStart Column offset where `prefix` begins on the line.
 */

/**
 * Canonicalize a stored `section` tag (e.g. "[map.regions]") into a dotted
 * family path comparable to {@link canonicalizeSection} output. Tags like
 * "(any)" / "(effect)" / "(top-level)" have no concrete section → ''.
 * @param {string} tag
 * @returns {string}
 */
function tagToCanon(tag) {
  if (!tag || tag.startsWith('(')) return '';
  return tag
    .replace(/^\[+/, '')
    .replace(/\]+$/, '')
    .trim()
    .split('.')
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .join('.');
}

/**
 * For a multi-variant catalog entry, pick the variant whose `section` tag best
 * matches the caret's canonical section. This intentionally re-implements the
 * lightweight scoring used in `field-docs.js` (which is not exported) so the
 * completion source resolves the same variant the hover tooltip would.
 *
 * @param {import('./field-docs.js').FieldDoc[]} variants
 * @param {string} sectionCanon
 * @returns {import('./field-docs.js').FieldDoc}
 */
function pickVariant(variants, sectionCanon) {
  let best = null;
  let bestScore = -Infinity;
  for (const v of variants) {
    const tagCanon = tagToCanon(v.section);
    let score;
    if (tagCanon === '') {
      score = 0; // generic fallback
    } else if (sectionCanon === '') {
      score = -1; // specific variant, no section context
    } else {
      const tagParts = tagCanon.split('.');
      const secParts = sectionCanon.split('.');
      score = tagParts.length;
      for (let i = 0; i < tagParts.length; i++) {
        if (tagParts[i] !== secParts[i]) {
          score = -1;
          break;
        }
      }
    }
    if (score > bestScore) {
      bestScore = score;
      best = v;
    }
  }
  return best || variants[0];
}

/**
 * Does a catalog variant apply in the caret's section? A variant applies when
 * its canonical tag is a prefix of (or equal to) the caret section, or when it
 * is a generic (no-section) variant. This is the "is this key valid here?"
 * filter for key completions.
 *
 * @param {import('./field-docs.js').FieldDoc} variant
 * @param {string} sectionCanon
 * @returns {boolean}
 */
function variantAppliesInSection(variant, sectionCanon) {
  const tagCanon = tagToCanon(variant.section);
  if (tagCanon === '') return true; // generic — valid anywhere
  if (sectionCanon === '') return false; // specific key, top-level caret
  const tagParts = tagCanon.split('.');
  const secParts = sectionCanon.split('.');
  if (tagParts.length > secParts.length) return false;
  for (let i = 0; i < tagParts.length; i++) {
    if (tagParts[i] !== secParts[i]) return false;
  }
  return true;
}

/**
 * Parse just enough TOML around the caret to drive completions: the enclosing
 * section, whether the caret is on a key vs a value, and the partial token.
 *
 * Rules:
 *   - On a comment line (`#…`) or section header (`[…]`) → position 'none'.
 *   - Left of the first `=` on the line (or on a line with no `=`) → 'key',
 *     and `prefix` is the identifier fragment ending at the caret.
 *   - Right of the `=` → 'value', `key` is the assignment's left-hand side, and
 *     `prefix` is the bare-word fragment ending at the caret (quotes stripped).
 *
 * @param {string} text    Full editor content.
 * @param {number} offset  Flat 0-based caret offset.
 * @returns {CompletionContext}
 */
export function completionContext(text, offset) {
  if (typeof text !== 'string') {
    return { position: 'none', section: '', prefix: '', tokenStart: 0 };
  }
  const clamped = Math.max(0, Math.min(offset, text.length));
  const lineStart = text.lastIndexOf('\n', clamped - 1) + 1;
  const lineEndRel = text.indexOf('\n', clamped);
  const lineEnd = lineEndRel === -1 ? text.length : lineEndRel;
  const line = text.slice(lineStart, lineEnd);
  const col = clamped - lineStart;
  const beforeCaret = line.slice(0, col);

  const trimmedStart = line.replace(/^\s*/, '');
  if (trimmedStart.startsWith('#') || trimmedStart.startsWith('[')) {
    return { position: 'none', section: '', prefix: '', tokenStart: col };
  }

  // Section context: walk back to the nearest header.
  const lineIdx = text.slice(0, lineStart).split('\n').length - 1;
  const lines = text.split('\n');
  const sectionCanon = canonicalizeSection(enclosingSection(lines, lineIdx));

  const eq = line.indexOf('=');
  const onValueSide = eq !== -1 && col > eq;

  if (onValueSide) {
    const key = line.slice(0, eq).trim().replace(/^["']|["']$/g, '');
    // The value fragment ending at the caret: a run of identifier chars,
    // optionally opened by a quote (which we strip from the prefix).
    const m = beforeCaret.match(/["']?([A-Za-z_][A-Za-z0-9_]*)?$/);
    const prefix = (m && m[1]) || '';
    const tokenStart = col - prefix.length;
    return { position: 'value', section: sectionCanon, prefix, key, tokenStart };
  }

  // Key side: the identifier fragment immediately left of the caret.
  const m = beforeCaret.match(/([A-Za-z_][A-Za-z0-9_]*)?$/);
  const prefix = (m && m[1]) || '';
  const tokenStart = col - prefix.length;
  return { position: 'key', section: sectionCanon, prefix, tokenStart };
}

/**
 * Build the list of key completions valid in a given canonical section,
 * filtered by an optional typed prefix. Reuses {@link FIELD_DOCS} as the
 * source — every catalog key whose (best) variant applies in this section is a
 * candidate.
 *
 * Ordering: case-insensitive prefix matches first (those that *start* with the
 * typed text), then substring matches, each group alphabetical. Engine-effect
 * keys are not boosted — relevance to the section is what matters.
 *
 * @param {string} sectionCanon  Canonical caret section (may be '').
 * @param {string} prefix        Partial key already typed (may be '').
 * @returns {Completion[]}
 */
export function keyCompletions(sectionCanon, prefix) {
  const pfx = (prefix || '').toLowerCase();
  const starts = [];
  const contains = [];
  for (const [key, entry] of Object.entries(FIELD_DOCS)) {
    const variants = Array.isArray(entry) ? entry : [entry];
    const applicable = variants.filter((v) => variantAppliesInSection(v, sectionCanon));
    if (applicable.length === 0) continue;
    const doc = pickVariant(applicable, sectionCanon);

    const lower = key.toLowerCase();
    let bucket;
    if (pfx === '') {
      bucket = starts;
    } else if (lower.startsWith(pfx)) {
      bucket = starts;
    } else if (lower.includes(pfx)) {
      bucket = contains;
    } else {
      continue;
    }
    bucket.push({
      kind: 'key',
      label: key,
      type: doc.type || '',
      range: doc.range,
      default: doc.default,
      engineEffect: !!doc.engineEffect,
      summary: doc.summary || '',
    });
  }
  const byLabel = (a, b) => a.label.localeCompare(b.label);
  starts.sort(byLabel);
  contains.sort(byLabel);
  return starts.concat(contains);
}

/**
 * Build enum-value completions for the field being assigned, when the catalog
 * entry for that key carries an `enumValues` list. Returns [] when the key is
 * unknown, has no enum variants, or none match the typed prefix.
 *
 * @param {string} key           The key on the left of `=`.
 * @param {string} sectionCanon  Canonical caret section (disambiguates variants).
 * @param {string} prefix        Partial value already typed (may be '').
 * @returns {Completion[]}
 */
export function valueCompletions(key, sectionCanon, prefix) {
  const entry = FIELD_DOCS[key];
  if (!entry) return [];
  const variants = Array.isArray(entry) ? entry : [entry];
  const doc = pickVariant(variants, sectionCanon);
  const values = doc.enumValues;
  if (!Array.isArray(values) || values.length === 0) return [];

  const pfx = (prefix || '').toLowerCase();
  return values
    .filter((v) => pfx === '' || v.toLowerCase().startsWith(pfx))
    .map((v) => ({
      kind: 'value',
      label: v,
      type: 'enum value',
      engineEffect: !!doc.engineEffect,
      summary: '',
    }));
}

/**
 * Top-level resolver the editor calls on each keystroke: parse the caret
 * context and return the appropriate completions (keys or enum values), plus
 * the context so the caller knows what range to replace on accept.
 *
 * @param {string} text    Full editor content.
 * @param {number} offset  Flat 0-based caret offset.
 * @returns {{ context: CompletionContext, items: Completion[] }}
 */
export function completionsAt(text, offset) {
  const context = completionContext(text, offset);
  let items = [];
  if (context.position === 'key') {
    items = keyCompletions(context.section, context.prefix);
  } else if (context.position === 'value') {
    items = valueCompletions(context.key, context.section, context.prefix);
  }
  return { context, items };
}
