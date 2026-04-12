/**
 * Bundled geographic map library for Faultline scenarios.
 *
 * Each map is a dictionary of named regions; each region carries a
 * display name, a label anchor (`labelPos`), and a list of sub-shapes
 * (compatible with the US macro-region structure).
 *
 * The polygons for Europe, East Asia, Middle East, and World are
 * regenerated from `datasets/geo-countries` (CC0 / ODC-PDDL) by
 * `tools/build-maps/build.mjs` and live in `generated-regions.js`. The
 * US macro-region polygons are hand-authored in `us-regions-geo.js`.
 *
 * All polygons are simplified via Ramer–Douglas–Peucker for schematic
 * rendering — they are NOT survey-accurate and must not be used for
 * cartographic measurement.
 */

import { US_REGIONS } from './us-regions-geo.js';
import {
  EUROPE_REGIONS,
  EAST_ASIA_REGIONS,
  MIDDLE_EAST_REGIONS,
  WORLD_REGIONS,
} from './generated-regions.js';

export { EUROPE_REGIONS, EAST_ASIA_REGIONS, MIDDLE_EAST_REGIONS, WORLD_REGIONS };

// ---------------------------------------------------------------------------
// World map library (keyed dictionary of maps)
// ---------------------------------------------------------------------------

/**
 * All bundled maps available in the app.
 * Each entry: { name, description, regions: Record<string, RegionGeo> }.
 */
export const MAP_LIBRARY = {
  us_states: {
    name: 'United States — Macro-Regions',
    description: '8 macro-regions covering the contiguous United States.',
    regions: US_REGIONS,
  },
  europe: {
    name: 'Europe — NATO & Eastern Flank',
    description: 'Western Europe, Nordics, Central Europe, Baltics, Ukraine, Western Russia.',
    regions: EUROPE_REGIONS,
  },
  east_asia: {
    name: 'East Asia — Taiwan Strait & Korean Peninsula',
    description: 'China, Taiwan, Japan, Koreas, Philippines, Vietnam.',
    regions: EAST_ASIA_REGIONS,
  },
  middle_east: {
    name: 'Middle East',
    description: 'Israel, Lebanon, Syria, Iraq, Iran, Saudi Arabia, Gulf States, Turkey, Yemen.',
    regions: MIDDLE_EAST_REGIONS,
  },
  world: {
    name: 'World — Global Macro-Regions',
    description: 'All inhabited continents grouped into 42 macro-regions for global-scale scenarios.',
    regions: WORLD_REGIONS,
  },
};

/**
 * Detect which bundled map (if any) matches a scenario's region IDs.
 *
 * Returns the map key (e.g. "us_states", "europe") or null for unknown /
 * custom / abstract-grid layouts.
 */
export function detectMap(scenarioRegions) {
  if (!scenarioRegions) return null;
  const rids = Object.keys(scenarioRegions);
  if (!rids.length) return null;

  // Score each bundled map by how many region IDs it covers. Prefer the map
  // with the highest *coverage ratio* (overlap / scenario size); this prevents
  // the large "world" map from swallowing a smaller-region Europe scenario on
  // tie cases while still letting it win for truly global scenarios.
  let bestKey = null;
  let bestScore = 0;
  let bestOverlap = 0;
  for (const [key, map] of Object.entries(MAP_LIBRARY)) {
    const libIds = Object.keys(map.regions);
    const overlap = rids.filter((r) => libIds.includes(r)).length;
    if (overlap < 3) continue;
    const ratio = overlap / rids.length;
    // Epsilon comparison to sidestep float equality: in practice `ratio`
    // is a rational with `rids.length` denominator so equal numerators
    // are bit-exact, but keep the guard to stay robust if this ever
    // changes to a weighted score.
    const EPS = 1e-9;
    if (ratio > bestScore + EPS || (Math.abs(ratio - bestScore) < EPS && overlap > bestOverlap)) {
      bestScore = ratio;
      bestOverlap = overlap;
      bestKey = key;
    }
  }
  return bestKey;
}

/**
 * Return the geographic data for a given scenario region ID against
 * the auto-detected map library. Falls back to null for custom maps.
 */
export function getRegionGeo(mapKey, regionId) {
  if (!mapKey || !MAP_LIBRARY[mapKey]) return null;
  return MAP_LIBRARY[mapKey].regions[regionId] || null;
}

/**
 * Return the list of all region entries for a bundled map.
 */
export function getMapRegions(mapKey) {
  if (!mapKey || !MAP_LIBRARY[mapKey]) return null;
  return MAP_LIBRARY[mapKey].regions;
}
