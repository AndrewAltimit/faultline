/**
 * Pure normalization for the feasibility radar / spider chart.
 *
 * The dashboard's feasibility matrix is a wide table of one row per kill
 * chain with seven analytical axes. This module turns those rows into a
 * normalized [0, 1] vector per axis so they can be plotted on a shared
 * radar, while preserving the raw values for tooltips/labels.
 *
 * Six of the seven axes are already proportions in [0, 1]. The seventh,
 * cost-asymmetry ratio (defender $ / attacker $), is unbounded and
 * heavy-tailed, so it is normalized on a log scale relative to the other
 * rows in the same batch — the radar is a *comparative* view, so a
 * per-batch relative scale reads better than an absolute cap.
 *
 * DOM-free and dependency-free so it can be unit-tested under Node.
 */

/**
 * The seven radar axes, in clockwise plotting order. `key` matches the
 * `FeasibilityRow` field; `label` is the short axis caption; `direction`
 * documents which end is "high" (all are oriented so a larger normalized
 * value sits further from the center).
 */
export const FEASIBILITY_AXES = [
  { key: 'technology_readiness', label: 'Tech', direction: 'higher = readier' },
  { key: 'operational_complexity', label: 'Complexity', direction: 'higher = more complex' },
  { key: 'detection_probability', label: 'Detection', direction: 'higher = more detectable' },
  { key: 'success_probability', label: 'Success', direction: 'higher = more likely' },
  { key: 'consequence_severity', label: 'Severity', direction: 'higher = more severe' },
  { key: 'attribution_difficulty', label: 'Attribution', direction: 'higher = harder to attribute' },
  { key: 'cost_asymmetry', label: 'Cost asym', direction: 'higher = more favorable to attacker' },
];

function clamp01(v) {
  if (!Number.isFinite(v)) return 0;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

/**
 * Normalize a cost-asymmetry ratio against the batch's observed range, on
 * a log scale. Returns 0..1. When all rows share one ratio (or only one
 * row exists) every chain maps to the same midpoint (0.5) so the axis
 * neither dominates nor vanishes.
 *
 * @param {number} ratio        this row's defender/attacker cost ratio
 * @param {number} minLog       min log10(ratio) across the batch
 * @param {number} maxLog       max log10(ratio) across the batch
 */
export function normalizeCostAsymmetry(ratio, minLog, maxLog) {
  if (!Number.isFinite(ratio) || ratio <= 0) return 0;
  const span = maxLog - minLog;
  if (!(span > 0)) return 0.5;
  return clamp01((Math.log10(ratio) - minLog) / span);
}

/**
 * Build normalized radar data from the feasibility-matrix rows.
 *
 * @param {Array<object>} rows  `summary.feasibility_matrix`
 * @returns {{
 *   axes: typeof FEASIBILITY_AXES,
 *   series: Array<{
 *     chainId: string,
 *     chainName: string,
 *     values: number[],      // normalized 0..1, axis order
 *     raw: number[],         // raw values, axis order
 *   }>
 * }}
 */
export function buildFeasibilityAxes(rows) {
  const list = Array.isArray(rows) ? rows.filter(Boolean) : [];
  if (list.length === 0) return { axes: FEASIBILITY_AXES, series: [] };

  // Precompute the log range of the cost-asymmetry ratios across the batch.
  let minLog = Infinity;
  let maxLog = -Infinity;
  for (const r of list) {
    const ratio = r.cost_asymmetry_ratio;
    if (Number.isFinite(ratio) && ratio > 0) {
      const l = Math.log10(ratio);
      if (l < minLog) minLog = l;
      if (l > maxLog) maxLog = l;
    }
  }

  const series = list.map((r) => {
    const raw = [];
    const values = [];
    for (const axis of FEASIBILITY_AXES) {
      if (axis.key === 'cost_asymmetry') {
        const ratio = r.cost_asymmetry_ratio;
        raw.push(Number.isFinite(ratio) ? ratio : 0);
        values.push(normalizeCostAsymmetry(ratio, minLog, maxLog));
      } else {
        const v = r[axis.key];
        raw.push(Number.isFinite(v) ? v : 0);
        values.push(clamp01(v));
      }
    }
    return {
      chainId: r.chain_id,
      chainName: r.chain_name || r.chain_id,
      values,
      raw,
    };
  });

  return { axes: FEASIBILITY_AXES, series };
}
