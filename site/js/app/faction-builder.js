/**
 * Visual faction builder — form-based alternative to editing raw TOML.
 * Generates TOML for the factions section and syncs with the editor.
 */
import { AppState } from './state.js';

const DOCTRINES = [
  'Conventional', 'Guerrilla', 'Defensive', 'Disruption',
  'CounterInsurgency', 'Blitzkrieg', 'Adaptive',
];

const FACTION_TYPES = [
  'Military', 'Government', 'Insurgent', 'Civilian',
  'PrivateMilitary', 'Foreign',
];

const UNIT_TYPES = [
  'Infantry', 'Armor', 'Artillery', 'Aerial', 'Naval',
  'Militia', 'SpecialOperations', 'CyberUnit', 'IntelligenceUnit',
];

export class FactionBuilder {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   */
  constructor(bus) {
    this.bus = bus;
    this.container = document.getElementById('faction-builder-container');

    /** @type {Array<object>} Local faction data for the builder forms */
    this.factions = [];

    /** @type {string|null} Currently selected faction ID (for map click) */
    this.selectedFactionId = null;

    /** @type {string|null} Currently selected force edit context */
    this.selectedForceContext = null;

    bus.on('scenario:loaded', (scenario) => this._onScenarioLoaded(scenario));
    bus.on('map:region-click', (regionId) => this._onRegionClick(regionId));
  }

  _onScenarioLoaded(scenario) {
    if (!scenario || !scenario.factions) return;

    this.factions = [];
    for (const [fid, faction] of Object.entries(scenario.factions)) {
      this.factions.push({
        id: fid,
        name: faction.name || fid,
        color: faction.color || '#7c5bf0',
        description: faction.description || '',
        doctrine: faction.doctrine || 'Conventional',
        factionType: this._extractFactionType(faction.faction_type),
        initialMorale: faction.initial_morale ?? 0.8,
        initialResources: faction.initial_resources ?? 100,
        resourceRate: faction.resource_rate ?? 5,
        logisticsCapacity: faction.logistics_capacity ?? 10,
        forces: Object.entries(faction.forces || {}).map(([uid, unit]) => ({
          id: uid,
          name: unit.name || uid,
          unitType: unit.unit_type || 'Infantry',
          region: unit.region || '',
          strength: unit.strength ?? 100,
          mobility: unit.mobility ?? 1.0,
          upkeep: unit.upkeep ?? 2.0,
        })),
      });
    }

    this._render();
  }

  _extractFactionType(ft) {
    if (!ft) return 'Insurgent';
    if (typeof ft === 'string') return ft;
    if (ft.kind) return ft.kind;
    // Check for object keys matching type names.
    for (const t of FACTION_TYPES) {
      if (ft[t] !== undefined) return t;
    }
    return 'Insurgent';
  }

  _render() {
    if (this.factions.length === 0) {
      this.container.innerHTML = `
        <div class="empty-state">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4-4v2"/>
            <circle cx="9" cy="7" r="4"/>
            <path d="M23 21v-2a4 4 0 00-3-3.87"/>
            <path d="M16 3.13a4 4 0 010 7.75"/>
          </svg>
          <span>Load a scenario to edit factions</span>
        </div>`;
      return;
    }

    let html = '';
    for (let i = 0; i < this.factions.length; i++) {
      const f = this.factions[i];
      html += this._renderFactionCard(f, i);
    }
    html += `<button class="btn-label" id="btn-add-faction" style="width: 100%; justify-content: center; margin-top: 4px;">+ Add Faction</button>`;

    this.container.innerHTML = html;

    // Bind events.
    this.container.querySelectorAll('.faction-input').forEach((input) => {
      input.addEventListener('change', () => this._onFormChange());
      input.addEventListener('input', () => {
        if (input.type === 'range' || input.type === 'number' || input.type === 'color') {
          this._onFormChange();
        }
      });
    });

    const addBtn = document.getElementById('btn-add-faction');
    if (addBtn) {
      addBtn.addEventListener('click', () => this._addFaction());
    }

    // Faction selection for map click.
    this.container.querySelectorAll('.faction-card').forEach((card) => {
      card.addEventListener('click', (e) => {
        // Don't interfere with input clicks.
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'SELECT') return;
        this.selectedFactionId = card.dataset.factionId;
        this._highlightSelected();
      });
    });
  }

  _renderFactionCard(f, idx) {
    const regionOptions = this._getRegionOptions();
    let forcesHtml = '';
    for (let j = 0; j < f.forces.length; j++) {
      const u = f.forces[j];
      forcesHtml += `
        <div class="faction-card" style="margin: 4px 0; padding: 8px; background: var(--bg-raised);">
          <div style="display: flex; gap: 6px; margin-bottom: 6px;">
            <input class="form-input faction-input" type="text" value="${this._esc(u.name)}"
                   data-faction="${idx}" data-force="${j}" data-field="name" placeholder="Unit name" style="flex: 1;">
            <select class="form-select faction-input" data-faction="${idx}" data-force="${j}" data-field="unitType" style="width: 120px;">
              ${UNIT_TYPES.map((t) => `<option value="${t}" ${t === u.unitType ? 'selected' : ''}>${t}</option>`).join('')}
            </select>
          </div>
          <div style="display: flex; gap: 6px;">
            <div style="flex: 1;">
              <label class="form-label">Region</label>
              <select class="form-select faction-input" data-faction="${idx}" data-force="${j}" data-field="region">
                ${regionOptions.map((r) => `<option value="${r.id}" ${r.id === u.region ? 'selected' : ''}>${r.name}</option>`).join('')}
              </select>
            </div>
            <div style="width: 70px;">
              <label class="form-label">Strength</label>
              <input class="form-input faction-input" type="number" value="${u.strength}"
                     data-faction="${idx}" data-force="${j}" data-field="strength" min="0" step="10">
            </div>
            <div style="width: 60px;">
              <label class="form-label">Mobility</label>
              <input class="form-input faction-input" type="number" value="${u.mobility}"
                     data-faction="${idx}" data-force="${j}" data-field="mobility" min="0" max="10" step="0.1">
            </div>
          </div>
        </div>`;
    }

    return `
      <div class="faction-card ${this.selectedFactionId === f.id ? 'selected' : ''}"
           data-faction-id="${this._esc(f.id)}" style="border-left: 3px solid ${this._safeColor(f.color)};">
        <div class="faction-card-header">
          <input type="color" class="faction-input" value="${this._safeColor(f.color)}"
                 data-faction="${idx}" data-field="color">
          <input class="form-input faction-input" type="text" value="${this._esc(f.name)}"
                 data-faction="${idx}" data-field="name" style="flex: 1; font-weight: 500;">
        </div>

        <div style="display: flex; gap: 6px; margin-bottom: 8px;">
          <div style="flex: 1;">
            <label class="form-label">Doctrine</label>
            <select class="form-select faction-input" data-faction="${idx}" data-field="doctrine">
              ${DOCTRINES.map((d) => `<option value="${d}" ${d === f.doctrine ? 'selected' : ''}>${d}</option>`).join('')}
            </select>
          </div>
          <div style="flex: 1;">
            <label class="form-label">Type</label>
            <select class="form-select faction-input" data-faction="${idx}" data-field="factionType">
              ${FACTION_TYPES.map((t) => `<option value="${t}" ${t === f.factionType ? 'selected' : ''}>${t}</option>`).join('')}
            </select>
          </div>
        </div>

        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-bottom: 8px;">
          <div>
            <label class="form-label">Morale</label>
            <input class="form-input faction-input" type="number" value="${f.initialMorale}"
                   data-faction="${idx}" data-field="initialMorale" min="0" max="1" step="0.05">
          </div>
          <div>
            <label class="form-label">Resources</label>
            <input class="form-input faction-input" type="number" value="${f.initialResources}"
                   data-faction="${idx}" data-field="initialResources" min="0" step="10">
          </div>
          <div>
            <label class="form-label">Resource Rate</label>
            <input class="form-input faction-input" type="number" value="${f.resourceRate}"
                   data-faction="${idx}" data-field="resourceRate" min="0" step="1">
          </div>
          <div>
            <label class="form-label">Logistics</label>
            <input class="form-input faction-input" type="number" value="${f.logisticsCapacity}"
                   data-faction="${idx}" data-field="logisticsCapacity" min="0" step="1">
          </div>
        </div>

        <div class="sidebar-title" style="margin-top: 8px;">Forces</div>
        ${forcesHtml}
        <button class="btn-label faction-add-force" data-faction="${idx}"
                style="width: 100%; justify-content: center; margin-top: 4px; font-size: 0.75rem;">
          + Add Force
        </button>
      </div>`;
  }

  _getRegionOptions() {
    if (!AppState.scenario || !AppState.scenario.map) return [];
    return Object.entries(AppState.scenario.map.regions).map(([rid, r]) => ({
      id: rid,
      name: r.name || rid,
    }));
  }

  _onFormChange() {
    // Read all form values back into this.factions.
    this.container.querySelectorAll('.faction-input').forEach((input) => {
      const fIdx = parseInt(input.dataset.faction, 10);
      const field = input.dataset.field;
      const forceIdx = input.dataset.force;

      if (isNaN(fIdx) || !field) return;

      let value = input.value;
      if (input.type === 'number') value = parseFloat(value) || 0;

      if (forceIdx !== undefined) {
        const jIdx = parseInt(forceIdx, 10);
        if (this.factions[fIdx]?.forces?.[jIdx]) {
          this.factions[fIdx].forces[jIdx][field] = value;
        }
      } else {
        if (this.factions[fIdx]) {
          this.factions[fIdx][field] = value;
        }
      }
    });

    this.bus.emit('builder:changed', this.factions);
  }

  _addFaction() {
    const id = `faction_${this.factions.length + 1}`;
    this.factions.push({
      id,
      name: `New Faction`,
      color: '#' + Math.floor(Math.random() * 0xffffff).toString(16).padStart(6, '0'),
      description: '',
      doctrine: 'Conventional',
      factionType: 'Military',
      initialMorale: 0.8,
      initialResources: 100,
      resourceRate: 5,
      logisticsCapacity: 10,
      forces: [],
    });
    this._render();
    this.bus.emit('builder:changed', this.factions);
  }

  _onRegionClick(regionId) {
    // If a faction is selected, set it as initial_control for the clicked region.
    if (this.selectedFactionId && AppState.scenario) {
      const region = AppState.scenario.map.regions[regionId];
      if (region) {
        region.initial_control = this.selectedFactionId;
        // Emit a region-control event with the actual change details,
        // not the unrelated factions array.
        this.bus.emit('map:region-control-changed', {
          regionId,
          factionId: this.selectedFactionId,
        });
      }
    }
  }

  _highlightSelected() {
    this.container.querySelectorAll('.faction-card').forEach((card) => {
      if (card.dataset.factionId === this.selectedFactionId) {
        card.style.outline = '2px solid var(--accent)';
        card.style.outlineOffset = '-2px';
      } else {
        card.style.outline = 'none';
      }
    });
  }

  _esc(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML.replace(/"/g, '&quot;');
  }

  /** Sanitize a color value for safe use in inline styles. */
  _safeColor(color) {
    if (typeof color === 'string' && /^#[0-9a-fA-F]{6}$/.test(color)) return color;
    return '#7c5bf0';
  }
}
