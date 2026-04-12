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
import { mapsToObjects } from './wasm-util.js';
import { readScenarioFromHash, clearScenarioHash } from './sharing.js';
import { Tutorial } from './tutorial.js';
import { TechCardsPanel } from './tech-cards.js';

async function bootstrap() {
  const loading = document.getElementById('map-loading');
  const loadingText = document.getElementById('loading-text');

  // Show loading state.
  if (loading) loading.style.display = 'flex';

  let wasm;
  try {
    const wasmModule = await import('../../pkg/faultline_backend_wasm.js');
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

  // Wire up the tutorial button.
  const tutorial = new Tutorial();
  const btnTutorial = document.getElementById('btn-tutorial');
  if (btnTutorial) {
    btnTutorial.addEventListener('click', () => tutorial.start());
  }
  // Auto-show on first visit (suppressed if the user already saw it).
  if (Tutorial.shouldAutoShow()) {
    setTimeout(() => tutorial.start(), 800);
  }

  // Only initialize WASM-dependent modules if WASM is available.
  let controls, editor, dashboard, builder, techCards;
  if (wasm) {
    controls = new SimControls(bus);
    editor = new Editor(bus, wasm);
    dashboard = new Dashboard(bus, wasm);
    builder = new FactionBuilder(bus);
    techCards = new TechCardsPanel(bus);

    // Wire event subscriptions.

    // When a scenario is loaded, update the map.
    bus.on('scenario:loaded', (scenario) => {
      map.setScenario(scenario);

      // Render initial state from engine.
      if (AppState.engine) {
        const state = mapsToObjects(AppState.engine.get_state());
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
        const state = mapsToObjects(AppState.engine.get_state());
        AppState.currentSnapshot = state;
        map.render(state);
      } else {
        map.render(null);
      }
    });

    // If a scenario was shared via URL hash, prefer it over the default
    // preset. Otherwise fall back to the US institutional fracture
    // scenario. In both cases we auto-trigger Load & Run so the user
    // sees a populated map immediately and can still pick a different
    // scenario from the preset dropdown afterward.
    const sharedToml = await readScenarioFromHash();
    if (sharedToml) {
      editor.setText(sharedToml);
      clearScenarioHash();
      editor.loadAndRun();
    } else {
      try {
        const defaultPath = 'scenarios/us_institutional_fracture.toml';
        const resp = await fetch(defaultPath);
        if (resp.ok) {
          const toml = await resp.text();
          editor.setText(toml);
          const select = document.getElementById('preset-select');
          if (select) select.value = defaultPath;
          editor.loadAndRun();
        }
      } catch {
        // Ignore — user can load manually.
      }
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
