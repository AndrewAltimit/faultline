/**
 * TOML scenario editor with validation, preset loading, and file I/O.
 */
import { AppState } from './state.js';
import { PRESETS } from './presets.js';
import { mapsToObjects } from './wasm-util.js';

export class Editor {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   * @param {object} wasm - WASM module exports
   */
  constructor(bus, wasm) {
    this.bus = bus;
    this.wasm = wasm;

    // DOM elements.
    this.textarea = document.getElementById('toml-editor');
    this.presetSelect = document.getElementById('preset-select');
    this.btnValidate = document.getElementById('btn-validate');
    this.btnLoad = document.getElementById('btn-load');
    this.btnImport = document.getElementById('btn-import');
    this.btnExport = document.getElementById('btn-export');
    this.fileInput = document.getElementById('file-import');
    this.validationMsg = document.getElementById('validation-msg');

    // Tab switching.
    document.querySelectorAll('.app-tab').forEach((tab) => {
      tab.addEventListener('click', () => this._switchTab(tab));
    });

    // Populate preset dropdown.
    for (const preset of PRESETS) {
      const opt = document.createElement('option');
      opt.value = preset.path;
      opt.textContent = preset.name;
      this.presetSelect.appendChild(opt);
    }

    // Event listeners.
    this.presetSelect.addEventListener('change', () => this._loadPreset());
    this.btnValidate.addEventListener('click', () => this._validate());
    this.btnLoad.addEventListener('click', () => this._loadAndRun());
    this.btnImport.addEventListener('click', () => this.fileInput.click());
    this.btnExport.addEventListener('click', () => this._export());
    this.fileInput.addEventListener('change', (e) => this._import(e));
  }

  /**
   * Set the editor text content.
   * @param {string} toml
   */
  setText(toml) {
    this.textarea.value = toml;
    AppState.toml = toml;
  }

  async _loadPreset() {
    const path = this.presetSelect.value;
    if (!path) return;

    try {
      const resp = await fetch(path);
      if (!resp.ok) throw new Error(`Failed to fetch ${path}`);
      const toml = await resp.text();
      this.setText(toml);
      this._clearValidation();
    } catch (e) {
      this._showError(`Failed to load preset: ${e.message}`);
    }
  }

  _validate() {
    const toml = this.textarea.value.trim();
    if (!toml) {
      this._showError('No TOML content to validate');
      return false;
    }

    try {
      this.wasm.validate_scenario_wasm(toml);
      this._showSuccess('Scenario is valid');
      return true;
    } catch (e) {
      this._showError(String(e));
      return false;
    }
  }

  _loadAndRun() {
    const toml = this.textarea.value.trim();
    if (!toml) {
      this._showError('No TOML content to load');
      return;
    }

    try {
      // Validate first.
      this.wasm.validate_scenario_wasm(toml);
    } catch (e) {
      this._showError(String(e));
      return;
    }

    try {
      // Parse scenario for map/UI.
      // Convert Map objects from serde_wasm_bindgen to plain objects.
      const scenario = mapsToObjects(this.wasm.load_scenario(toml));
      AppState.scenario = scenario;
      AppState.toml = toml;

      // Create engine.
      const WasmEngine = AppState._WasmEngine;
      if (!WasmEngine) {
        this._showError('WASM engine not available');
        return;
      }

      AppState.engine = new WasmEngine(toml);
      AppState.currentSnapshot = null;
      AppState.snapshots = [];
      AppState.eventLog = [];
      AppState.mcResult = null;

      this._showSuccess('Scenario loaded');

      this.bus.emit('scenario:loaded', scenario);
    } catch (e) {
      this._showError(`Load error: ${e}`);
    }
  }

  _export() {
    const toml = this.textarea.value;
    if (!toml) return;

    const blob = new Blob([toml], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'scenario.toml';
    a.click();
    URL.revokeObjectURL(url);
  }

  _import(e) {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = () => {
      this.setText(reader.result);
      this._clearValidation();
    };
    reader.readAsText(file);

    // Reset the input so the same file can be re-imported.
    e.target.value = '';
  }

  _switchTab(tab) {
    const targetId = tab.dataset.tab;

    document.querySelectorAll('.app-tab').forEach((t) => t.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach((c) => c.classList.remove('active'));

    tab.classList.add('active');
    const target = document.getElementById(targetId);
    if (target) target.classList.add('active');
  }

  _showError(msg) {
    this.validationMsg.className = 'validation-msg error';
    this.validationMsg.textContent = msg;
  }

  _showSuccess(msg) {
    this.validationMsg.className = 'validation-msg success';
    this.validationMsg.textContent = msg;
  }

  _clearValidation() {
    this.validationMsg.className = '';
    this.validationMsg.textContent = '';
  }
}
