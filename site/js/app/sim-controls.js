/**
 * Simulation controls: play/pause/step/speed/reset/timeline.
 * Manages the WasmEngine lifecycle and tick animation loop.
 */
import { AppState } from './state.js';
import { mapsToObjects } from './wasm-util.js';

const SPEED_STEPS = [1, 2, 5, 10, 25, 50];

export class SimControls {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   */
  constructor(bus) {
    this.bus = bus;
    this._animFrameId = null;

    // DOM elements.
    this.btnPlay = document.getElementById('btn-play');
    this.btnStep = document.getElementById('btn-step');
    this.btnReset = document.getElementById('btn-reset');
    this.speedSlider = document.getElementById('speed-slider');
    this.speedLabel = document.getElementById('speed-label');
    this.tickDisplay = document.getElementById('tick-display');
    this.tickMax = document.getElementById('tick-max');
    this.timelineSlider = document.getElementById('timeline-slider');
    this.playIcon = document.getElementById('play-icon');

    // Bind events.
    this.btnPlay.addEventListener('click', () => this._togglePlay());
    this.btnStep.addEventListener('click', () => this._step());
    this.btnReset.addEventListener('click', () => this._reset());
    this.speedSlider.addEventListener('input', () => this._updateSpeed());
    this.timelineSlider.addEventListener('input', () => this._scrub());

    // Listen for scenario loaded.
    bus.on('scenario:loaded', () => this._onScenarioLoaded());
  }

  _onScenarioLoaded() {
    this.btnPlay.disabled = false;
    this.btnStep.disabled = false;
    this.btnReset.disabled = false;
    this.timelineSlider.disabled = false;

    const engine = AppState.engine;
    if (engine) {
      this.tickMax.textContent = engine.max_ticks();
      this.timelineSlider.max = engine.max_ticks();
    }

    this._updateTickDisplay();
  }

  _togglePlay() {
    if (AppState.isPlaying) {
      this._pause();
    } else {
      this._play();
    }
  }

  _play() {
    if (!AppState.engine || AppState.engine.is_finished()) return;

    AppState.isPlaying = true;
    this._setPlayIcon(true);
    this.btnPlay.classList.add('active');
    this._animLoop();
  }

  _pause() {
    AppState.isPlaying = false;
    this._setPlayIcon(false);
    this.btnPlay.classList.remove('active');
    if (this._animFrameId) {
      cancelAnimationFrame(this._animFrameId);
      this._animFrameId = null;
    }
  }

  _animLoop() {
    if (!AppState.isPlaying || !AppState.engine) return;

    const engine = AppState.engine;
    if (engine.is_finished()) {
      this._pause();
      this.bus.emit('sim:finished');
      return;
    }

    try {
      const tickResults = mapsToObjects(engine.tick_n(AppState.playSpeed));
      const state = mapsToObjects(engine.get_state());

      AppState.currentSnapshot = state;
      AppState.snapshots.push(state);

      // Check for events in tick results.
      const results = Array.isArray(tickResults) ? tickResults : [];
      for (const tr of results) {
        if (tr.events_fired && tr.events_fired.length > 0) {
          for (const eid of tr.events_fired) {
            AppState.eventLog.push({ tick: tr.tick, event_id: eid });
          }
        }
      }

      this.bus.emit('sim:tick', state);
      this._updateTickDisplay();

      // Update timeline slider.
      this.timelineSlider.value = engine.current_tick();

      // Check if finished after this tick batch.
      if (engine.is_finished()) {
        const lastResult = results[results.length - 1];
        if (lastResult?.outcome) {
          this.bus.emit('sim:finished', lastResult.outcome);
        } else {
          this.bus.emit('sim:finished');
        }
        this._pause();
        return;
      }
    } catch (e) {
      console.error('Tick error:', e);
      this._pause();
      return;
    }

    this._animFrameId = requestAnimationFrame(() => this._animLoop());
  }

  _step() {
    if (!AppState.engine || AppState.engine.is_finished()) return;

    try {
      const tickResults = mapsToObjects(AppState.engine.tick_n(1));
      const state = mapsToObjects(AppState.engine.get_state());

      AppState.currentSnapshot = state;
      AppState.snapshots.push(state);

      // Collect events.
      const results = Array.isArray(tickResults) ? tickResults : [];
      for (const tr of results) {
        if (tr.events_fired && tr.events_fired.length > 0) {
          for (const eid of tr.events_fired) {
            AppState.eventLog.push({ tick: tr.tick, event_id: eid });
          }
        }
      }

      this.bus.emit('sim:tick', state);
      this._updateTickDisplay();
      this.timelineSlider.value = AppState.engine.current_tick();

      if (AppState.engine.is_finished()) {
        const lastResult = results[results.length - 1];
        this.bus.emit('sim:finished', lastResult?.outcome);
      }
    } catch (e) {
      console.error('Step error:', e);
    }
  }

  _reset() {
    this._pause();

    if (!AppState.toml) return;

    try {
      const WasmEngine = AppState._WasmEngine;
      if (!WasmEngine) return;

      AppState.engine = new WasmEngine(AppState.toml);
      AppState.currentSnapshot = null;
      AppState.snapshots = [];
      AppState.eventLog = [];

      this.timelineSlider.value = 0;
      this._updateTickDisplay();

      this.bus.emit('sim:reset');
    } catch (e) {
      console.error('Reset error:', e);
    }
  }

  _scrub() {
    const targetTick = parseInt(this.timelineSlider.value, 10);

    // Find the nearest snapshot.
    let closest = null;
    let closestDist = Infinity;
    for (const snap of AppState.snapshots) {
      const dist = Math.abs(snap.tick - targetTick);
      if (dist < closestDist) {
        closestDist = dist;
        closest = snap;
      }
    }

    if (closest) {
      this.bus.emit('sim:snapshot', closest);
    }
  }

  _updateSpeed() {
    const idx = parseInt(this.speedSlider.value, 10);
    AppState.playSpeed = SPEED_STEPS[idx] || 1;
    this.speedLabel.textContent = `${AppState.playSpeed}x`;
  }

  _updateTickDisplay() {
    const engine = AppState.engine;
    const tick = engine ? engine.current_tick() : 0;
    const max = engine ? engine.max_ticks() : 0;

    this.tickDisplay.innerHTML =
      `Tick <span class="tick-value">${tick}</span> / <span class="tick-value">${max}</span>`;
  }

  _setPlayIcon(playing) {
    if (playing) {
      this.playIcon.innerHTML =
        '<rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>';
    } else {
      this.playIcon.innerHTML = '<polygon points="5 3 19 12 5 21 5 3"/>';
    }
  }
}
