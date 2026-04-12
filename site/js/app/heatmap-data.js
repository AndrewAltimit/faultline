/**
 * Pure aggregation helpers for the Monte Carlo dashboard charts.
 *
 * These live in their own module so they can be unit-tested in Node
 * without standing up the full DOM. The dashboard imports the same
 * functions; do not duplicate the logic in dashboard.js.
 */

/**
 * Aggregate per-tick regional control across an array of MC runs.
 *
 * Each run is expected to expose a `snapshots` array; each snapshot
 * carries a `tick` number and a `region_control` map of
 * `{ regionId: factionId | null }`.
 *
 * Returns `{ ticks, regions, dominant }` where:
 *   - `ticks` is the sorted list of snapshot ticks observed
 *   - `regions` is the sorted list of region ids
 *   - `dominant[regionId][tickIdx] = { faction, prob }` is the
 *     plurality faction holding that region at that tick, and the
 *     share of runs (0..1) that held it
 *
 * Neutral cells (region_control entry is `null`) are bucketed under
 * the synthetic faction id `__neutral__` so the dashboard can render
 * them in a distinct color.
 *
 * Returns `null` if no run has any snapshots — in which case the
 * dashboard hides the heatmap entirely instead of drawing an empty
 * chart.
 *
 * @param {Array<{snapshots?: Array<{tick: number, region_control?: Object}>}>} runs
 * @returns {{ticks: number[], regions: string[], dominant: Object}|null}
 */
export function buildRegionalHeatmap(runs) {
  if (!Array.isArray(runs) || runs.length === 0) return null;
  const haveSnapshots = runs.some(
    (r) => Array.isArray(r.snapshots) && r.snapshots.length > 0,
  );
  if (!haveSnapshots) return null;

  const tickSet = new Set();
  const regionSet = new Set();
  for (const run of runs) {
    for (const snap of run.snapshots || []) {
      tickSet.add(snap.tick);
      if (snap.region_control) {
        for (const rid of Object.keys(snap.region_control)) regionSet.add(rid);
      }
    }
  }
  if (tickSet.size === 0 || regionSet.size === 0) return null;

  const ticks = Array.from(tickSet).sort((a, b) => a - b);
  const regions = Array.from(regionSet).sort();
  const tickIndex = new Map(ticks.map((t, i) => [t, i]));

  // counts[regionId][tickIdx][factionId] = number of runs
  const counts = {};
  for (const rid of regions) {
    counts[rid] = ticks.map(() => ({}));
  }

  for (const run of runs) {
    for (const snap of run.snapshots || []) {
      const ti = tickIndex.get(snap.tick);
      if (ti === undefined) continue;
      for (const [rid, faction] of Object.entries(snap.region_control || {})) {
        if (!counts[rid]) continue;
        const fid = faction == null ? '__neutral__' : faction;
        counts[rid][ti][fid] = (counts[rid][ti][fid] || 0) + 1;
      }
    }
  }

  const totalRuns = runs.length;
  const dominant = {};
  for (const rid of regions) {
    dominant[rid] = counts[rid].map((tickCounts) => {
      let bestFaction = null;
      let bestCount = 0;
      for (const [fid, n] of Object.entries(tickCounts)) {
        if (n > bestCount) {
          bestCount = n;
          bestFaction = fid;
        }
      }
      return { faction: bestFaction, prob: bestCount / totalRuns };
    });
  }

  return { ticks, regions, dominant };
}

/**
 * Build per-faction win-rate ranges from a SensitivityResult.
 *
 * For each faction observed in any sweep step's `win_rates`, compute
 * `{ min, max, range }` across the sweep. Returns the array sorted by
 * descending range so the most sensitive factions come first — that's
 * the conventional layout for tornado charts (widest swings on top).
 *
 * @param {{outcomes: Array<{win_rates?: Object}>}} sensResult
 * @returns {Array<{fid: string, min: number, max: number, range: number}>}
 */
export function buildTornadoRanges(sensResult) {
  const factions = new Set();
  for (const summary of sensResult.outcomes || []) {
    for (const fid of Object.keys(summary.win_rates || {})) factions.add(fid);
  }
  const ranges = [];
  for (const fid of factions) {
    let min = Infinity;
    let max = -Infinity;
    for (const summary of sensResult.outcomes || []) {
      const r = summary.win_rates?.[fid] ?? 0;
      if (r < min) min = r;
      if (r > max) max = r;
    }
    if (min === Infinity) continue;
    ranges.push({ fid, min, max, range: max - min });
  }
  ranges.sort((a, b) => b.range - a.range);
  return ranges;
}
