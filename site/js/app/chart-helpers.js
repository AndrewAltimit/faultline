/**
 * Pure Canvas-2D charting helpers: summary statistics, kernel-density
 * estimation, "nice" axis tick generation, and small reusable drawing
 * primitives (gridlines, axes, tick labels).
 *
 * The math here is dependency-free and DOM-free so it can be unit-tested
 * under Node. The drawing primitives take a `CanvasRenderingContext2D`
 * and so only run in the browser, but they hold no state.
 */
import { NEUTRAL } from './palette.js';

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/**
 * Linear-interpolated percentile of an UNSORTED numeric array.
 * `p` is in [0, 100]. Returns NaN for an empty array.
 *
 * @param {number[]} values
 * @param {number} p
 * @returns {number}
 */
export function percentile(values, p) {
  const xs = values.filter((v) => Number.isFinite(v)).sort((a, b) => a - b);
  return percentileSorted(xs, p);
}

/**
 * Percentile of an array that is ALREADY filtered + ascending-sorted. Lets
 * callers that need several percentiles of the same data sort once.
 *
 * @param {number[]} sorted
 * @param {number} p
 * @returns {number}
 */
export function percentileSorted(sorted, p) {
  if (sorted.length === 0) return NaN;
  if (sorted.length === 1) return sorted[0];
  const rank = (clamp(p, 0, 100) / 100) * (sorted.length - 1);
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  if (lo === hi) return sorted[lo];
  const frac = rank - lo;
  return sorted[lo] + (sorted[hi] - sorted[lo]) * frac;
}

/**
 * Common summary stats used by the distribution charts. Computed once so
 * the chart code doesn't re-sort the data per annotation.
 *
 * @param {number[]} values
 */
export function summaryStats(values) {
  const xs = values.filter((v) => Number.isFinite(v));
  if (xs.length === 0) {
    return { n: 0, min: NaN, max: NaN, mean: NaN, std: NaN, p5: NaN, p50: NaN, p95: NaN };
  }
  const n = xs.length;
  let sum = 0;
  let min = Infinity;
  let max = -Infinity;
  for (const v of xs) {
    sum += v;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  const mean = sum / n;
  let varSum = 0;
  for (const v of xs) varSum += (v - mean) * (v - mean);
  const std = n > 1 ? Math.sqrt(varSum / (n - 1)) : 0;
  // Sort once for all three percentiles instead of re-sorting per call.
  const sorted = xs.slice().sort((a, b) => a - b);
  return {
    n,
    min,
    max,
    mean,
    std,
    p5: percentileSorted(sorted, 5),
    p50: percentileSorted(sorted, 50),
    p95: percentileSorted(sorted, 95),
  };
}

/**
 * Silverman's rule-of-thumb bandwidth for a Gaussian kernel. Falls back
 * to a small positive value if the spread is degenerate so the KDE never
 * divides by zero.
 *
 * @param {number[]} values
 * @returns {number}
 */
export function silvermanBandwidth(values) {
  const xs = values.filter((v) => Number.isFinite(v));
  const n = xs.length;
  if (n < 2) return 1;
  const mean = xs.reduce((a, b) => a + b, 0) / n;
  let varSum = 0;
  for (const v of xs) varSum += (v - mean) * (v - mean);
  const std = Math.sqrt(varSum / (n - 1));
  // Sort once for both quartiles instead of re-sorting inside percentile() twice.
  const sorted = xs.slice().sort((a, b) => a - b);
  const iqr = percentileSorted(sorted, 75) - percentileSorted(sorted, 25);
  // Silverman: 0.9 * min(std, IQR/1.34) * n^(-1/5)
  let spread = std;
  if (iqr > 0) spread = Math.min(std, iqr / 1.34);
  if (!(spread > 0)) spread = std > 0 ? std : 1;
  const bw = 0.9 * spread * Math.pow(n, -1 / 5);
  return bw > 0 ? bw : 1;
}

/**
 * Gaussian kernel-density estimate sampled on a uniform grid over
 * `[min, max]` (optionally padded). Returns `{ xs, ys }` where `ys` is the
 * estimated density at each grid point (in per-x-unit units, integrates to
 * ~1 over the support). Returns `null` when there isn't enough data.
 *
 * @param {number[]} values
 * @param {{ samples?: number, bandwidth?: number, pad?: number }} [opts]
 * @returns {{ xs: number[], ys: number[], bandwidth: number }|null}
 */
export function kde(values, opts = {}) {
  const xs = values.filter((v) => Number.isFinite(v));
  if (xs.length < 2) return null;
  const samples = Math.max(2, opts.samples || 96);
  const bandwidth = opts.bandwidth && opts.bandwidth > 0 ? opts.bandwidth : silvermanBandwidth(xs);
  // Loop rather than spread: Math.min(...xs)/Math.max(...xs) blow the
  // call-stack argument limit (~125k) on long MC runs.
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of xs) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  if (lo === hi) {
    // Degenerate (all identical) — widen so we still draw a spike.
    lo -= 1;
    hi += 1;
  }
  const pad = opts.pad != null ? opts.pad : bandwidth * 3;
  lo -= pad;
  hi += pad;
  const gridXs = new Array(samples);
  const gridYs = new Array(samples);
  const invH = 1 / bandwidth;
  const norm = 1 / (xs.length * bandwidth * Math.sqrt(2 * Math.PI));
  for (let i = 0; i < samples; i++) {
    const x = lo + ((hi - lo) * i) / (samples - 1);
    let acc = 0;
    for (const v of xs) {
      const z = (x - v) * invH;
      acc += Math.exp(-0.5 * z * z);
    }
    gridXs[i] = x;
    gridYs[i] = acc * norm;
  }
  return { xs: gridXs, ys: gridYs, bandwidth };
}

// ---------------------------------------------------------------------------
// Axis tick generation
// ---------------------------------------------------------------------------

/**
 * Generate "nice" round tick values spanning `[min, max]` with roughly
 * `count` ticks, using the 1-2-5 step heuristic. Always returns at least
 * the two endpoints' nice bounds. Safe for degenerate ranges.
 *
 * @param {number} min
 * @param {number} max
 * @param {number} [count]
 * @returns {number[]}
 */
export function niceTicks(min, max, count = 5) {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [];
  if (min === max) return [min];
  const lo = Math.min(min, max);
  const hi = Math.max(min, max);
  const span = hi - lo;
  const rawStep = span / Math.max(1, count);
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  const norm = rawStep / mag;
  let step;
  if (norm < 1.5) step = 1;
  else if (norm < 3) step = 2;
  else if (norm < 7) step = 5;
  else step = 10;
  step *= mag;
  const start = Math.ceil(lo / step) * step;
  const ticks = [];
  // Guard against runaway loops from FP error.
  for (let v = start, i = 0; v <= hi + step * 1e-9 && i < 1000; v += step, i++) {
    // Snap near-zero FP noise.
    ticks.push(Math.abs(v) < step * 1e-9 ? 0 : v);
  }
  return ticks;
}

// ---------------------------------------------------------------------------
// Drawing primitives (browser-only; ctx is a CanvasRenderingContext2D)
// ---------------------------------------------------------------------------

/**
 * @typedef {{ top: number, right: number, bottom: number, left: number }} Plot
 *   inset describing the plotting rectangle inside a canvas of width `w`,
 *   height `h`. The plot area is [left, w-right] × [top, h-bottom].
 */

/** Vertical gridlines + x-axis tick labels at the given data values. */
export function drawXGrid(ctx, opts) {
  const { x0, x1, top, bottom, xScale, ticks, fmt, labelY } = opts;
  ctx.save();
  ctx.font = '400 9px "JetBrains Mono", monospace';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'top';
  ctx.lineWidth = 1;
  for (const t of ticks) {
    const px = Math.round(xScale(t)) + 0.5;
    if (px < x0 - 0.5 || px > x1 + 0.5) continue;
    ctx.strokeStyle = NEUTRAL.gridline;
    ctx.beginPath();
    ctx.moveTo(px, top);
    ctx.lineTo(px, bottom);
    ctx.stroke();
    ctx.fillStyle = NEUTRAL.tickLabel;
    ctx.fillText(fmt ? fmt(t) : String(t), px, labelY != null ? labelY : bottom + 4);
  }
  ctx.restore();
}

/** Horizontal gridlines + y-axis tick labels at the given data values. */
export function drawYGrid(ctx, opts) {
  const { left, right, top, bottom, yScale, ticks, fmt, labelX } = opts;
  ctx.save();
  ctx.font = '400 9px "JetBrains Mono", monospace';
  ctx.textAlign = 'right';
  ctx.textBaseline = 'middle';
  ctx.lineWidth = 1;
  for (const t of ticks) {
    const py = Math.round(yScale(t)) + 0.5;
    // Skip ticks that snap outside the plot area (mirrors drawXGrid). The
    // guard is a no-op when top/bottom are omitted.
    if (top != null && py < top - 0.5) continue;
    if (bottom != null && py > bottom + 0.5) continue;
    ctx.strokeStyle = NEUTRAL.gridline;
    ctx.beginPath();
    ctx.moveTo(left, py);
    ctx.lineTo(right, py);
    ctx.stroke();
    ctx.fillStyle = NEUTRAL.tickLabel;
    ctx.fillText(fmt ? fmt(t) : String(t), labelX != null ? labelX : left - 6, py);
  }
  ctx.restore();
}

/** A single vertical reference line (e.g. median, baseline) with a label. */
export function drawVRule(ctx, opts) {
  const { px, top, bottom, color, dash, label, labelColor } = opts;
  ctx.save();
  ctx.strokeStyle = color || NEUTRAL.gridlineStrong;
  ctx.lineWidth = 1.25;
  if (dash) ctx.setLineDash(dash);
  const x = Math.round(px) + 0.5;
  ctx.beginPath();
  ctx.moveTo(x, top);
  ctx.lineTo(x, bottom);
  ctx.stroke();
  ctx.setLineDash([]);
  if (label) {
    ctx.font = '500 9px "JetBrains Mono", monospace';
    ctx.fillStyle = labelColor || color || NEUTRAL.text;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'bottom';
    ctx.fillText(label, x, top + 9);
  }
  ctx.restore();
}

function clamp(v, lo, hi) {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}
