/**
 * Bundled geographic map library for Faultline scenarios.
 *
 * Each map is a dictionary of named regions; each region carries a
 * display name, a label anchor (`labelPos`), and a list of sub-shapes
 * (compatible with the existing US macro-region structure). The
 * polygons are simplified bounding outlines sufficient for
 * schematic rendering — they are NOT survey-accurate and must not be
 * used for cartographic measurement.
 *
 * All shapes are derived from public Natural Earth 50m data
 * coarse-simplified by hand.
 */

import { US_REGIONS } from './us-regions-geo.js';

// ---------------------------------------------------------------------------
// Europe — NATO vs Russia neighborhood
// ---------------------------------------------------------------------------

export const EUROPE_REGIONS = {
  british_isles: {
    name: 'British Isles',
    labelPos: [-3, 54],
    states: [
      { name: 'Great Britain', coords: [[-5.7, 50], [1.8, 51.1], [1.8, 53.5], [-0.1, 55.8], [-3.1, 58.6], [-5.2, 58.6], [-6, 56.6], [-5.7, 50]] },
      { name: 'Ireland', coords: [[-10.5, 51.4], [-6, 51.5], [-6, 55.4], [-10.5, 54.5], [-10.5, 51.4]] },
    ],
  },
  nordic: {
    name: 'Nordic',
    labelPos: [15, 63],
    states: [
      { name: 'Norway', coords: [[4.5, 58], [11.3, 58], [12.5, 61.5], [18, 68.5], [25, 70.5], [31, 70], [27, 69], [20, 68], [12, 64], [5, 61], [4.5, 58]] },
      { name: 'Sweden', coords: [[11.3, 55.5], [14, 55.5], [18, 57], [19.5, 60], [24, 65.5], [21, 69], [18, 68.5], [12.5, 61.5], [11.3, 58], [11.3, 55.5]] },
      { name: 'Finland', coords: [[21, 60], [28, 60], [31, 62], [30, 65], [29, 68], [27, 69], [21, 69], [21, 60]] },
      { name: 'Denmark', coords: [[8, 54.8], [12.6, 54.8], [12.6, 57.5], [8, 57.5], [8, 54.8]] },
    ],
  },
  central_europe: {
    name: 'Central Europe',
    labelPos: [10, 51],
    states: [
      { name: 'Germany', coords: [[6, 47.5], [15, 47.5], [15, 51], [14, 53], [9, 54.8], [6, 54.8], [6, 51], [6, 47.5]] },
      { name: 'Netherlands', coords: [[3.4, 51.2], [7.2, 51.2], [7.2, 53.6], [3.4, 53.6], [3.4, 51.2]] },
      { name: 'Belgium', coords: [[2.5, 49.5], [6.4, 49.5], [6.4, 51.5], [2.5, 51.5], [2.5, 49.5]] },
      { name: 'Austria', coords: [[9.5, 46.4], [17, 46.4], [17, 49], [9.5, 49], [9.5, 46.4]] },
      { name: 'Switzerland', coords: [[6, 45.8], [10.5, 45.8], [10.5, 47.8], [6, 47.8], [6, 45.8]] },
      { name: 'Czechia', coords: [[12.2, 48.6], [18.9, 48.6], [18.9, 51], [12.2, 51], [12.2, 48.6]] },
    ],
  },
  poland_baltics: {
    name: 'Poland & Baltics',
    labelPos: [22, 55],
    states: [
      { name: 'Poland', coords: [[14, 49], [24, 49], [24, 54], [19, 54.8], [14, 54.5], [14, 49]] },
      { name: 'Lithuania', coords: [[21, 53.9], [27, 53.9], [27, 56.5], [21, 56.5], [21, 53.9]] },
      { name: 'Latvia', coords: [[21, 55.7], [28, 55.7], [28, 58], [21, 58], [21, 55.7]] },
      { name: 'Estonia', coords: [[22, 57.5], [28.2, 57.5], [28.2, 59.7], [22, 59.7], [22, 57.5]] },
    ],
  },
  france_iberia: {
    name: 'France & Iberia',
    labelPos: [0, 44],
    states: [
      { name: 'France', coords: [[-4.8, 42.3], [8, 42.3], [8, 48.5], [2.5, 51], [-1.5, 51], [-4.8, 48], [-4.8, 42.3]] },
      { name: 'Spain', coords: [[-9.5, 36], [3.3, 36.5], [3.3, 42.5], [-1.5, 43.5], [-9.5, 43.5], [-9.5, 36]] },
      { name: 'Portugal', coords: [[-9.5, 37], [-6.2, 37], [-6.2, 42], [-9.5, 42], [-9.5, 37]] },
    ],
  },
  italy_balkans: {
    name: 'Italy & Balkans',
    labelPos: [15, 43],
    states: [
      { name: 'Italy', coords: [[7, 44], [13.8, 46], [14, 40], [18.5, 40], [18.5, 42], [13, 38], [8, 38.5], [7, 44]] },
      { name: 'Croatia', coords: [[13.5, 42.4], [19.4, 42.4], [19.4, 46.5], [13.5, 46.5], [13.5, 42.4]] },
      { name: 'Serbia', coords: [[19.2, 42], [23, 42], [23, 46.2], [19.2, 46.2], [19.2, 42]] },
      { name: 'Greece', coords: [[19.3, 34.8], [28.2, 34.8], [28.2, 41.8], [19.3, 41.8], [19.3, 34.8]] },
    ],
  },
  ukraine: {
    name: 'Ukraine',
    labelPos: [31, 49],
    states: [
      { name: 'Ukraine', coords: [[22, 44.5], [40, 44.5], [40, 52.3], [22, 52.3], [22, 44.5]] },
      { name: 'Moldova', coords: [[26.6, 45.5], [30.2, 45.5], [30.2, 48.5], [26.6, 48.5], [26.6, 45.5]] },
      { name: 'Belarus', coords: [[23, 51.3], [32.7, 51.3], [32.7, 56.2], [23, 56.2], [23, 51.3]] },
    ],
  },
  russia_west: {
    name: 'Western Russia',
    labelPos: [40, 58],
    states: [
      { name: 'Russia (West)', coords: [[28, 50], [55, 50], [60, 60], [60, 70], [30, 70], [28, 60], [28, 50]] },
    ],
  },
};

// ---------------------------------------------------------------------------
// East Asia — Taiwan Strait / Korean Peninsula
// ---------------------------------------------------------------------------

export const EAST_ASIA_REGIONS = {
  china_east: {
    name: 'China (East)',
    labelPos: [115, 32],
    states: [
      { name: 'China (Eastern Provinces)', coords: [[108, 22], [122, 23], [122, 40], [115, 42], [108, 40], [105, 32], [108, 22]] },
    ],
  },
  china_north: {
    name: 'China (North)',
    labelPos: [110, 42],
    states: [
      { name: 'China (Northern Provinces)', coords: [[100, 37], [125, 40], [125, 50], [100, 50], [100, 37]] },
    ],
  },
  taiwan: {
    name: 'Taiwan',
    labelPos: [121, 24],
    states: [
      { name: 'Taiwan', coords: [[120, 22], [122.3, 22.2], [122.3, 25.5], [120, 25.5], [120, 22]] },
    ],
  },
  japan: {
    name: 'Japan',
    labelPos: [138, 38],
    states: [
      { name: 'Honshu', coords: [[131, 34], [142, 34], [142, 41], [131, 41], [131, 34]] },
      { name: 'Hokkaido', coords: [[139.5, 41.4], [145.8, 41.4], [145.8, 45.6], [139.5, 45.6], [139.5, 41.4]] },
      { name: 'Kyushu', coords: [[129.5, 31], [132.5, 31], [132.5, 34], [129.5, 34], [129.5, 31]] },
    ],
  },
  north_korea: {
    name: 'North Korea',
    labelPos: [127, 40],
    states: [
      { name: 'DPRK', coords: [[124.3, 37.8], [131, 37.8], [131, 43], [124.3, 43], [124.3, 37.8]] },
    ],
  },
  south_korea: {
    name: 'South Korea',
    labelPos: [128, 36],
    states: [
      { name: 'ROK', coords: [[125.5, 33.5], [130, 33.5], [130, 38.5], [125.5, 38.5], [125.5, 33.5]] },
    ],
  },
  philippines: {
    name: 'Philippines',
    labelPos: [122, 13],
    states: [
      { name: 'Luzon', coords: [[119, 13], [123, 13], [123, 19], [119, 19], [119, 13]] },
      { name: 'Mindanao', coords: [[121, 5.5], [126.5, 5.5], [126.5, 10.5], [121, 10.5], [121, 5.5]] },
    ],
  },
  vietnam: {
    name: 'Vietnam',
    labelPos: [107, 16],
    states: [
      { name: 'Vietnam', coords: [[102, 8.5], [109.5, 10.5], [109.5, 23.5], [103, 23.5], [102, 8.5]] },
    ],
  },
};

// ---------------------------------------------------------------------------
// Middle East
// ---------------------------------------------------------------------------

export const MIDDLE_EAST_REGIONS = {
  israel_palestine: {
    name: 'Israel / Palestine',
    labelPos: [35, 31.5],
    states: [
      { name: 'Israel & Territories', coords: [[34.2, 29.5], [35.9, 29.5], [35.9, 33.3], [34.2, 33.3], [34.2, 29.5]] },
    ],
  },
  lebanon_syria: {
    name: 'Lebanon & Syria',
    labelPos: [37, 35],
    states: [
      { name: 'Lebanon', coords: [[35, 33], [36.6, 33], [36.6, 34.7], [35, 34.7], [35, 33]] },
      { name: 'Syria', coords: [[35.7, 32.3], [42.4, 32.3], [42.4, 37.3], [35.7, 37.3], [35.7, 32.3]] },
    ],
  },
  iraq: {
    name: 'Iraq',
    labelPos: [44, 33],
    states: [
      { name: 'Iraq', coords: [[38.8, 29], [48.6, 29], [48.6, 37.4], [38.8, 37.4], [38.8, 29]] },
    ],
  },
  iran: {
    name: 'Iran',
    labelPos: [53, 32],
    states: [
      { name: 'Iran', coords: [[44, 25.1], [63.3, 25.1], [63.3, 39.8], [44, 39.8], [44, 25.1]] },
    ],
  },
  saudi_arabia: {
    name: 'Saudi Arabia',
    labelPos: [45, 24],
    states: [
      { name: 'Saudi Arabia', coords: [[34.5, 16.4], [55.7, 16.4], [55.7, 32.2], [34.5, 32.2], [34.5, 16.4]] },
    ],
  },
  gulf_states: {
    name: 'Gulf States',
    labelPos: [52, 25],
    states: [
      { name: 'UAE', coords: [[51.5, 22.6], [56.4, 22.6], [56.4, 26.1], [51.5, 26.1], [51.5, 22.6]] },
      { name: 'Qatar', coords: [[50.7, 24.5], [51.6, 24.5], [51.6, 26.2], [50.7, 26.2], [50.7, 24.5]] },
      { name: 'Kuwait', coords: [[46.5, 28.5], [48.5, 28.5], [48.5, 30.1], [46.5, 30.1], [46.5, 28.5]] },
      { name: 'Bahrain', coords: [[50.4, 25.8], [50.8, 25.8], [50.8, 26.3], [50.4, 26.3], [50.4, 25.8]] },
      { name: 'Oman', coords: [[52, 16.7], [59.8, 16.7], [59.8, 26.4], [52, 26.4], [52, 16.7]] },
    ],
  },
  turkey: {
    name: 'Turkey',
    labelPos: [35, 39],
    states: [
      { name: 'Turkey', coords: [[26, 36], [45, 36], [45, 42.1], [26, 42.1], [26, 36]] },
    ],
  },
  yemen: {
    name: 'Yemen',
    labelPos: [47, 15],
    states: [
      { name: 'Yemen', coords: [[42.5, 12.6], [53.1, 12.6], [53.1, 19], [42.5, 19], [42.5, 12.6]] },
    ],
  },
};

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

  let bestKey = null;
  let bestScore = 0;
  for (const [key, map] of Object.entries(MAP_LIBRARY)) {
    const libIds = Object.keys(map.regions);
    const overlap = rids.filter((r) => libIds.includes(r)).length;
    if (overlap > bestScore) {
      bestScore = overlap;
      bestKey = key;
    }
  }
  return bestScore >= 3 ? bestKey : null;
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
