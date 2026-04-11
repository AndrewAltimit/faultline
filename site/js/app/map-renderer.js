/**
 * Canvas 2D map renderer for Faultline simulation.
 * Renders a grid of regions colored by controlling faction, with force
 * unit icons and strength indicators.
 */

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
const FILL_ALPHA = 0.25;
const BORDER_WIDTH = 2;
const REGION_PADDING = 6;
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

    /** @type {object|null} */
    this.scenario = null;
    /** @type {object|null} */
    this.snapshot = null;

    /** @type {Map<string, {x: number, y: number, w: number, h: number}>} */
    this.regionLayout = new Map();

    /** @type {string|null} */
    this.hoveredRegion = null;

    // Set up resize observer.
    this._resizeObserver = new ResizeObserver(() => this._resize());
    this._resizeObserver.observe(canvas.parentElement);

    // Mouse events.
    canvas.addEventListener('mousemove', (e) => this._onMouseMove(e));
    canvas.addEventListener('mouseleave', () => this._onMouseLeave());
    canvas.addEventListener('click', (e) => this._onClick(e));

    this._resize();
  }

  /**
   * Set the scenario and compute grid layout.
   * @param {object} scenario
   */
  setScenario(scenario) {
    this.scenario = scenario;
    this._computeLayout();
    this.render(null);
  }

  /**
   * Render the map at the given snapshot state.
   * @param {object|null} snapshot
   */
  render(snapshot) {
    this.snapshot = snapshot;
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;

    ctx.clearRect(0, 0, w, h);

    if (!this.scenario) {
      this._drawEmptyState(ctx, w, h);
      return;
    }

    // Draw region cells.
    const regions = this.scenario.map.regions;
    const factions = this.scenario.factions;

    // Build region control map from snapshot or initial_control.
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

    // Draw connections between adjacent regions first (behind cells).
    this._drawConnections(ctx, regions);

    // Draw each region.
    for (const [rid, region] of Object.entries(regions)) {
      const layout = this.regionLayout.get(rid);
      if (!layout) continue;

      const controllingFaction = regionControl.get(rid);
      const factionColor = controllingFaction
        ? this._getFactionColor(factions, controllingFaction)
        : NEUTRAL_COLOR;

      // Region fill.
      ctx.save();
      this._roundRect(ctx, layout.x, layout.y, layout.w, layout.h, CORNER_RADIUS);

      // Fill with faction color at low opacity.
      ctx.fillStyle = this._hexToRgba(factionColor, FILL_ALPHA);
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

      // Region label.
      ctx.save();
      ctx.font = `500 ${LABEL_FONT_SIZE}px Inter, system-ui, sans-serif`;
      ctx.fillStyle = '#e4e4e7';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      ctx.fillText(
        region.name,
        layout.x + layout.w / 2,
        layout.y + 10,
        layout.w - 16
      );
      ctx.restore();

      // Population / strategic value sub-label.
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

    // Draw force units on top of regions.
    this._drawForces(ctx, factions, snapshot);
  }

  // -------------------------------------------------------------------
  // Layout
  // -------------------------------------------------------------------

  _computeLayout() {
    if (!this.scenario) return;

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
      // Estimate grid from region count.
      const n = regionIds.length;
      gridW = Math.ceil(Math.sqrt(n));
      gridH = Math.ceil(n / gridW);
    }

    // Assign regions to grid cells.
    const assignments = this._assignRegionsToGrid(regionIds, regions, gridW, gridH);

    const padding = 24;
    const gap = 8;
    const availW = this.canvas.width - padding * 2;
    const availH = this.canvas.height - padding * 2;
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

  /**
   * Assign region IDs to grid positions using directional name heuristics.
   */
  _assignRegionsToGrid(regionIds, regions, gridW, gridH) {
    const assignments = [];
    const assigned = new Set();

    // Try directional parsing first.
    for (const rid of regionIds) {
      const name = (regions[rid]?.name || rid).toLowerCase();
      const id = rid.toLowerCase();

      let row = -1, col = -1;

      // Vertical hints.
      if (id.includes('north') || name.includes('north')) row = 0;
      else if (id.includes('south') || name.includes('south')) row = gridH - 1;

      // Horizontal hints.
      if (id.includes('west') || name.includes('west')) col = 0;
      else if (id.includes('east') || name.includes('east')) col = gridW - 1;

      // Partial match for 2-row grids.
      if (row === -1 && gridH === 2) {
        if (id.includes('north') || name.includes('upper')) row = 0;
        else if (id.includes('south') || name.includes('lower') || name.includes('gulf')) row = 1;
      }

      if (row >= 0 && col >= 0) {
        assignments.push({ rid, col, row });
        assigned.add(rid);
      }
    }

    // Fill remaining regions in alphabetical order.
    const remaining = regionIds.filter((rid) => !assigned.has(rid)).sort();
    let idx = 0;
    for (let row = 0; row < gridH && remaining.length > 0; row++) {
      for (let col = 0; col < gridW && idx < remaining.length; col++) {
        // Skip cells already taken.
        const taken = assignments.some((a) => a.col === col && a.row === row);
        if (taken) continue;
        assignments.push({ rid: remaining[idx], col, row });
        idx++;
      }
    }

    // If we still have unassigned regions, append them.
    while (idx < remaining.length) {
      const row = Math.floor(assignments.length / gridW);
      const col = assignments.length % gridW;
      assignments.push({ rid: remaining[idx], col, row });
      idx++;
    }

    return assignments;
  }

  // -------------------------------------------------------------------
  // Drawing Helpers
  // -------------------------------------------------------------------

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

        const ax = layoutA.x + layoutA.w / 2;
        const ay = layoutA.y + layoutA.h / 2;
        const bx = layoutB.x + layoutB.w / 2;
        const by = layoutB.y + layoutB.h / 2;

        ctx.beginPath();
        ctx.moveTo(ax, ay);
        ctx.lineTo(bx, by);
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  _drawForces(ctx, factions, snapshot) {
    if (!factions) return;

    // Collect forces by region.
    /** @type {Map<string, Array<{name: string, type: string, strength: number, color: string}>>} */
    const forcesByRegion = new Map();

    for (const [fid, faction] of Object.entries(factions)) {
      const color = faction.color || NEUTRAL_COLOR;

      // Get strength from snapshot if available.
      const snapshotFaction = snapshot?.faction_states?.[fid];

      for (const [, unit] of Object.entries(faction.forces || {})) {
        const regionId = unit.region;
        if (!forcesByRegion.has(regionId)) {
          forcesByRegion.set(regionId, []);
        }

        // Try to find updated strength from snapshot (total_strength is per-faction,
        // individual unit strength is not in snapshot, so use scenario values).
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

    // Draw forces in each region.
    for (const [regionId, forces] of forcesByRegion) {
      const layout = this.regionLayout.get(regionId);
      if (!layout) continue;

      const startY = layout.y + 10 + LABEL_FONT_SIZE + 4 + 14 + 8;
      const unitHeight = UNIT_SIZE * 2 + 6;

      forces.forEach((force, i) => {
        const cx = layout.x + 24;
        const cy = startY + i * unitHeight;

        if (cy + UNIT_SIZE > layout.y + layout.h - 4) return;

        // Draw unit shape.
        this._drawUnitShape(ctx, cx, cy, UNIT_SIZE, force.type, force.color);

        // Label.
        ctx.save();
        ctx.font = `400 10px 'JetBrains Mono', monospace`;
        ctx.fillStyle = force.color;
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        const label = `${force.name} (${Math.round(force.strength)})`;
        ctx.fillText(label, cx + UNIT_SIZE + 6, cy, layout.w - 50);
        ctx.restore();
      });
    }
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
        const spikes = 5;
        const outerR = size / 2;
        const innerR = outerR * 0.4;
        ctx.beginPath();
        for (let i = 0; i < spikes * 2; i++) {
          const r = i % 2 === 0 ? outerR : innerR;
          const angle = (Math.PI / spikes) * i - Math.PI / 2;
          const px = cx + Math.cos(angle) * r;
          const py = cy + Math.sin(angle) * r;
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
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

  // -------------------------------------------------------------------
  // Interaction
  // -------------------------------------------------------------------

  _hitTest(x, y) {
    for (const [rid, layout] of this.regionLayout) {
      if (
        x >= layout.x && x <= layout.x + layout.w &&
        y >= layout.y && y <= layout.y + layout.h
      ) {
        return rid;
      }
    }
    return null;
  }

  _onMouseMove(e) {
    const rect = this.canvas.getBoundingClientRect();
    const scaleX = this.canvas.width / rect.width;
    const scaleY = this.canvas.height / rect.height;
    const x = (e.clientX - rect.left) * scaleX;
    const y = (e.clientY - rect.top) * scaleY;

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
    const scaleX = this.canvas.width / rect.width;
    const scaleY = this.canvas.height / rect.height;
    const x = (e.clientX - rect.left) * scaleX;
    const y = (e.clientY - rect.top) * scaleY;

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
    this.ctx.scale(dpr, dpr);

    // Reset scale for proper drawing.
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
