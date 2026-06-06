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
import { completionsAt } from './autocomplete.js';
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

    // Schema-driven autocomplete: as the user types a key (or an enum value),
    // offer valid completions for the current `[table]` context, sourced from
    // the same field-doc catalog that powers the hover tooltip.
    this._initAutocomplete();

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
   * Wire the field-documentation tooltip on the TOML editor textarea.
   *
   * Triggering is caret-driven via `keyup` / `click`, which resolves the
   * field key at the current caret position. This works for both mouse users
   * (clicking into a field sets `selectionStart`) and keyboard users (arrowing
   * onto a key). We deliberately do *not* use a `mousemove` + caret-from-point
   * path: `caretPositionFromPoint` / `caretRangeFromPoint` treat a `<textarea>`
   * as an opaque native widget and return a position anchored to the element
   * (offset 0), not a character index into `textarea.value`, so pointer hover
   * could not resolve the field under the cursor.
   * Scroll, blur, input, and Escape dismiss the tooltip.
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

  // -------------------------------------------------------------------
  // Schema-driven autocomplete
  // -------------------------------------------------------------------

  /**
   * Wire the schema-driven completion popover on the editor textarea.
   *
   * A focused custom popover over the existing `<textarea>` (rather than a
   * Monaco/CodeMirror swap) keeps the frontend dependency-free and low-risk.
   * The completion *content* — which keys are valid in the current `[table]`
   * and which enum values a field accepts — comes entirely from
   * {@link completionsAt}, which reads the shared field-doc catalog.
   *
   * Interaction model:
   *   - `input` recomputes completions for the caret context and (re)shows the
   *     popover when there are matches and a non-empty token is being typed, or
   *     when the user explicitly invokes it.
   *   - ArrowUp/ArrowDown move the highlighted item; Enter/Tab accept it;
   *     Escape dismisses. These are intercepted only while the popover is open
   *     so normal editing is unaffected.
   *   - Ctrl/Cmd+Space force-opens the popover even on an empty token (the
   *     "show me everything valid here" affordance).
   *   - Blur / scroll / clicking elsewhere dismiss it.
   */
  _initAutocomplete() {
    if (!this.textarea) return;

    /** @type {import('./autocomplete.js').Completion[]} */
    this._acItems = [];
    this._acIndex = 0;
    this._acContext = null;

    this.textarea.addEventListener('input', () => this._updateAutocomplete(false));
    this.textarea.addEventListener('keydown', (e) => this._onAutocompleteKeydown(e));
    this.textarea.addEventListener('blur', () => {
      // Defer so a click on a popover item lands before the popover is torn
      // down (mousedown on the item fires before the textarea blur completes,
      // but the click handler runs after).
      setTimeout(() => this._hideAutocomplete(), 120);
    });
    this.textarea.addEventListener('scroll', () => this._hideAutocomplete());
  }

  /**
   * Intercept navigation / accept / dismiss keys while the popover is open,
   * and the force-open chord. Returns early (doing nothing) when the popover
   * is closed so ordinary typing is never swallowed.
   * @param {KeyboardEvent} e
   */
  _onAutocompleteKeydown(e) {
    // Force-open: Ctrl/Cmd + Space.
    if ((e.ctrlKey || e.metaKey) && (e.key === ' ' || e.code === 'Space')) {
      e.preventDefault();
      this._updateAutocomplete(true);
      return;
    }

    if (!this._acItems || this._acItems.length === 0 || !this._acEl || this._acEl.hidden) {
      return;
    }

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        this._acIndex = (this._acIndex + 1) % this._acItems.length;
        this._renderAutocomplete();
        break;
      case 'ArrowUp':
        e.preventDefault();
        this._acIndex = (this._acIndex - 1 + this._acItems.length) % this._acItems.length;
        this._renderAutocomplete();
        break;
      case 'Enter':
      case 'Tab':
        e.preventDefault();
        this._acceptCompletion(this._acItems[this._acIndex]);
        break;
      case 'Escape':
        e.preventDefault();
        this._hideAutocomplete();
        break;
      default:
        break;
    }
  }

  /**
   * Recompute completions for the current caret and show/hide the popover.
   * @param {boolean} forced True when invoked via the force-open chord, which
   *   shows the popover even on an empty token.
   */
  _updateAutocomplete(forced) {
    const offset = this.textarea.selectionStart;
    if (typeof offset !== 'number') {
      this._hideAutocomplete();
      return;
    }
    const { context, items } = completionsAt(this.textarea.value, offset);
    // When not forced, only auto-pop once the user has begun a token — popping
    // on every keystroke (including right after `=` with no prefix) is noisy.
    if (!forced && (!context.prefix || context.prefix.length === 0)) {
      this._hideAutocomplete();
      return;
    }
    if (!items || items.length === 0) {
      this._hideAutocomplete();
      return;
    }
    this._acItems = items;
    this._acContext = context;
    this._acIndex = 0;
    this._renderAutocomplete();
  }

  /** Render the popover list and position it near the textarea caret. */
  _renderAutocomplete() {
    if (!this._acEl) {
      const el = document.createElement('div');
      el.className = 'ac-popover';
      el.setAttribute('role', 'listbox');
      el.hidden = true;
      document.body.appendChild(el);
      this._acEl = el;
    }

    const rows = this._acItems
      .map((item, i) => {
        const active = i === this._acIndex ? ' active' : '';
        const effectClass = item.engineEffect ? 'has-effect' : 'no-effect';
        const effectLabel = item.engineEffect ? 'engine' : 'descriptive';
        const meta = [];
        if (item.type) meta.push(this._acEsc(item.type));
        if (item.range) meta.push(this._acEsc(item.range));
        if (item.default !== undefined && item.default !== null && item.default !== '') {
          meta.push(`default ${this._acEsc(item.default)}`);
        }
        const summary = item.summary
          ? `<div class="ac-item-summary">${this._acEsc(item.summary)}</div>`
          : '';
        return `<div class="ac-item${active}" role="option" data-index="${i}" aria-selected="${i === this._acIndex}">
  <div class="ac-item-head">
    <span class="ac-item-label">${this._acEsc(item.label)}</span>
    <span class="ac-item-effect ${effectClass}">${effectLabel}</span>
  </div>
  <div class="ac-item-meta">${meta.join(' · ')}</div>
  ${summary}
</div>`;
      })
      .join('');
    this._acEl.innerHTML = rows;
    this._acEl.hidden = false;

    // Click-to-accept (mousedown so it precedes the textarea blur).
    this._acEl.querySelectorAll('.ac-item').forEach((row) => {
      row.addEventListener('mousedown', (ev) => {
        ev.preventDefault();
        const idx = Number(row.getAttribute('data-index'));
        this._acceptCompletion(this._acItems[idx]);
      });
    });

    // Position the popover below the (approximate) caret. Textareas don't
    // expose per-caret geometry, so we estimate from the caret's line/column
    // using the computed font metrics — good enough to anchor the list near
    // where the user is typing, then clamp inside the viewport.
    const pos = this._caretClientPoint();
    const margin = 12;
    const rect = this._acEl.getBoundingClientRect();
    let left = pos.x;
    let top = pos.y + 4;
    if (left + rect.width + margin > window.innerWidth) {
      left = Math.max(margin, window.innerWidth - rect.width - margin);
    }
    if (top + rect.height + margin > window.innerHeight) {
      top = Math.max(margin, pos.y - rect.height - 4);
    }
    this._acEl.style.left = `${left}px`;
    this._acEl.style.top = `${top}px`;
  }

  /**
   * Estimate the caret's viewport point from its line/column and the
   * textarea's font metrics, accounting for scroll and padding. Approximate by
   * design — a textarea is an opaque native widget — but stable enough to keep
   * the popover near the typing position.
   * @returns {{x: number, y: number}}
   */
  _caretClientPoint() {
    const ta = this.textarea;
    const rect = ta.getBoundingClientRect();
    const style = window.getComputedStyle(ta);
    const padL = parseFloat(style.paddingLeft) || 0;
    const padT = parseFloat(style.paddingTop) || 0;
    const fontSize = parseFloat(style.fontSize) || 13;
    const lineHeight = parseFloat(style.lineHeight) || fontSize * 1.6;
    // Monospace char width ≈ 0.6em for the editor's mono font.
    const charW = fontSize * 0.6;

    const upto = ta.value.slice(0, ta.selectionStart);
    const nl = upto.lastIndexOf('\n');
    const col = upto.length - (nl + 1);
    const row = upto.split('\n').length - 1;

    const x = rect.left + padL + col * charW - ta.scrollLeft;
    const y = rect.top + padT + (row + 1) * lineHeight - ta.scrollTop;
    // Clamp inside the textarea's visible box so an off-screen caret (long
    // scroll) still anchors the popover somewhere sensible.
    return {
      x: Math.min(Math.max(x, rect.left + 4), rect.right - 4),
      y: Math.min(Math.max(y, rect.top + 4), rect.bottom - 4),
    };
  }

  /**
   * Replace the partial token under the caret with the accepted completion.
   * For a value completion we also drop the surrounding quotes into place when
   * the field is a string-quoted enum (TOML enum values are bare-quoted
   * strings), matching how the schema serializes them.
   * @param {import('./autocomplete.js').Completion} item
   */
  _acceptCompletion(item) {
    if (!item || !this._acContext) {
      this._hideAutocomplete();
      return;
    }
    const ta = this.textarea;
    const value = ta.value;
    const caret = ta.selectionStart;
    const ctx = this._acContext;

    // The token being replaced spans [lineStart + tokenStart, caret).
    const lineStart = value.lastIndexOf('\n', caret - 1) + 1;
    const tokenAbsStart = lineStart + ctx.tokenStart;
    const before = value.slice(0, tokenAbsStart);
    const after = value.slice(caret);

    let insert = item.label;
    let newCaret = tokenAbsStart + insert.length;
    if (item.kind === 'key') {
      // If the line has no `=` yet, scaffold the assignment so the caret lands
      // ready to type the value.
      const lineEndRel = value.indexOf('\n', caret);
      const lineEnd = lineEndRel === -1 ? value.length : lineEndRel;
      const restOfLine = value.slice(caret, lineEnd);
      if (!restOfLine.includes('=')) {
        insert = `${item.label} = `;
        newCaret = tokenAbsStart + insert.length;
      }
    } else if (item.kind === 'value') {
      // Quote enum values the way TOML expects (bare-quoted strings). Only add
      // the closing quote if the user hasn't already typed one after the caret.
      const openedQuote = before.endsWith('"') || before.endsWith("'");
      const closing = after.startsWith('"') || after.startsWith("'") ? '' : '"';
      if (!openedQuote) {
        insert = `"${item.label}${closing}`;
        newCaret = tokenAbsStart + 1 + item.label.length + (closing ? 1 : 0);
      } else {
        insert = `${item.label}${closing}`;
        newCaret = tokenAbsStart + item.label.length + (closing ? 1 : 0);
      }
    }

    ta.value = before + insert + after;
    AppState.toml = ta.value;
    ta.selectionStart = ta.selectionEnd = newCaret;
    // A programmatic `value` assignment does not fire `input`, so the editor's
    // other input-driven consumers (field-doc dismissal, and any future
    // debounced parse/validation wired on `input`) would otherwise read stale
    // text until the next keystroke. Dispatch one so the accepted completion
    // drives the same path a typed character would.
    ta.dispatchEvent(new Event('input', { bubbles: true }));
    ta.focus();
    this._hideAutocomplete();
  }

  /** Hide and reset the completion popover. */
  _hideAutocomplete() {
    this._acItems = [];
    this._acContext = null;
    this._acIndex = 0;
    if (this._acEl) this._acEl.hidden = true;
  }

  /**
   * Local HTML escape for popover text. Mirrors `explain-panel`'s escape but
   * kept inline to avoid widening that module's import surface for one use.
   * @param {string} s
   */
  _acEsc(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
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
