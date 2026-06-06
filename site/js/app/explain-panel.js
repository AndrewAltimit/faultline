/**
 * Pure render helpers for the editor's Explain button and inline
 * advisory-warnings panel (Epic P).
 *
 * These functions take the plain-object shapes the WASM exports return
 * (after `mapsToObjects` normalization) and produce HTML strings. They
 * are intentionally side-effect-free so they can be unit-tested
 * headlessly under `node --test` without a DOM or a WASM module.
 */

/**
 * Map a WarningKind enum tag (snake_case, as serialized by serde) to a
 * short human label. Kept in sync with `WarningKind::label` in
 * `crates/faultline-stats/src/warnings.rs`. Unknown kinds fall back to
 * the raw tag so a future Rust-side addition degrades gracefully instead
 * of rendering blank.
 */
export function warningKindLabel(kind) {
  switch (kind) {
    case 'faction_no_objective':
      return 'Faction has no objective';
    case 'unreferenced_region':
      return 'Unreferenced region';
    case 'unreachable_phase':
      return 'Unreachable kill-chain phase';
    default:
      return String(kind);
  }
}

/**
 * Minimal HTML-escape for text interpolated into innerHTML. Covers the
 * five characters that can break out of text / attribute context.
 * @param {string} s
 */
export function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Render the advisory-warnings report as an HTML fragment.
 *
 * @param {{warnings: Array<{kind: string, subject: string, message: string}>}} report
 * @returns {string} HTML string for the warnings panel body.
 */
export function renderWarnings(report) {
  const warnings = (report && report.warnings) || [];
  if (warnings.length === 0) {
    return `<div class="warnings-panel-title">No advisory warnings</div>
<div class="warning-item-message">The scenario loads cleanly and trips none of the modelling-smell checks (factions with no objective, unreferenced regions, unreachable kill-chain phases).</div>`;
  }

  const plural = warnings.length === 1 ? 'warning' : 'warnings';
  const items = warnings
    .map(
      (w) => `<div class="warning-item">
  <div class="warning-item-head">
    <span class="warning-item-kind">${escapeHtml(warningKindLabel(w.kind))}</span>
    <span class="warning-item-subject">${escapeHtml(w.subject)}</span>
  </div>
  <div class="warning-item-message">${escapeHtml(w.message)}</div>
</div>`,
    )
    .join('');

  return `<div class="warnings-panel-title">${warnings.length} advisory ${plural}</div>
${items}`;
}

/**
 * Whether the warnings report is clean (no findings). Used by the caller
 * to toggle the panel's "clean" styling class.
 * @param {{warnings: Array}} report
 */
export function warningsClean(report) {
  return !report || !report.warnings || report.warnings.length === 0;
}

/**
 * Render the explain Markdown into an HTML fragment.
 *
 * The Markdown is produced by the same `faultline_stats::explain`
 * renderer the CLI uses. We deliberately do NOT pull in a Markdown
 * parser dependency (the frontend is dependency-light vanilla JS) —
 * instead we present the Markdown verbatim in a <pre> block, which keeps
 * headings / tables / lists legible in a monospace panel without adding
 * a build step. The text is escaped so it can never inject markup.
 *
 * @param {string} markdown
 * @returns {string} HTML string for the explain panel body.
 */
export function renderExplain(markdown) {
  return `<pre>${escapeHtml(markdown || '')}</pre>`;
}
