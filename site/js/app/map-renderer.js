/**
 * Canvas 2D map renderer for Faultline simulation.
 *
 * Supports two rendering modes:
 *  - Geographic: real polygon outlines for known regions (e.g. US macro-regions)
 *  - Grid: fallback colored rectangles for abstract/tutorial scenarios
 */
import { US_REGIONS, US_OUTLINE, isUSScenario } from './us-regions-geo.js';

const UNIT_SHAPES = {
  Infantry: 'circle',
  Armor: 'square',
  Artillery: 'triangle',
  Aerial: 'diamond',
  Naval: 'pentagon',
  Militia: 'hexagon',
  SpecialOperations: 'star',
  CyberUnit: 'square',
  IntelligenceUnit: 'diamond',
};

const NEUTRAL_COLOR = '#27272a';
const HOVER_ALPHA = 0.15;
const FILL_ALPHA = 0.3;
const GEO_FILL_ALPHA = 0.4;
const BORDER_WIDTH = 2;
const CORNER_RADIUS = 8;
const LABEL_FONT_SIZE = 13;
const UNIT_SIZE = 10;

export class MapRenderer {
  /**
   * @param {HTMLCanvasElement} canvas
   * @param {import('./event-bus.js').EventBus} bus
   */
  constructor(canvas, bus) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.bus = bus;

    this.scenario = null;
    this.snapshot = null;
    this.hoveredRegion = null;

    /** @type {'geo'|'grid'} */
    this.mode = 'grid';

    // Grid mode layout: Map<rid, {x, y, w, h}>
    this.regionLayout = new Map();

    // Geo mode: projection params + precomputed screen polygons
    this._geoProjection = null;
    /** @type {Map<string, {screenPolygons: number[][][], labelPos: number[]}>} */
    this._geoRegions = new Map();

    this._resizeObserver = new ResizeObserver(() => this._resize());
    this._resizeObserver.observe(canvas.parentElement);

    canvas.addEventListener('mousemove', (e) => this._onMouseMove(e));
    canvas.addEventListener('mouseleave', () => this._onMouseLeave());
    canvas.addEventListener('click', (e) => this._onClick(e));

    this._resize();
  }

  setScenario(scenario) {
    this.scenario = scenario;

    // Detect rendering mode.
    if (scenario && isUSScenario(scenario.map.regions)) {
      this.mode = 'geo';
    } else {
      this.mode = 'grid';
    }

    this._computeLayout();
    this.render(null);
  }

  render(snapshot) {
    this.snapshot = snapshot;
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.width / dpr;
    const h = this.canvas.height / dpr;

    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

    if (!this.scenario) {
      this._drawEmptyState(ctx, w, h);
      return;
    }

    const regions = this.scenario.map.regions;
    const factions = this.scenario.factions;

    // Build region control map.
    const regionControl = new Map();
    if (snapshot && snapshot.region_control) {
      for (const [rid, fid] of Object.entries(snapshot.region_control)) {
        regionControl.set(rid, fid);
      }
    } else {
      for (const [rid, region] of Object.entries(regions)) {
        if (region.initial_control) {
          regionControl.set(rid, region.initial_control);
        }
      }
    }

    if (this.mode === 'geo') {
      this._renderGeo(ctx, regions, factions, regionControl);
    } else {
      this._renderGrid(ctx, regions, factions, regionControl);
    }

    this._drawForces(ctx, factions, snapshot);
  }

  // ===================================================================
  // Geographic rendering
  // ===================================================================

  _renderGeo(ctx, regions, factions, regionControl) {
    // Draw country outline.
    ctx.save();
    const outlineScreen = this._projectPolygon(US_OUTLINE);
    ctx.beginPath();
    for (let i = 0; i < outlineScreen.length; i++) {
      const [sx, sy] = outlineScreen[i];
      if (i === 0) ctx.moveTo(sx, sy);
      else ctx.lineTo(sx, sy);
    }
    ctx.closePath();
    ctx.strokeStyle = '#3f3f46';
    ctx.lineWidth = 1.5;
    ctx.stroke();
    ctx.restore();

    // Draw each region polygon.
    for (const [rid] of Object.entries(regions)) {
      const geoData = this._geoRegions.get(rid);
      if (!geoData) continue;

      const controllingFaction = regionControl.get(rid);
      const factionColor = controllingFaction
        ? this._getFactionColor(factions, controllingFaction)
        : NEUTRAL_COLOR;

      for (const screenPoly of geoData.screenPolygons) {
        ctx.save();

        // Draw polygon path.
        ctx.beginPath();
        for (let i = 0; i < screenPoly.length; i++) {
          const [sx, sy] = screenPoly[i];
          if (i === 0) ctx.moveTo(sx, sy);
          else ctx.lineTo(sx, sy);
        }
        ctx.closePath();

        // Fill.
        ctx.fillStyle = this._hexToRgba(factionColor, GEO_FILL_ALPHA);
        ctx.fill();

        // Hover highlight.
        if (this.hoveredRegion === rid) {
          ctx.fillStyle = `rgba(255, 255, 255, ${HOVER_ALPHA})`;
          ctx.fill();
        }

        // Border.
        ctx.strokeStyle = factionColor;
        ctx.lineWidth = BORDER_WIDTH;
        ctx.stroke();

        ctx.restore();
      }

      // Region label at label position.
      const region = regions[rid];
      const labelScreen = this._projectPoint(geoData.labelPos[0], geoData.labelPos[1]);

      ctx.save();
      ctx.font = `600 ${LABEL_FONT_SIZE}px Inter, system-ui, sans-serif`;
      ctx.fillStyle = '#ffffff';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.shadowColor = 'rgba(0,0,0,0.8)';
      ctx.shadowBlur = 4;
      ctx.fillText(region?.name || rid, labelScreen[0], labelScreen[1]);
      ctx.restore();

      // Population sub-label.
      if (region?.population) {
        ctx.save();
        ctx.font = `400 10px 'JetBrains Mono', monospace`;
        ctx.fillStyle = '#a1a1aa';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.shadowColor = 'rgba(0,0,0,0.8)';
        ctx.shadowBlur = 3;
        const popLabel = region.population >= 1_000_000
          ? `${(region.population / 1_000_000).toFixed(1)}M`
          : `${(region.population / 1_000).toFixed(0)}K`;
        ctx.fillText(popLabel, labelScreen[0], labelScreen[1] + 16);
        ctx.restore();
      }
    }
  }

  _computeGeoProjection() {
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.width / dpr;
    const h = this.canvas.height / dpr;
    const padding = 30;

    // Bounding box of the US outline.
    let minLon = Infinity, maxLon = -Infinity;
    let minLat = Infinity, maxLat = -Infinity;
    for (const [lon, lat] of US_OUTLINE) {
      if (lon < minLon) minLon = lon;
      if (lon > maxLon) maxLon = lon;
      if (lat < minLat) minLat = lat;
      if (lat > maxLat) maxLat = lat;
    }

    const geoW = maxLon - minLon;
    const geoH = maxLat - minLat;

    const availW = w - padding * 2;
    const availH = h - padding * 2;

    // Scale to fit, maintaining aspect ratio (lon/lat ~= 1.3 at US latitudes).
    const latCorrectionFactor = 1.3;
    const scaleX = availW / geoW;
    const scaleY = availH / (geoH * latCorrectionFactor);
    const scale = Math.min(scaleX, scaleY);

    // Center offset.
    const projectedW = geoW * scale;
    const projectedH = geoH * latCorrectionFactor * scale;
    const offsetX = padding + (availW - projectedW) / 2;
    const offsetY = padding + (availH - projectedH) / 2;

    this._geoProjection = { minLon, maxLat, scale, offsetX, offsetY, latCorrectionFactor };
  }

  _projectPoint(lon, lat) {
    const p = this._geoProjection;
    const x = p.offsetX + (lon - p.minLon) * p.scale;
    const y = p.offsetY + (p.maxLat - lat) * p.latCorrectionFactor * p.scale;
    return [x, y];
  }

  _projectPolygon(coords) {
    return coords.map(([lon, lat]) => this._projectPoint(lon, lat));
  }

  _computeGeoRegions() {
    this._geoRegions.clear();
    if (!this.scenario) return;

    for (const rid of Object.keys(this.scenario.map.regions)) {
      const geo = US_REGIONS[rid];
      if (!geo) continue;

      const screenPolygons = geo.polygons.map((poly) => this._projectPolygon(poly));
      this._geoRegions.set(rid, {
        screenPolygons,
        labelPos: geo.labelPos,
      });
    }

    // Also populate regionLayout for force icon positioning.
    this.regionLayout.clear();
    for (const [rid, geoData] of this._geoRegions) {
      // Compute bounding box of screen polygons.
      let minX = Infinity, maxX = -Infinity;
      let minY = Infinity, maxY = -Infinity;
      for (const poly of geoData.screenPolygons) {
        for (const [sx, sy] of poly) {
          if (sx < minX) minX = sx;
          if (sx > maxX) maxX = sx;
          if (sy < minY) minY = sy;
          if (sy > maxY) maxY = sy;
        }
      }
      this.regionLayout.set(rid, {
        x: minX, y: minY,
        w: maxX - minX, h: maxY - minY,
      });
    }
  }

  // ===================================================================
  // Grid rendering (fallback)
  // ===================================================================

  _renderGrid(ctx, regions, factions, regionControl) {
    this._drawConnections(ctx, regions);

    for (const [rid, region] of Object.entries(regions)) {
      const layout = this.regionLayout.get(rid);
      if (!layout) continue;

      const controllingFaction = regionControl.get(rid);
      const factionColor = controllingFaction
        ? this._getFactionColor(factions, controllingFaction)
        : NEUTRAL_COLOR;

      ctx.save();
      this._roundRect(ctx, layout.x, layout.y, layout.w, layout.h, CORNER_RADIUS);
      ctx.fillStyle = this._hexToRgba(factionColor, FILL_ALPHA);
      ctx.fill();

      if (this.hoveredRegion === rid) {
        ctx.fillStyle = `rgba(255, 255, 255, ${HOVER_ALPHA})`;
        ctx.fill();
      }

      ctx.strokeStyle = factionColor;
      ctx.lineWidth = BORDER_WIDTH;
      ctx.stroke();
      ctx.restore();

      ctx.save();
      ctx.font = `500 ${LABEL_FONT_SIZE}px Inter, system-ui, sans-serif`;
      ctx.fillStyle = '#e4e4e7';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      ctx.fillText(region.name, layout.x + layout.w / 2, layout.y + 10, layout.w - 16);
      ctx.restore();

      ctx.save();
      ctx.font = `400 10px 'JetBrains Mono', monospace`;
      ctx.fillStyle = '#71717a';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      const popLabel = region.population >= 1_000_000
        ? `${(region.population / 1_000_000).toFixed(1)}M`
        : `${(region.population / 1_000).toFixed(0)}K`;
      ctx.fillText(
        `pop: ${popLabel}`,
        layout.x + layout.w / 2,
        layout.y + 10 + LABEL_FONT_SIZE + 4,
        layout.w - 16
      );
      ctx.restore();
    }
  }

  // ===================================================================
  // Layout computation
  // ===================================================================

  _computeLayout() {
    if (!this.scenario) return;

    if (this.mode === 'geo') {
      this._computeGeoProjection();
      this._computeGeoRegions();
    } else {
      this._computeGridLayout();
    }
  }

  _computeGridLayout() {
    const regions = this.scenario.map.regions;
    const regionIds = Object.keys(regions);
    const source = this.scenario.map.source;

    let gridW = 1, gridH = 1;
    if (source.Grid) {
      gridW = source.Grid.width || 1;
      gridH = source.Grid.height || 1;
    } else if (source.width) {
      gridW = source.width;
      gridH = source.height || 1;
    } else {
      const n = regionIds.length;
      gridW = Math.ceil(Math.sqrt(n));
      gridH = Math.ceil(n / gridW);
    }

    const assignments = this._assignRegionsToGrid(regionIds, regions, gridW, gridH);

    const dpr = window.devicePixelRatio || 1;
    const canvasW = this.canvas.width / dpr;
    const canvasH = this.canvas.height / dpr;
    const padding = 24;
    const gap = 8;
    const availW = canvasW - padding * 2;
    const availH = canvasH - padding * 2;
    const cellW = (availW - gap * (gridW - 1)) / gridW;
    const cellH = (availH - gap * (gridH - 1)) / gridH;

    this.regionLayout.clear();
    for (const { rid, col, row } of assignments) {
      this.regionLayout.set(rid, {
        x: padding + col * (cellW + gap),
        y: padding + row * (cellH + gap),
        w: cellW,
        h: cellH,
      });
    }
  }

  _assignRegionsToGrid(regionIds, regions, gridW, gridH) {
    const assignments = [];
    const assigned = new Set();

    for (const rid of regionIds) {
      const name = (regions[rid]?.name || rid).toLowerCase();
      const id = rid.toLowerCase();
      let row = -1, col = -1;

      if (id.includes('north') || name.includes('north')) row = 0;
      else if (id.includes('south') || name.includes('south')) row = gridH - 1;

      if (id.includes('west') || name.includes('west')) col = 0;
      else if (id.includes('east') || name.includes('east')) col = gridW - 1;

      if (row === -1 && gridH === 2) {
        if (id.includes('north') || name.includes('upper')) row = 0;
        else if (id.includes('south') || name.includes('lower') || name.includes('gulf')) row = 1;
      }

      if (row >= 0 && col >= 0) {
        assignments.push({ rid, col, row });
        assigned.add(rid);
      }
    }

    const remaining = regionIds.filter((rid) => !assigned.has(rid)).sort();
    let idx = 0;
    for (let row = 0; row < gridH && remaining.length > 0; row++) {
      for (let col = 0; col < gridW && idx < remaining.length; col++) {
        const taken = assignments.some((a) => a.col === col && a.row === row);
        if (taken) continue;
        assignments.push({ rid: remaining[idx], col, row });
        idx++;
      }
    }

    while (idx < remaining.length) {
      const row = Math.floor(assignments.length / gridW);
      const col = assignments.length % gridW;
      assignments.push({ rid: remaining[idx], col, row });
      idx++;
    }

    return assignments;
  }

  // ===================================================================
  // Force unit rendering (shared between modes)
  // ===================================================================

  _drawForces(ctx, factions, snapshot) {
    if (!factions) return;

    const forcesByRegion = new Map();

    for (const [fid, faction] of Object.entries(factions)) {
      const color = faction.color || NEUTRAL_COLOR;
      const snapshotFaction = snapshot?.faction_states?.[fid];

      for (const [, unit] of Object.entries(faction.forces || {})) {
        const regionId = unit.region;
        if (!forcesByRegion.has(regionId)) {
          forcesByRegion.set(regionId, []);
        }
        forcesByRegion.get(regionId).push({
          name: unit.name,
          type: unit.unit_type,
          strength: unit.strength,
          color,
          factionName: faction.name,
          morale: snapshotFaction?.morale,
        });
      }
    }

    for (const [regionId, forces] of forcesByRegion) {
      const layout = this.regionLayout.get(regionId);
      if (!layout) continue;

      // In geo mode, position forces near the label point.
      // In grid mode, position below the region label.
      let startX, startY;
      if (this.mode === 'geo') {
        const geoData = this._geoRegions.get(regionId);
        if (geoData) {
          const lp = this._projectPoint(geoData.labelPos[0], geoData.labelPos[1]);
          startX = lp[0] - 40;
          startY = lp[1] + 24;
        } else {
          startX = layout.x + 16;
          startY = layout.y + 40;
        }
      } else {
        startX = layout.x + 24;
        startY = layout.y + 10 + LABEL_FONT_SIZE + 4 + 14 + 8;
      }

      const unitHeight = UNIT_SIZE * 2 + 4;
      forces.forEach((force, i) => {
        const cx = startX;
        const cy = startY + i * unitHeight;

        // Clip to region bounds for grid mode.
        if (this.mode === 'grid' && cy + UNIT_SIZE > layout.y + layout.h - 4) return;

        this._drawUnitShape(ctx, cx, cy, UNIT_SIZE, force.type, force.color);

        ctx.save();
        ctx.font = `400 10px 'JetBrains Mono', monospace`;
        ctx.fillStyle = force.color;
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        if (this.mode === 'geo') {
          ctx.shadowColor = 'rgba(0,0,0,0.8)';
          ctx.shadowBlur = 3;
        }
        const label = `${force.name} (${Math.round(force.strength)})`;
        ctx.fillText(label, cx + UNIT_SIZE + 6, cy, 180);
        ctx.restore();
      });
    }
  }

  // ===================================================================
  // Drawing helpers
  // ===================================================================

  _drawConnections(ctx, regions) {
    ctx.save();
    ctx.strokeStyle = '#3f3f46';
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 4]);

    const drawn = new Set();
    for (const [rid, region] of Object.entries(regions)) {
      const layoutA = this.regionLayout.get(rid);
      if (!layoutA) continue;

      for (const neighborId of (region.borders || [])) {
        const key = [rid, neighborId].sort().join(':');
        if (drawn.has(key)) continue;
        drawn.add(key);

        const layoutB = this.regionLayout.get(neighborId);
        if (!layoutB) continue;

        ctx.beginPath();
        ctx.moveTo(layoutA.x + layoutA.w / 2, layoutA.y + layoutA.h / 2);
        ctx.lineTo(layoutB.x + layoutB.w / 2, layoutB.y + layoutB.h / 2);
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  _drawUnitShape(ctx, cx, cy, size, unitType, color) {
    const shape = UNIT_SHAPES[unitType] || 'circle';
    ctx.save();
    ctx.fillStyle = color;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;

    switch (shape) {
      case 'circle':
        ctx.beginPath();
        ctx.arc(cx, cy, size / 2, 0, Math.PI * 2);
        ctx.fill();
        break;
      case 'square':
        ctx.fillRect(cx - size / 2, cy - size / 2, size, size);
        break;
      case 'triangle':
        ctx.beginPath();
        ctx.moveTo(cx, cy - size / 2);
        ctx.lineTo(cx + size / 2, cy + size / 2);
        ctx.lineTo(cx - size / 2, cy + size / 2);
        ctx.closePath();
        ctx.fill();
        break;
      case 'diamond':
        ctx.beginPath();
        ctx.moveTo(cx, cy - size / 2);
        ctx.lineTo(cx + size / 2, cy);
        ctx.lineTo(cx, cy + size / 2);
        ctx.lineTo(cx - size / 2, cy);
        ctx.closePath();
        ctx.fill();
        break;
      case 'star': {
        const spikes = 5, outerR = size / 2, innerR = outerR * 0.4;
        ctx.beginPath();
        for (let i = 0; i < spikes * 2; i++) {
          const r = i % 2 === 0 ? outerR : innerR;
          const angle = (Math.PI / spikes) * i - Math.PI / 2;
          if (i === 0) ctx.moveTo(cx + Math.cos(angle) * r, cy + Math.sin(angle) * r);
          else ctx.lineTo(cx + Math.cos(angle) * r, cy + Math.sin(angle) * r);
        }
        ctx.closePath();
        ctx.fill();
        break;
      }
      case 'hexagon': {
        ctx.beginPath();
        for (let i = 0; i < 6; i++) {
          const angle = (Math.PI / 3) * i - Math.PI / 6;
          const px = cx + Math.cos(angle) * (size / 2);
          const py = cy + Math.sin(angle) * (size / 2);
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.closePath();
        ctx.fill();
        break;
      }
      case 'pentagon': {
        ctx.beginPath();
        for (let i = 0; i < 5; i++) {
          const angle = (Math.PI * 2 / 5) * i - Math.PI / 2;
          const px = cx + Math.cos(angle) * (size / 2);
          const py = cy + Math.sin(angle) * (size / 2);
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.closePath();
        ctx.fill();
        break;
      }
    }
    ctx.restore();
  }

  _drawEmptyState(ctx, w, h) {
    ctx.save();
    ctx.font = '500 16px Inter, system-ui, sans-serif';
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText('Load a scenario to view the map', w / 2, h / 2);
    ctx.restore();
  }

  _roundRect(ctx, x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + r);
    ctx.lineTo(x + w, y + h - r);
    ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    ctx.lineTo(x + r, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  }

  _hexToRgba(hex, alpha) {
    const r = parseInt(hex.slice(1, 3), 16) || 0;
    const g = parseInt(hex.slice(3, 5), 16) || 0;
    const b = parseInt(hex.slice(5, 7), 16) || 0;
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  _getFactionColor(factions, fid) {
    if (!factions) return NEUTRAL_COLOR;
    const faction = factions[fid];
    return faction?.color || NEUTRAL_COLOR;
  }

  // ===================================================================
  // Hit testing & interaction
  // ===================================================================

  _hitTest(x, y) {
    if (this.mode === 'geo') {
      return this._hitTestGeo(x, y);
    }
    return this._hitTestGrid(x, y);
  }

  _hitTestGrid(x, y) {
    for (const [rid, layout] of this.regionLayout) {
      if (x >= layout.x && x <= layout.x + layout.w &&
          y >= layout.y && y <= layout.y + layout.h) {
        return rid;
      }
    }
    return null;
  }

  _hitTestGeo(x, y) {
    for (const [rid, geoData] of this._geoRegions) {
      for (const poly of geoData.screenPolygons) {
        if (this._pointInPolygon(x, y, poly)) {
          return rid;
        }
      }
    }
    return null;
  }

  _pointInPolygon(x, y, polygon) {
    let inside = false;
    for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
      const xi = polygon[i][0], yi = polygon[i][1];
      const xj = polygon[j][0], yj = polygon[j][1];
      const intersect = ((yi > y) !== (yj > y)) &&
        (x < (xj - xi) * (y - yi) / (yj - yi) + xi);
      if (intersect) inside = !inside;
    }
    return inside;
  }

  _onMouseMove(e) {
    const rect = this.canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const rid = this._hitTest(x, y);
    if (rid !== this.hoveredRegion) {
      this.hoveredRegion = rid;
      this.canvas.style.cursor = rid ? 'pointer' : 'default';
      this.render(this.snapshot);
    }
  }

  _onMouseLeave() {
    if (this.hoveredRegion) {
      this.hoveredRegion = null;
      this.canvas.style.cursor = 'default';
      this.render(this.snapshot);
    }
  }

  _onClick(e) {
    const rect = this.canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const rid = this._hitTest(x, y);
    if (rid) {
      this.bus.emit('map:region-click', rid);
    }
  }

  _resize() {
    const parent = this.canvas.parentElement;
    if (!parent) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = parent.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    this.canvas.style.width = rect.width + 'px';
    this.canvas.style.height = rect.height + 'px';

    if (this.scenario) {
      this._computeLayout();
      this.render(this.snapshot);
    } else {
      this._drawEmptyState(this.ctx, rect.width, rect.height);
    }
  }
}
