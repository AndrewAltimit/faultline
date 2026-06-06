/**
 * Centralized, colorblind-safe chart palette + small Canvas-2D drawing
 * helpers shared across the dashboard charts.
 *
 * The qualitative ramp is the Okabe-Ito palette (Okabe & Ito 2008), the
 * de-facto standard for deuteranopia/protanopia/tritanopia safety: its
 * eight hues stay mutually distinguishable under all three common forms
 * of color-vision deficiency. We deliberately do NOT key chart series off
 * scenario-authored faction colors (which are arbitrary and frequently
 * collide under CVD); instead each faction is mapped to a stable slot in
 * this ramp by index, so a given chart is internally consistent and safe.
 *
 * Everything here is dependency-free and DOM-free where possible, so the
 * pure helpers can be unit-tested under Node without a canvas.
 */

/**
 * Okabe-Ito qualitative palette. Order chosen so the first few entries
 * (blue, orange, bluish-green) are maximally separable — most charts only
 * use 2-3 series.
 */
export const QUALITATIVE = [
  '#0072b2', // blue
  '#e69f00', // orange
  '#009e73', // bluish green
  '#cc79a7', // reddish purple
  '#56b4e9', // sky blue
  '#d55e00', // vermillion
  '#f0e442', // yellow
  '#999999', // grey
];

/**
 * Qualitative-ramp slot assignments for the Duration distribution chart's
 * series. Shared between the canvas renderer and the HTML legend so the two
 * can never drift: change a slot here and both update together.
 */
export const DURATION_SERIES = {
  band: 0, // p5–p95 credible band + histogram bars
  kde: 1, // KDE density overlay
  mean: 2, // mean reference line (dashed)
  median: 3, // median reference line (solid)
};

/**
 * Neutral / structural colors that match the dark dashboard theme. Kept
 * here so chart code never hard-codes hex literals.
 */
export const NEUTRAL = {
  neutralFaction: '#8a8a93', // "no faction holds this" cells
  axis: 'rgba(161, 161, 170, 0.55)', // axis lines (text-muted @ 55%)
  gridline: 'rgba(82, 82, 91, 0.28)', // subtle interior gridlines
  gridlineStrong: 'rgba(113, 113, 122, 0.45)', // emphasized gridline (e.g. zero)
  trackBg: 'rgba(39, 39, 42, 0.55)', // bar-track background
  tickLabel: '#8b8b94', // axis tick labels
  text: '#e4e4e7', // primary in-chart text
  textDim: '#a1a1aa', // secondary in-chart text
};

/**
 * Sequential single-hue ramp (light → dark blue), colorblind-safe, used
 * for density / intensity encodings (e.g. heatmap alpha fallback). Returns
 * an `#rrggbb` string for `t` in [0, 1].
 */
export function sequential(t) {
  const c = clamp01(t);
  // Interpolate in sRGB between a pale blue and the Okabe-Ito deep blue.
  const lo = [222, 235, 247]; // #deebf7
  const hi = [8, 48, 107]; // #08306b
  const r = Math.round(lo[0] + (hi[0] - lo[0]) * c);
  const g = Math.round(lo[1] + (hi[1] - lo[1]) * c);
  const b = Math.round(lo[2] + (hi[2] - lo[2]) * c);
  return rgbToHex(r, g, b);
}

/**
 * Map an arbitrary index to a stable qualitative color, cycling the ramp.
 * @param {number} i
 * @returns {string} `#rrggbb`
 */
export function qualitative(i) {
  const n = QUALITATIVE.length;
  const idx = ((Math.trunc(i) % n) + n) % n;
  return QUALITATIVE[idx];
}

/**
 * Build a stable faction-id → color map from an ordered list of faction
 * ids. The order should be deterministic (e.g. `Object.keys` on a
 * scenario's factions, which the WASM layer emits in declaration order)
 * so the same scenario always paints the same way.
 *
 * @param {string[]} factionIds
 * @returns {Map<string, string>}
 */
export function factionColorMap(factionIds) {
  const map = new Map();
  let i = 0;
  for (const fid of factionIds || []) {
    if (map.has(fid)) continue;
    map.set(fid, qualitative(i));
    i += 1;
  }
  return map;
}

/** Convert a `#rrggbb` color to `rgba(r,g,b,a)` for alpha blending. */
export function withAlpha(hex, alpha) {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) return hex;
  const r = parseInt(m[1], 16);
  const g = parseInt(m[2], 16);
  const b = parseInt(m[3], 16);
  const a = clamp01(alpha);
  return `rgba(${r}, ${g}, ${b}, ${a.toFixed(3)})`;
}

function clamp01(v) {
  if (!Number.isFinite(v)) return 0;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

function rgbToHex(r, g, b) {
  const h = (n) => n.toString(16).padStart(2, '0');
  return `#${h(r)}${h(g)}${h(b)}`;
}
