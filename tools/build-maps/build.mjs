// Build simplified country polygons for faultline map-library.js.
//
// Usage (from repo root):
//   curl -L -o tools/build-maps/countries.geojson \
//     https://raw.githubusercontent.com/datasets/geo-countries/master/data/countries.geojson
//   node tools/build-maps/build.mjs
//
// Output: site/js/app/generated-regions.js
// Source dataset: datasets/geo-countries (CC0 / ODC-PDDL).

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const INPUT = path.join(__dirname, 'countries.geojson');
const OUTPUT = path.join(__dirname, '..', '..', 'site', 'js', 'app', 'generated-regions.js');

if (!fs.existsSync(INPUT)) {
  console.error(`Missing input: ${INPUT}

Fetch the source GeoJSON first (not committed to the repo due to size):

  curl -L -o tools/build-maps/countries.geojson \\
    https://raw.githubusercontent.com/datasets/geo-countries/master/data/countries.geojson
`);
  process.exit(1);
}

const gj = JSON.parse(fs.readFileSync(INPUT, 'utf8'));

// Ramer-Douglas-Peucker simplification (2D, lon/lat treated as Euclidean — fine
// for regional maps at this scale).
function rdp(points, epsilon) {
  if (points.length < 3) return points.slice();
  let maxD = 0, idx = 0;
  const end = points.length - 1;
  for (let i = 1; i < end; i++) {
    const d = perpDist(points[i], points[0], points[end]);
    if (d > maxD) { maxD = d; idx = i; }
  }
  if (maxD > epsilon) {
    const a = rdp(points.slice(0, idx + 1), epsilon);
    const b = rdp(points.slice(idx), epsilon);
    return a.slice(0, -1).concat(b);
  }
  return [points[0], points[end]];
}
function perpDist(p, a, b) {
  const [x, y] = p, [x1, y1] = a, [x2, y2] = b;
  const dx = x2 - x1, dy = y2 - y1;
  if (dx === 0 && dy === 0) return Math.hypot(x - x1, y - y1);
  const t = ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy);
  const cx = x1 + t * dx, cy = y1 + t * dy;
  return Math.hypot(x - cx, y - cy);
}

function polyArea(ring) {
  let a = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    a += (ring[j][0] + ring[i][0]) * (ring[j][1] - ring[i][1]);
  }
  return Math.abs(a) / 2;
}

function boundsOf(ring) {
  let minLon = Infinity, maxLon = -Infinity, minLat = Infinity, maxLat = -Infinity;
  for (const [lon, lat] of ring) {
    if (lon < minLon) minLon = lon;
    if (lon > maxLon) maxLon = lon;
    if (lat < minLat) minLat = lat;
    if (lat > maxLat) maxLat = lat;
  }
  return { minLon, maxLon, minLat, maxLat };
}

// Round a coord to N decimal places.
function round(arr, n = 2) {
  const f = Math.pow(10, n);
  return arr.map(([lon, lat]) => [Math.round(lon * f) / f, Math.round(lat * f) / f]);
}

// Get all outer rings for a feature.
function getOuterRings(feature) {
  const g = feature.geometry;
  if (!g) return [];
  if (g.type === 'Polygon') return [g.coordinates[0]];
  if (g.type === 'MultiPolygon') return g.coordinates.map((poly) => poly[0]);
  return [];
}

// Drop polygons below `minArea` (in lon/lat squared) — drops tiny islands.
// Then simplify + round.
function extractCountry(name, { epsilon = 0.25, minArea = 0.5, decimals = 2, maxPolys = 4 } = {}) {
  const f = gj.features.find((ft) => ft.properties.name === name);
  if (!f) {
    console.error('MISSING:', name);
    return [];
  }
  const rings = getOuterRings(f);
  const scored = rings
    .map((r) => ({ r, area: polyArea(r) }))
    .filter((x) => x.area >= minArea)
    .sort((a, b) => b.area - a.area)
    .slice(0, maxPolys);
  return scored.map(({ r }) => round(rdp(r, epsilon), decimals));
}

// Compute a label anchor from a polygon's centroid (area-weighted).
function centroid(polys) {
  let cx = 0, cy = 0, total = 0;
  for (const ring of polys) {
    const a = polyArea(ring);
    const b = boundsOf(ring);
    cx += a * (b.minLon + b.maxLon) / 2;
    cy += a * (b.minLat + b.maxLat) / 2;
    total += a;
  }
  if (total === 0) return [0, 0];
  return [Math.round(cx / total * 100) / 100, Math.round(cy / total * 100) / 100];
}

// --- Region definitions ------------------------------------------------

/**
 * Each region is { id, name, countries: [countryName], labelPos? }
 * `countries` can be merged (multiple countries' polygons under one region).
 * For Russia which has long longitude wrap near 180, we trim below.
 */

const EUROPE = [
  { id: 'british_isles', name: 'British Isles', countries: ['United Kingdom', 'Ireland'] },
  { id: 'nordic', name: 'Nordic', countries: ['Norway', 'Sweden', 'Finland', 'Denmark'] },
  { id: 'central_europe', name: 'Central Europe',
    countries: ['Germany', 'Netherlands', 'Belgium', 'Luxembourg', 'Austria', 'Switzerland', 'Czechia', 'Slovakia', 'Hungary'] },
  { id: 'poland_baltics', name: 'Poland & Baltics',
    countries: ['Poland', 'Lithuania', 'Latvia', 'Estonia'] },
  { id: 'france_iberia', name: 'France & Iberia',
    countries: ['France', 'Spain', 'Portugal', 'Andorra'] },
  { id: 'italy_balkans', name: 'Italy & Balkans',
    countries: ['Italy', 'Slovenia', 'Croatia', 'Bosnia and Herzegovina', 'Republic of Serbia', 'Montenegro', 'Albania', 'North Macedonia', 'Kosovo', 'Bulgaria', 'Romania', 'Greece'] },
  { id: 'ukraine', name: 'Ukraine',
    countries: ['Ukraine', 'Moldova', 'Belarus'] },
  { id: 'russia_west', name: 'Western Russia',
    countries: ['Russia'], clipLon: [20, 60] },
];

const EAST_ASIA = [
  { id: 'china_east', name: 'China (East)', countries: ['China'], clipLon: [105, 125], clipLat: [20, 42] },
  { id: 'china_north', name: 'China (North)', countries: ['China'], clipLon: [100, 130], clipLat: [37, 50] },
  { id: 'taiwan', name: 'Taiwan', countries: ['Taiwan'] },
  { id: 'japan', name: 'Japan', countries: ['Japan'] },
  { id: 'north_korea', name: 'North Korea', countries: ['North Korea'] },
  { id: 'south_korea', name: 'South Korea', countries: ['South Korea'] },
  { id: 'philippines', name: 'Philippines', countries: ['Philippines'] },
  { id: 'vietnam', name: 'Vietnam', countries: ['Vietnam'] },
];

const MIDDLE_EAST = [
  { id: 'israel_palestine', name: 'Israel / Palestine', countries: ['Israel', 'Palestine'] },
  { id: 'lebanon_syria', name: 'Lebanon & Syria', countries: ['Lebanon', 'Syria'] },
  { id: 'iraq', name: 'Iraq', countries: ['Iraq'] },
  { id: 'iran', name: 'Iran', countries: ['Iran'] },
  { id: 'saudi_arabia', name: 'Saudi Arabia', countries: ['Saudi Arabia'] },
  { id: 'gulf_states', name: 'Gulf States',
    countries: ['United Arab Emirates', 'Qatar', 'Kuwait', 'Bahrain', 'Oman'] },
  { id: 'turkey', name: 'Turkey', countries: ['Turkey'] },
  { id: 'yemen', name: 'Yemen', countries: ['Yemen'] },
  { id: 'jordan', name: 'Jordan', countries: ['Jordan'] },
  { id: 'egypt_sinai', name: 'Egypt', countries: ['Egypt'] },
];

// World map: major countries for global-scale scenarios.
const WORLD = [
  { id: 'usa', name: 'United States', countries: ['United States of America'], clipLon: [-125, -66], clipLat: [24, 50] },
  { id: 'canada', name: 'Canada', countries: ['Canada'], clipLon: [-141, -50], clipLat: [41, 72] },
  { id: 'mexico', name: 'Mexico', countries: ['Mexico'] },
  { id: 'central_america', name: 'Central America',
    countries: ['Guatemala', 'Belize', 'Honduras', 'El Salvador', 'Nicaragua', 'Costa Rica', 'Panama'] },
  { id: 'caribbean', name: 'Caribbean',
    countries: ['Cuba', 'Dominican Republic', 'Haiti', 'Jamaica', 'Puerto Rico'] },
  { id: 'brazil', name: 'Brazil', countries: ['Brazil'] },
  { id: 'argentina_cone', name: 'Southern Cone',
    countries: ['Argentina', 'Chile', 'Uruguay', 'Paraguay'] },
  { id: 'andes_north', name: 'Northern Andes',
    countries: ['Colombia', 'Venezuela', 'Ecuador', 'Peru', 'Bolivia', 'Guyana', 'Suriname'] },
  { id: 'uk', name: 'United Kingdom', countries: ['United Kingdom', 'Ireland'] },
  { id: 'france', name: 'France', countries: ['France'], clipLon: [-5, 10], clipLat: [41, 52] },
  { id: 'iberia', name: 'Iberia', countries: ['Spain', 'Portugal'] },
  { id: 'germany', name: 'Germany', countries: ['Germany'] },
  { id: 'italy', name: 'Italy', countries: ['Italy'] },
  { id: 'nordic', name: 'Nordic', countries: ['Norway', 'Sweden', 'Finland', 'Denmark', 'Iceland'] },
  { id: 'eastern_europe', name: 'Eastern Europe',
    countries: ['Poland', 'Czechia', 'Slovakia', 'Hungary', 'Romania', 'Bulgaria', 'Lithuania', 'Latvia', 'Estonia'] },
  { id: 'balkans', name: 'Balkans',
    countries: ['Slovenia', 'Croatia', 'Bosnia and Herzegovina', 'Republic of Serbia', 'Montenegro', 'Albania', 'North Macedonia', 'Kosovo', 'Greece'] },
  { id: 'ukraine', name: 'Ukraine', countries: ['Ukraine', 'Moldova', 'Belarus'] },
  { id: 'russia', name: 'Russia', countries: ['Russia'], clipLon: [20, 180] },
  { id: 'turkey', name: 'Turkey', countries: ['Turkey'] },
  { id: 'caucasus', name: 'Caucasus', countries: ['Georgia', 'Armenia', 'Azerbaijan'] },
  { id: 'iran', name: 'Iran', countries: ['Iran'] },
  { id: 'iraq', name: 'Iraq', countries: ['Iraq'] },
  { id: 'levant', name: 'Levant', countries: ['Syria', 'Lebanon', 'Jordan', 'Israel', 'Palestine'] },
  { id: 'arabia', name: 'Arabia',
    countries: ['Saudi Arabia', 'Yemen', 'Oman', 'United Arab Emirates', 'Qatar', 'Kuwait', 'Bahrain'] },
  { id: 'egypt', name: 'Egypt', countries: ['Egypt'] },
  { id: 'maghreb', name: 'Maghreb',
    countries: ['Morocco', 'Algeria', 'Tunisia', 'Libya', 'Western Sahara'] },
  { id: 'west_africa', name: 'West Africa',
    countries: ['Nigeria', 'Ghana', 'Ivory Coast', 'Senegal', 'Mali', 'Burkina Faso', 'Niger', 'Guinea', 'Mauritania', 'Sierra Leone', 'Liberia', 'Togo', 'Benin', 'Gambia', 'Guinea-Bissau'] },
  { id: 'central_africa', name: 'Central Africa',
    countries: ['Democratic Republic of the Congo', 'Republic of the Congo', 'Cameroon', 'Central African Republic', 'Gabon', 'Equatorial Guinea', 'Chad'] },
  { id: 'east_africa', name: 'East Africa',
    countries: ['Sudan', 'South Sudan', 'Ethiopia', 'Eritrea', 'Somalia', 'Kenya', 'Uganda', 'United Republic of Tanzania', 'Rwanda', 'Burundi', 'Djibouti'] },
  { id: 'southern_africa', name: 'Southern Africa',
    countries: ['South Africa', 'Namibia', 'Botswana', 'Zimbabwe', 'Mozambique', 'Angola', 'Zambia', 'Malawi', 'Lesotho', 'eSwatini', 'Madagascar'] },
  { id: 'central_asia', name: 'Central Asia',
    countries: ['Kazakhstan', 'Uzbekistan', 'Turkmenistan', 'Kyrgyzstan', 'Tajikistan', 'Afghanistan'] },
  { id: 'south_asia', name: 'South Asia',
    countries: ['India', 'Pakistan', 'Bangladesh', 'Nepal', 'Bhutan', 'Sri Lanka'] },
  { id: 'china', name: 'China', countries: ['China'] },
  { id: 'mongolia', name: 'Mongolia', countries: ['Mongolia'] },
  { id: 'korea_north', name: 'North Korea', countries: ['North Korea'] },
  { id: 'korea_south', name: 'South Korea', countries: ['South Korea'] },
  { id: 'japan', name: 'Japan', countries: ['Japan'] },
  { id: 'taiwan', name: 'Taiwan', countries: ['Taiwan'] },
  { id: 'sea_mainland', name: 'SE Asia (Mainland)',
    countries: ['Vietnam', 'Laos', 'Cambodia', 'Thailand', 'Myanmar'] },
  { id: 'sea_maritime', name: 'SE Asia (Maritime)',
    countries: ['Indonesia', 'Malaysia', 'Philippines', 'Brunei', 'East Timor', 'Singapore'] },
  { id: 'australia', name: 'Australia', countries: ['Australia'] },
  { id: 'new_zealand', name: 'New Zealand', countries: ['New Zealand'] },
];

// Sutherland–Hodgman polygon clip against a rectangle.
function clipRing(ring, clipLon, clipLat) {
  const [minLon, maxLon] = clipLon || [-Infinity, Infinity];
  const [minLat, maxLat] = clipLat || [-Infinity, Infinity];
  const edges = [
    { side: 'left', fn: (p) => p[0] >= minLon, inter: (a, b) => interp(a, b, 0, minLon) },
    { side: 'right', fn: (p) => p[0] <= maxLon, inter: (a, b) => interp(a, b, 0, maxLon) },
    { side: 'bottom', fn: (p) => p[1] >= minLat, inter: (a, b) => interp(a, b, 1, minLat) },
    { side: 'top', fn: (p) => p[1] <= maxLat, inter: (a, b) => interp(a, b, 1, maxLat) },
  ];
  let out = ring.slice();
  for (const edge of edges) {
    const inp = out;
    out = [];
    for (let i = 0; i < inp.length; i++) {
      const cur = inp[i];
      const prev = inp[(i - 1 + inp.length) % inp.length];
      const curIn = edge.fn(cur);
      const prevIn = edge.fn(prev);
      if (curIn) {
        if (!prevIn) out.push(edge.inter(prev, cur));
        out.push(cur);
      } else if (prevIn) {
        out.push(edge.inter(prev, cur));
      }
    }
    if (out.length === 0) return null;
  }
  // Drop degenerate rings (2-point output can survive when RDP + clipping
  // collapse a polygon to a sliver).
  if (out.length < 3) return null;
  return out;
}
function interp(a, b, axis, val) {
  const denom = b[axis] - a[axis];
  // Segment parallel to the clip plane: the intersection is undefined, so
  // return a point on the plane at one endpoint's other-axis coordinate.
  // Sutherland–Hodgman tolerates this because parallel segments either lie
  // entirely on one side of the plane (no intersection is ever requested)
  // or exactly on it (the endpoints themselves are the result).
  if (denom === 0) {
    const other = 1 - axis;
    return axis === 0 ? [val, a[other]] : [a[other], val];
  }
  const t = (val - a[axis]) / denom;
  const other = 1 - axis;
  const o = a[other] + t * (b[other] - a[other]);
  return axis === 0 ? [val, o] : [o, val];
}

function buildRegion(spec, opts) {
  const tagged = [];
  for (const name of spec.countries) {
    const cps = extractCountry(name, opts);
    for (const p of cps) {
      if (spec.clipLon || spec.clipLat) {
        const kept = clipRing(p, spec.clipLon, spec.clipLat);
        if (kept) tagged.push({ name, coords: kept });
      } else {
        tagged.push({ name, coords: p });
      }
    }
  }
  return {
    name: spec.name,
    labelPos: centroid(tagged.map((t) => t.coords)),
    states: tagged,
  };
}

function emit(mapName, specs, opts) {
  const obj = {};
  for (const s of specs) obj[s.id] = buildRegion(s, opts);
  return obj;
}

function stringify(obj) {
  // Compact JSON with coord arrays on single lines.
  let out = '{\n';
  const rids = Object.keys(obj);
  rids.forEach((rid, i) => {
    const r = obj[rid];
    out += `  ${rid}: {\n`;
    out += `    name: ${JSON.stringify(r.name)},\n`;
    out += `    labelPos: [${r.labelPos[0]}, ${r.labelPos[1]}],\n`;
    out += `    states: [\n`;
    r.states.forEach((st, j) => {
      const coords = st.coords.map(([a, b]) => `[${a},${b}]`).join(',');
      out += `      { name: ${JSON.stringify(st.name)}, coords: [${coords}] },\n`;
    });
    out += `    ],\n`;
    out += `  },\n`;
  });
  out += '}';
  return out;
}

const europe = emit('europe', EUROPE, { epsilon: 0.15, minArea: 1.0, maxPolys: 2 });
const eastAsia = emit('east_asia', EAST_ASIA, { epsilon: 0.15, minArea: 0.5, maxPolys: 4 });
const middleEast = emit('middle_east', MIDDLE_EAST, { epsilon: 0.1, minArea: 0.3, maxPolys: 2 });
const world = emit('world', WORLD, { epsilon: 0.4, minArea: 3.0, maxPolys: 2 });

let out = '';
out += '// AUTO-GENERATED by tools/build-maps.mjs — do not hand-edit the polygon bodies.\n';
out += '// Source: datasets/geo-countries (CC0 / ODC-PDDL) simplified via Ramer-Douglas-Peucker.\n\n';
out += 'export const EUROPE_REGIONS = ' + stringify(europe) + ';\n\n';
out += 'export const EAST_ASIA_REGIONS = ' + stringify(eastAsia) + ';\n\n';
out += 'export const MIDDLE_EAST_REGIONS = ' + stringify(middleEast) + ';\n\n';
out += 'export const WORLD_REGIONS = ' + stringify(world) + ';\n';

fs.writeFileSync(OUTPUT, out);
console.error('wrote', OUTPUT, out.length, 'bytes');
