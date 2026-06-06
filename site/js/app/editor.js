/**
 * TOML scenario editor with validation, preset loading, and file I/O.
 */
import { AppState } from './state.js';
import { PRESETS } from './presets.js';
import { mapsToObjects } from './wasm-util.js';
import { buildShareUrl } from './sharing.js';
import { renderDiff } from './diff.js';
import { renderWarnings, warningsClean, renderExplain, renderFieldDoc } from './explain-panel.js';
import { docAtOffset } from './field-docs.js';
/** @typedef {import('./pinned.js').PinnedStore} PinnedStore */

export class Editor {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   * @param {object} wasm - WASM module exports
   * @param {PinnedStore} pinned - Shared pinned store (same instance the
   *   dashboard gets, so diff baselines reflect pins added during the
   *   session without a page reload).
   */
  constructor(bus, wasm, pinned) {
    this.bus = bus;
    this.wasm = wasm;

    // DOM elements.
    this.textarea = document.getElementById('toml-editor');
    this.presetSelect = document.getElementById('preset-select');
    this.btnValidate = document.getElementById('btn-validate');
    this.btnExplain = document.getElementById('btn-explain');
    this.btnLoad = document.getElementById('btn-load');
    this.btnDiff = document.getElementById('btn-diff');
    this.btnImport = document.getElementById('btn-import');
    this.btnExport = document.getElementById('btn-export');
    this.btnShare = document.getElementById('btn-share');
    this.fileInput = document.getElementById('file-import');
    this.validationMsg = document.getElementById('validation-msg');
    this.warningsPanel = document.getElementById('warnings-panel');
    this.explainPanel = document.getElementById('explain-panel');

    // Last preset/imported text — used as one of the diff baselines so
    // the user can see "what I changed since loading this scenario".
    this._loadedBaselineToml = '';
    this._loadedBaselineLabel = '';

    // Share the same pinned store the dashboard uses so diff baselines
    // stay in sync with pinned MC results.
    this.pinned = pinned;

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
    if (this.btnExplain) this.btnExplain.addEventListener('click', () => this._explain());
    this.btnLoad.addEventListener('click', () => this._loadAndRun());
    if (this.btnDiff) this.btnDiff.addEventListener('click', () => this._openDiff());
    this.btnImport.addEventListener('click', () => this.fileInput.click());
    this.btnExport.addEventListener('click', () => this._export());
    if (this.btnShare) this.btnShare.addEventListener('click', () => this._share());
    this.fileInput.addEventListener('change', (e) => this._import(e));

    // Schema-aware hover documentation: hovering (or moving the caret to) a
    // field key pops a tooltip explaining what the field means, its type,
    // default, and whether it has an engine effect.
    this._initFieldDocs();

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

  // -------------------------------------------------------------------
  // Schema-aware hover documentation
  // -------------------------------------------------------------------

  /**
   * Wire the hover-documentation tooltip on the TOML editor textarea.
   *
   * Triggering is dual-mode so it works for both mouse and keyboard users:
   *   - `mousemove` over the textarea resolves the character offset under
   *     the pointer (via the standard caret-from-point APIs) and shows the
   *     doc for the field key there.
   *   - `keyup` / `click` resolves the key at the current caret position, so
   *     arrowing onto a key (no mouse) still surfaces its docs.
   * `mouseleave`, scroll, blur, and Escape dismiss the tooltip.
   *
   * The tooltip element is created lazily and appended to <body> so it can
   * escape the sidebar's overflow clipping; it is positioned in viewport
   * coordinates near the trigger point. Everything is vanilla DOM — no new
   * dependencies — consistent with the rest of site/js.
   */
  _initFieldDocs() {
    if (!this.textarea) return;

    // The most recently shown key, so repeated mousemove events over the
    // same word don't thrash the DOM.
    this._fieldDocKey = null;

    const onMove = (e) => this._handleFieldDocHover(e.clientX, e.clientY);
    this.textarea.addEventListener('mousemove', onMove);
    this.textarea.addEventListener('mouseleave', () => this._hideFieldDoc());
    // Caret-driven (keyboard / click) lookup.
    this.textarea.addEventListener('keyup', () => this._showFieldDocAtCaret());
    this.textarea.addEventListener('click', () => this._showFieldDocAtCaret());
    // Dismiss on anything that would move the textarea content out from
    // under a positioned tooltip, or on editing.
    this.textarea.addEventListener('scroll', () => this._hideFieldDoc());
    this.textarea.addEventListener('blur', () => this._hideFieldDoc());
    this.textarea.addEventListener('input', () => this._hideFieldDoc());
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') this._hideFieldDoc();
    });
  }

  /**
   * Resolve and show the doc for the field key under a viewport point.
   * @param {number} clientX
   * @param {number} clientY
   */
  _handleFieldDocHover(clientX, clientY) {
    const offset = this._offsetFromPoint(clientX, clientY);
    if (offset == null) {
      this._hideFieldDoc();
      return;
    }
    const doc = docAtOffset(this.textarea.value, offset);
    if (!doc) {
      this._hideFieldDoc();
      return;
    }
    this._showFieldDoc(doc, clientX, clientY);
  }

  /** Resolve and show the doc for the field key at the current caret. */
  _showFieldDocAtCaret() {
    const offset = this.textarea.selectionStart;
    if (typeof offset !== 'number') return;
    const doc = docAtOffset(this.textarea.value, offset);
    if (!doc) {
      this._hideFieldDoc();
      return;
    }
    // Anchor the caret-driven tooltip near the textarea's top-left rather
    // than chasing an invisible caret rectangle (textareas don't expose
    // per-caret geometry). Good enough to surface the doc on keyboard nav.
    const rect = this.textarea.getBoundingClientRect();
    this._showFieldDoc(doc, rect.left + 16, rect.top + 16);
  }

  /**
   * Map a viewport point to a character offset in the textarea, or null if
   * the point isn't over editable text. Uses the standard
   * `caretPositionFromPoint` (Firefox) / `caretRangeFromPoint` (WebKit/
   * Blink) APIs, which both work for <textarea> content.
   * @param {number} clientX
   * @param {number} clientY
   * @returns {number | null}
   */
  _offsetFromPoint(clientX, clientY) {
    const doc = document;
    if (typeof doc.caretPositionFromPoint === 'function') {
      const pos = doc.caretPositionFromPoint(clientX, clientY);
      if (pos && pos.offsetNode) return pos.offset;
    }
    if (typeof doc.caretRangeFromPoint === 'function') {
      const range = doc.caretRangeFromPoint(clientX, clientY);
      if (range) return range.startOffset;
    }
    return null;
  }

  /**
   * Render and position the field-doc tooltip.
   * @param {{key: string}} doc
   * @param {number} clientX
   * @param {number} clientY
   */
  _showFieldDoc(doc, clientX, clientY) {
    // Skip re-render if the same key is already shown — keeps mousemove cheap.
    if (this._fieldDocKey === doc.key && this._fieldDocEl && !this._fieldDocEl.hidden) {
      return;
    }
    this._fieldDocKey = doc.key;

    if (!this._fieldDocEl) {
      const el = document.createElement('div');
      el.className = 'field-doc-tooltip';
      el.setAttribute('role', 'tooltip');
      el.hidden = true;
      document.body.appendChild(el);
      this._fieldDocEl = el;
    }

    this._fieldDocEl.innerHTML = renderFieldDoc(doc);
    this._fieldDocEl.hidden = false;

    // Position just below-right of the trigger point, then nudge back inside
    // the viewport so the tooltip never clips off-screen.
    const margin = 12;
    const rect = this._fieldDocEl.getBoundingClientRect();
    let left = clientX + 14;
    let top = clientY + 16;
    if (left + rect.width + margin > window.innerWidth) {
      left = Math.max(margin, window.innerWidth - rect.width - margin);
    }
    if (top + rect.height + margin > window.innerHeight) {
      top = Math.max(margin, clientY - rect.height - 12);
    }
    this._fieldDocEl.style.left = `${left}px`;
    this._fieldDocEl.style.top = `${top}px`;
  }

  /** Hide the field-doc tooltip. */
  _hideFieldDoc() {
    this._fieldDocKey = null;
    if (this._fieldDocEl) {
      this._fieldDocEl.hidden = true;
    }
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

  /**
   * Explain the current scenario without running a simulation: render the
   * structured "what does this scenario model?" summary (same producer as
   * the CLI `--explain`) and surface the inline advisory-warnings panel
   * (factions with no objective, unreferenced regions, unreachable
   * kill-chain phases).
   *
   * Both call into WASM exports that load the scenario but never simulate,
   * so this stays cheap enough to run on demand. A scenario that fails to
   * load (bad TOML, refused migration) surfaces the load error in the
   * usual validation message and leaves the panels hidden.
   */
  _explain() {
    const toml = this.textarea.value.trim();
    if (!toml) {
      this._showError('No TOML content to explain');
      return;
    }

    // Advisory warnings — non-fatal. A loadable scenario always yields a
    // (possibly empty) report; only a load failure throws.
    let warningsReport;
    try {
      warningsReport = mapsToObjects(this.wasm.scenario_warnings_wasm(toml));
    } catch (e) {
      this._showError(String(e));
      this._hidePanels();
      return;
    }
    this._showWarnings(warningsReport);

    // Explain summary. The export returns `{ markdown, report }`; we
    // render the Markdown verbatim in the panel.
    try {
      const result = mapsToObjects(this.wasm.explain_scenario_wasm(toml));
      this._showExplain(result && result.markdown ? result.markdown : '');
      this._showSuccess('Scenario explained');
    } catch (e) {
      this._showError(String(e));
      this._hideExplain();
    }
  }

  _showWarnings(report) {
    if (!this.warningsPanel) return;
    this.warningsPanel.innerHTML = renderWarnings(report);
    this.warningsPanel.classList.toggle('clean', warningsClean(report));
    this.warningsPanel.hidden = false;
  }

  _showExplain(markdown) {
    if (!this.explainPanel) return;
    this.explainPanel.innerHTML = renderExplain(markdown);
    this.explainPanel.hidden = false;
  }

  _hideExplain() {
    if (this.explainPanel) {
      this.explainPanel.hidden = true;
      this.explainPanel.innerHTML = '';
    }
  }

  _hidePanels() {
    if (this.warningsPanel) {
      this.warningsPanel.hidden = true;
      this.warningsPanel.innerHTML = '';
    }
    this._hideExplain();
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
    // If a previous modal is still open, route its dismissal through the
    // saved close() so its keydown listener gets unregistered. Removing
    // the DOM node directly would orphan the handler on document.
    if (this._closeDiffModal) {
      this._closeDiffModal();
    }

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

    // Single close path so the document-level keydown listener gets
    // unregistered no matter how the modal is dismissed (X button,
    // backdrop click, Escape, or another _showDiffModal call). Without
    // this the listener stayed attached after modal.remove() and
    // accumulated across repeated open/close.
    const escHandler = (ev) => {
      if (ev.key === 'Escape') close();
    };
    const close = () => {
      document.removeEventListener('keydown', escHandler);
      modal.remove();
      if (this._closeDiffModal === close) {
        this._closeDiffModal = null;
      }
    };
    this._closeDiffModal = close;
    modal.querySelector('.diff-modal-close').addEventListener('click', close);
    modal.addEventListener('click', (e) => {
      // Backdrop click closes; clicks inside the card don't.
      if (e.target === modal) close();
    });
    document.addEventListener('keydown', escHandler);
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
    // Stale explain / warnings output for the previous scenario would be
    // misleading once a new preset / import replaces the editor text.
    this._hidePanels();
  }
}
