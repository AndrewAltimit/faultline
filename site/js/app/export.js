/**
 * Result export: shape the in-memory Monte Carlo dashboard summary into a
 * portable JSON object and into flat CSV rows, plus tiny browser-side
 * download helpers (text blobs and chart-canvas PNGs).
 *
 * The data-shaping functions are pure and DOM-free so they can be unit
 * tested under Node. The download helpers touch `document` / `URL` /
 * `canvas`, so they only run in the browser; they're kept small and
 * separate from the shaping logic.
 *
 * Note: the `FinalTension` metric is intentionally omitted from the
 * exported metric distributions — the UI deliberately does not surface a
 * "tension" figure, and the export mirrors what the dashboard shows.
 */

/** Metric distribution keys we never export (see module note). */
const EXCLUDED_METRICS = new Set(['FinalTension']);

/** Distribution-stats fields we flatten into JSON/CSV, in display order. */
const DIST_FIELDS = [
  'mean',
  'median',
  'std_dev',
  'min',
  'max',
  'percentile_5',
  'percentile_95',
];

/** Feasibility columns flattened into CSV, in table order. */
const FEASIBILITY_FIELDS = [
  'technology_readiness',
  'operational_complexity',
  'detection_probability',
  'success_probability',
  'consequence_severity',
  'attribution_difficulty',
  'cost_asymmetry_ratio',
];

function isFiniteNum(v) {
  return typeof v === 'number' && Number.isFinite(v);
}

/** Round to 6 sig-ish figures for stable, compact output; pass through non-numbers. */
function tidy(v) {
  if (!isFiniteNum(v)) return v;
  // Avoid -0 and long floating tails.
  const r = Number(v.toFixed(6));
  return r === 0 ? 0 : r;
}

/**
 * Build a plain, JSON-serializable object from a `MonteCarloSummary`.
 *
 * Pulls out the pieces the dashboard renders: win rates, the metric
 * distributions (duration / casualties / cost / displacement / …, minus
 * tension), and the feasibility-matrix rows. Unknown / absent sections are
 * simply omitted. Includes a `total_runs` count and an optional
 * `scenario_name` for provenance.
 *
 * @param {object} summary  the MonteCarloSummary from the WASM run
 * @param {{ scenarioName?: string }} [opts]
 * @returns {object}
 */
export function summaryToExportObject(summary, opts = {}) {
  const out = {};
  if (opts.scenarioName) out.scenario_name = opts.scenarioName;
  if (summary && isFiniteNum(summary.total_runs)) out.total_runs = summary.total_runs;

  // Win rates (faction id -> probability).
  const winRates = summary?.win_rates || {};
  if (Object.keys(winRates).length) {
    out.win_rates = {};
    for (const [fid, rate] of Object.entries(winRates)) out.win_rates[fid] = tidy(rate);
  }

  // Metric distributions (duration, casualties, cost, …), tension excluded.
  const dists = summary?.metric_distributions || {};
  const metricOut = {};
  for (const [metric, stats] of Object.entries(dists)) {
    if (EXCLUDED_METRICS.has(metric) || !stats) continue;
    const row = {};
    for (const f of DIST_FIELDS) row[f] = tidy(stats[f]);
    metricOut[metric] = row;
  }
  if (Object.keys(metricOut).length) out.metric_distributions = metricOut;

  // Feasibility matrix rows.
  const rows = Array.isArray(summary?.feasibility_matrix) ? summary.feasibility_matrix : [];
  if (rows.length) {
    out.feasibility_matrix = rows.map((r) => {
      const o = { chain_id: r.chain_id, chain_name: r.chain_name };
      for (const f of FEASIBILITY_FIELDS) o[f] = tidy(r[f]);
      return o;
    });
  }

  return out;
}

/** Pretty-printed JSON string of {@link summaryToExportObject}. */
export function summaryToJSON(summary, opts = {}) {
  return JSON.stringify(summaryToExportObject(summary, opts), null, 2);
}

/**
 * Escape one CSV field per RFC 4180: wrap in quotes and double embedded
 * quotes when the value contains a comma, quote, or newline.
 */
export function csvEscape(value) {
  const s = value == null ? '' : String(value);
  if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
  return s;
}

/** Join a 2-D array of cells into an RFC-4180 CSV string (CRLF rows). */
export function rowsToCsv(rows) {
  return rows.map((cells) => cells.map(csvEscape).join(',')).join('\r\n');
}

/**
 * Flatten a summary into a single CSV with a leading `section` column so
 * the heterogeneous tables (win rates, metric distributions, feasibility)
 * coexist in one file. Returns the CSV text.
 *
 * Layout:
 *   section,key,metric,value
 *   win_rate,<faction>,,<prob>
 *   distribution,<metric>,mean,<v>            (one row per stat field)
 *   feasibility,<chain_name>,<axis>,<v>
 *
 * @param {object} summary
 * @param {{ scenarioName?: string }} [opts]
 * @returns {string}
 */
export function summaryToCsv(summary, opts = {}) {
  const rows = [['section', 'key', 'metric', 'value']];

  if (opts.scenarioName) rows.push(['meta', 'scenario_name', '', opts.scenarioName]);
  if (summary && isFiniteNum(summary.total_runs)) {
    rows.push(['meta', 'total_runs', '', summary.total_runs]);
  }

  const winRates = summary?.win_rates || {};
  for (const [fid, rate] of Object.entries(winRates)) {
    rows.push(['win_rate', fid, '', tidy(rate)]);
  }

  const dists = summary?.metric_distributions || {};
  for (const [metric, stats] of Object.entries(dists)) {
    if (EXCLUDED_METRICS.has(metric) || !stats) continue;
    for (const f of DIST_FIELDS) rows.push(['distribution', metric, f, tidy(stats[f])]);
  }

  const feas = Array.isArray(summary?.feasibility_matrix) ? summary.feasibility_matrix : [];
  for (const r of feas) {
    for (const f of FEASIBILITY_FIELDS) {
      rows.push(['feasibility', r.chain_name || r.chain_id, f, tidy(r[f])]);
    }
  }

  return rowsToCsv(rows);
}

/**
 * Build a filesystem-safe slug for a download filename from a scenario
 * name (or a default). Lowercased, non-alphanumerics collapsed to `-`.
 */
export function exportFilename(scenarioName, ext) {
  const base = (scenarioName || 'faultline-results')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60) || 'faultline-results';
  return `${base}.${ext}`;
}

// ---------------------------------------------------------------------------
// Browser-only download helpers
// ---------------------------------------------------------------------------

/**
 * Delay before revoking a download object URL. The anchor `click()` is
 * synchronous but the browser's download pipeline is not — revoking on the
 * next tick (`0ms`) can tear the URL down before Safari/Firefox have
 * initiated the file-system write, yielding 0-byte downloads. A generous
 * delay covers that window; the URL is also reclaimed on page unload.
 */
const REVOKE_DELAY_MS = 60_000;

/**
 * Trigger a download of `text` as a file named `filename` with the given
 * MIME type. Uses an object URL revoked after a delay (see
 * {@link REVOKE_DELAY_MS}). No-op outside a browser.
 */
export function downloadText(text, filename, mime = 'application/octet-stream') {
  if (typeof document === 'undefined') return;
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  triggerDownload(url, filename);
  setTimeout(() => URL.revokeObjectURL(url), REVOKE_DELAY_MS);
}

/**
 * Download a canvas as a PNG. Prefers `toBlob` (lower peak memory); falls
 * back to `toDataURL` when `toBlob` is unavailable. No-op for a missing
 * canvas or outside a browser.
 *
 * @param {HTMLCanvasElement} canvas
 * @param {string} filename
 */
export function downloadCanvasPng(canvas, filename) {
  if (typeof document === 'undefined' || !canvas || typeof canvas.toDataURL !== 'function') {
    return;
  }
  if (typeof canvas.toBlob === 'function') {
    canvas.toBlob((blob) => {
      if (!blob) return;
      const url = URL.createObjectURL(blob);
      triggerDownload(url, filename);
      setTimeout(() => URL.revokeObjectURL(url), REVOKE_DELAY_MS);
    }, 'image/png');
  } else {
    triggerDownload(canvas.toDataURL('image/png'), filename);
  }
}

function triggerDownload(href, filename) {
  const a = document.createElement('a');
  a.href = href;
  a.download = filename;
  a.rel = 'noopener';
  document.body.appendChild(a);
  a.click();
  a.remove();
}
