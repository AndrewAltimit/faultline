/**
 * TOML scenario editor with validation, preset loading, and file I/O.
 */
import { AppState } from './state.js';
import { PRESETS } from './presets.js';
import { mapsToObjects } from './wasm-util.js';
import { buildShareUrl } from './sharing.js';
import { renderDiff } from './diff.js';
import { PinnedStore } from './pinned.js';

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
    this.btnDiff = document.getElementById('btn-diff');
    this.btnImport = document.getElementById('btn-import');
    this.btnExport = document.getElementById('btn-export');
    this.btnShare = document.getElementById('btn-share');
    this.fileInput = document.getElementById('file-import');
    this.validationMsg = document.getElementById('validation-msg');

    // Last preset/imported text — used as one of the diff baselines so
    // the user can see "what I changed since loading this scenario".
    this._loadedBaselineToml = '';
    this._loadedBaselineLabel = '';

    // Share the same pinned store the dashboard uses so diff baselines
    // stay in sync with pinned MC results.
    this.pinned = new PinnedStore();

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
    if (this.btnDiff) this.btnDiff.addEventListener('click', () => this._openDiff());
    this.btnImport.addEventListener('click', () => this.fileInput.click());
    this.btnExport.addEventListener('click', () => this._export());
    if (this.btnShare) this.btnShare.addEventListener('click', () => this._share());
    this.fileInput.addEventListener('change', (e) => this._import(e));

    // Other modules can request the editor load arbitrary TOML
    // (e.g. dashboard "Load TOML" on a pinned result).
    this.bus.on('editor:load-toml', ({ toml, source }) => {
      if (typeof toml !== 'string') return;
      this.setText(toml);
      this._loadedBaselineToml = toml;
      this._loadedBaselineLabel = source || 'loaded';
      this._showSuccess(`Loaded TOML from ${source || 'pin'}`);
    });
  }

  async _share() {
    const toml = this.textarea.value.trim();
    if (!toml) {
      this._showError('No TOML content to share');
      return;
    }
    try {
      const url = await buildShareUrl(toml);
      try {
        await navigator.clipboard.writeText(url);
        this._showSuccess(`Share URL copied to clipboard (${url.length} chars)`);
      } catch {
        // Clipboard may be blocked (e.g. insecure context). Fall back to
        // updating the address bar so the user can copy manually.
        history.replaceState(null, '', new URL(url).hash);
        this._showSuccess('Share URL placed in the address bar — copy from there');
      }
    } catch (e) {
      this._showError(`Share failed: ${e}`);
    }
  }

  /**
   * Set the editor text content.
   * @param {string} toml
   */
  setText(toml) {
    this.textarea.value = toml;
    AppState.toml = toml;
  }

  /**
   * Record `(toml, label)` as the "last loaded baseline" used by the
   * Diff button. Called by bootstrap when it auto-loads the default
   * scenario without going through the preset dropdown.
   */
  setDiffBaseline(toml, label) {
    this._loadedBaselineToml = toml;
    this._loadedBaselineLabel = label || 'baseline';
  }

  /** Public entry point so bootstrap can auto-load the default scenario. */
  loadAndRun() {
    this._loadAndRun();
  }

  async _loadPreset() {
    const path = this.presetSelect.value;
    if (!path) return;

    try {
      const resp = await fetch(path);
      if (!resp.ok) throw new Error(`Failed to fetch ${path}`);
      const toml = await resp.text();
      this.setText(toml);
      this._loadedBaselineToml = toml;
      // The preset dropdown holds the path; the option text is the
      // human-readable name. Use the latter when both are available.
      const opt = this.presetSelect.selectedOptions?.[0];
      this._loadedBaselineLabel = opt?.textContent?.trim() || path;
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
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  _import(e) {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result || '');
      this.setText(text);
      this._loadedBaselineToml = text;
      this._loadedBaselineLabel = file.name || 'imported';
      this._clearValidation();
    };
    reader.readAsText(file);

    // Reset the input so the same file can be re-imported.
    e.target.value = '';
  }

  // -------------------------------------------------------------------
  // Diff viewer
  // -------------------------------------------------------------------

  _openDiff() {
    const current = this.textarea.value;
    const baselines = this._collectDiffBaselines();
    if (baselines.length === 0) {
      this._showError('No baseline available — load a preset, import a file, or pin a result first.');
      return;
    }
    this._showDiffModal(current, baselines);
  }

  /**
   * Build the list of baselines the user can diff against:
   *   1. The most recently loaded preset / imported file (if any).
   *   2. Each pinned MC result that captured a TOML payload.
   *
   * Order matters — the first entry is selected by default in the modal.
   */
  _collectDiffBaselines() {
    const out = [];
    if (this._loadedBaselineToml) {
      out.push({
        id: '__loaded__',
        label: `Last loaded: ${this._loadedBaselineLabel || 'preset/import'}`,
        toml: this._loadedBaselineToml,
      });
    }
    for (const pin of this.pinned.list()) {
      if (pin.toml && pin.toml.trim()) {
        out.push({
          id: pin.id,
          label: `Pin: ${pin.label}`,
          toml: pin.toml,
        });
      }
    }
    return out;
  }

  _showDiffModal(currentToml, baselines) {
    // Remove any existing modal so repeated clicks just refresh.
    document.querySelectorAll('.diff-modal').forEach((el) => el.remove());

    const modal = document.createElement('div');
    modal.className = 'diff-modal';
    modal.innerHTML = `
      <div class="diff-modal-card">
        <div class="diff-modal-header">
          <div class="diff-modal-title">Scenario Diff</div>
          <button class="diff-modal-close" aria-label="Close">×</button>
        </div>
        <div class="diff-modal-controls">
          <label for="diff-baseline-select">Compare current against:</label>
          <select id="diff-baseline-select" class="preset-select" style="flex:1; min-width:160px;"></select>
        </div>
        <div class="diff-modal-body" id="diff-modal-body"></div>
      </div>
    `;
    document.body.appendChild(modal);

    const select = modal.querySelector('#diff-baseline-select');
    for (const b of baselines) {
      const opt = document.createElement('option');
      opt.value = b.id;
      opt.textContent = b.label;
      select.appendChild(opt);
    }

    const body = modal.querySelector('#diff-modal-body');
    const update = () => {
      const id = select.value;
      const chosen = baselines.find((b) => b.id === id) || baselines[0];
      body.innerHTML = renderDiff(chosen.toml, currentToml, {
        baselineLabel: chosen.label,
        variantLabel: 'current editor',
      });
    };
    select.addEventListener('change', update);
    update();

    const close = () => modal.remove();
    modal.querySelector('.diff-modal-close').addEventListener('click', close);
    modal.addEventListener('click', (e) => {
      // Backdrop click closes; clicks inside the card don't.
      if (e.target === modal) close();
    });
    document.addEventListener(
      'keydown',
      function escHandler(ev) {
        if (ev.key === 'Escape') {
          close();
          document.removeEventListener('keydown', escHandler);
        }
      },
    );
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
