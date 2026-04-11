/**
 * Faultline Simulator — main entry point.
 * Initializes the WASM module and bootstraps all app modules.
 */
import { EventBus } from './event-bus.js';
import { AppState } from './state.js';
import { MapRenderer } from './map-renderer.js';
import { SimControls } from './sim-controls.js';
import { Editor } from './editor.js';
import { Dashboard } from './dashboard.js';
import { FactionBuilder } from './faction-builder.js';

async function bootstrap() {
  const loading = document.getElementById('map-loading');
  const loadingText = document.getElementById('loading-text');

  // Show loading state.
  if (loading) loading.style.display = 'flex';

  let wasm;
  try {
    const wasmModule = await import('../pkg/faultline_backend_wasm.js');
    await wasmModule.default();
    wasmModule.init();
    wasm = wasmModule;

    // Store the WasmEngine constructor for other modules.
    AppState._WasmEngine = wasmModule.WasmEngine;

    if (loadingText) loadingText.textContent = 'WASM loaded';
  } catch (e) {
    console.warn('WASM module not available:', e);
    if (loadingText) {
      loadingText.textContent = 'WASM not available — build with wasm-pack first';
      loadingText.style.color = '#fca5a5';
    }
    // Keep the app functional for layout preview even without WASM.
    wasm = null;
  }

  // Hide loading overlay.
  if (loading) loading.style.display = 'none';

  // Initialize event bus and modules.
  const bus = new EventBus();
  const map = new MapRenderer(document.getElementById('map-canvas'), bus);

  // Only initialize WASM-dependent modules if WASM is available.
  let controls, editor, dashboard, builder;
  if (wasm) {
    controls = new SimControls(bus);
    editor = new Editor(bus, wasm);
    dashboard = new Dashboard(bus, wasm);
    builder = new FactionBuilder(bus);

    // Wire event subscriptions.

    // When a scenario is loaded, update the map.
    bus.on('scenario:loaded', (scenario) => {
      map.setScenario(scenario);

      // Render initial state from engine.
      if (AppState.engine) {
        const state = AppState.engine.get_state();
        AppState.currentSnapshot = state;
        map.render(state);
      }
    });

    // On tick, re-render map with new state.
    bus.on('sim:tick', (snapshot) => {
      map.render(snapshot);
    });

    // On snapshot selection (timeline scrub), render that snapshot.
    bus.on('sim:snapshot', (snapshot) => {
      AppState.currentSnapshot = snapshot;
      map.render(snapshot);
    });

    // On reset, re-render map with initial state.
    bus.on('sim:reset', () => {
      if (AppState.engine) {
        const state = AppState.engine.get_state();
        AppState.currentSnapshot = state;
        map.render(state);
      } else {
        map.render(null);
      }
    });

    // Load default preset on startup.
    try {
      const resp = await fetch('scenarios/tutorial_symmetric.toml');
      if (resp.ok) {
        const toml = await resp.text();
        editor.setText(toml);
        // Auto-select in dropdown.
        const select = document.getElementById('preset-select');
        if (select) select.value = 'scenarios/tutorial_symmetric.toml';
      }
    } catch {
      // Ignore — user can load manually.
    }
  }

  // Mobile nav toggle (same as main site).
  const hamburger = document.querySelector('.hamburger');
  const overlay = document.querySelector('.nav-overlay');
  if (hamburger && overlay) {
    hamburger.addEventListener('click', () => {
      hamburger.classList.toggle('open');
      overlay.classList.toggle('open');
    });
    overlay.querySelectorAll('a').forEach((a) => {
      a.addEventListener('click', () => {
        hamburger.classList.remove('open');
        overlay.classList.remove('open');
      });
    });
  }

  console.log('Faultline Simulator initialized');
}

bootstrap().catch((e) => console.error('Bootstrap failed:', e));
