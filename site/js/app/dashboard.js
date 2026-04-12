/**
 * Results dashboard: event log, state inspector, Monte Carlo charts.
 * All charts rendered with Canvas 2D (no external dependencies).
 */
import { AppState } from './state.js';
import { mapsToObjects } from './wasm-util.js';

export class Dashboard {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   * @param {object} wasm - WASM module exports
   */
  constructor(bus, wasm) {
    this.bus = bus;
    this.wasm = wasm;

    this.eventLogList = document.getElementById('event-log-list');
    this.stateInspector = document.getElementById('state-inspector');
    this.mcResultsContainer = document.getElementById('mc-results');
    this.btnMcRun = document.getElementById('btn-mc-run');
    this.mcRunsInput = document.getElementById('mc-runs');

    this.btnMcRun.addEventListener('click', () => this._runMonteCarlo());
    this._mcWorker = null;
    this._mcRequestId = 0;

    // Sensitivity sweep controls.
    this.sensParam = document.getElementById('sens-param');
    this.sensLow = document.getElementById('sens-low');
    this.sensHigh = document.getElementById('sens-high');
    this.sensSteps = document.getElementById('sens-steps');
    this.sensRuns = document.getElementById('sens-runs');
    this.btnSensRun = document.getElementById('btn-sens-run');
    this.sensResultsContainer = document.getElementById('sens-results');
    if (this.btnSensRun) {
      this.btnSensRun.addEventListener('click', () => this._runSensitivity());
    }

    bus.on('sim:tick', (snapshot) => this._onTick(snapshot));
    bus.on('sim:snapshot', (snapshot) => this._onSnapshot(snapshot));
    bus.on('sim:finished', (outcome) => this._onFinished(outcome));
    bus.on('sim:reset', () => this._onReset());
    bus.on('scenario:loaded', () => this._onScenarioLoaded());
  }

  _onScenarioLoaded() {
    this.btnMcRun.disabled = false;
    if (this.btnSensRun) this.btnSensRun.disabled = false;
    this._onReset();
  }

  _onReset() {
    this.eventLogList.innerHTML = '<div class="event-log-empty">No events yet</div>';
    this.stateInspector.innerHTML =
      '<div class="empty-state" style="padding: 16px 0;"><span style="font-size: 0.8125rem;">Run a simulation to inspect state</span></div>';
  }

  _onTick(snapshot) {
    this._updateEventLog();
    this._updateStateInspector(snapshot);
  }

  _onSnapshot(snapshot) {
    this._updateStateInspector(snapshot);
  }

  _onFinished(outcome) {
    if (outcome) {
      const victor = outcome.victor || 'None (stalemate)';
      const condition = outcome.victory_condition || 'max ticks';
      const el = document.createElement('div');
      el.className = 'validation-msg success';
      el.style.margin = '8px 0 0 0';
      el.textContent = `Result: ${victor} wins via ${condition}`;
      this.stateInspector.appendChild(el);
    }
  }

  // -------------------------------------------------------------------
  // Event Log
  // -------------------------------------------------------------------

  _updateEventLog() {
    const events = AppState.eventLog;
    if (!events || events.length === 0) {
      this.eventLogList.innerHTML = '<div class="event-log-empty">No events yet</div>';
      return;
    }

    // Show last 100 events (most recent first).
    const recent = events.slice(-100).reverse();
    let html = '';
    for (const ev of recent) {
      const eventId = typeof ev.event_id === 'object' ? ev.event_id[0] || ev.event_id : ev.event_id;
      html += `
        <div class="event-log-item" data-tick="${ev.tick}">
          <span class="event-log-tick">T${ev.tick}</span>
          <span class="event-log-name">${this._esc(String(eventId))}</span>
        </div>`;
    }
    this.eventLogList.innerHTML = html;

    // Bind click to jump to tick.
    this.eventLogList.querySelectorAll('.event-log-item').forEach((item) => {
      item.addEventListener('click', () => {
        const tick = parseInt(item.dataset.tick, 10);
        const slider = document.getElementById('timeline-slider');
        if (slider) {
          slider.value = tick;
          slider.dispatchEvent(new Event('input'));
        }
      });
    });
  }

  // -------------------------------------------------------------------
  // State Inspector
  // -------------------------------------------------------------------

  _updateStateInspector(snapshot) {
    if (!snapshot || !snapshot.faction_states) {
      return;
    }

    const scenario = AppState.scenario;
    let html = '';

    for (const [fid, state] of Object.entries(snapshot.faction_states)) {
      const factionInfo = scenario?.factions?.[fid];
      const name = factionInfo?.name || fid;
      const color = this._safeColor(factionInfo?.color || '#7c5bf0');

      html += `
        <div class="state-faction">
          <div class="state-faction-header">
            <div class="state-faction-color" style="background: ${color};"></div>
            <span class="state-faction-name">${this._esc(name)}</span>
          </div>
          <div class="state-faction-details">
            <div class="state-row">
              <span class="label">Strength</span>
              <span class="value">${state.total_strength?.toFixed(1) ?? '?'}</span>
            </div>
            <div class="state-row">
              <span class="label">Morale</span>
              <span class="value">${state.morale?.toFixed(2) ?? '?'}</span>
            </div>
            <div class="state-row">
              <span class="label">Resources</span>
              <span class="value">${state.resources?.toFixed(1) ?? '?'}</span>
            </div>
            <div class="state-row">
              <span class="label">Regions</span>
              <span class="value">${(state.controlled_regions || []).length}</span>
            </div>
          </div>
        </div>`;
    }

    this.stateInspector.innerHTML = html;
  }

  // -------------------------------------------------------------------
  // Monte Carlo
  // -------------------------------------------------------------------

  _runMonteCarlo() {
    if (!AppState.toml) return;

    const numRuns = parseInt(this.mcRunsInput.value, 10) || 100;
    this.btnMcRun.disabled = true;
    this.btnMcRun.innerHTML = '<span class="spinner"></span> Running...';

    this._runMonteCarloInWorker(numRuns)
      .then((result) => {
        AppState.mcResult = result;
        this._renderMcResults(result.summary);
        this.bus.emit('mc:complete', result);
      })
      .catch((e) => {
        this.mcResultsContainer.innerHTML =
          `<div class="validation-msg error">${this._esc(String(e))}</div>`;
      })
      .finally(() => {
        this.btnMcRun.disabled = false;
        this.btnMcRun.textContent = 'Run MC';
      });
  }

  /**
   * Run a Monte Carlo batch in a dedicated web worker.
   *
   * Falls back to a synchronous in-thread call if the browser doesn't
   * support module workers. The worker is created lazily and reused
   * across runs so the WASM module isn't re-instantiated each time.
   */
  _runMonteCarloInWorker(numRuns) {
    if (typeof Worker === 'undefined') {
      // No worker support — run synchronously after a UI yield.
      return new Promise((resolve, reject) => {
        setTimeout(() => {
          try {
            resolve(
              mapsToObjects(
                this.wasm.run_monte_carlo(AppState.toml, numRuns, undefined, true),
              ),
            );
          } catch (e) {
            reject(e);
          }
        }, 50);
      });
    }

    if (!this._mcWorker) {
      try {
        this._mcWorker = new Worker(new URL('./mc-worker.js', import.meta.url), {
          type: 'module',
        });
      } catch (e) {
        console.warn('Failed to create MC worker, falling back to main thread:', e);
        this._mcWorker = null;
        return new Promise((resolve, reject) => {
          setTimeout(() => {
            try {
              resolve(
                mapsToObjects(
                  this.wasm.run_monte_carlo(AppState.toml, numRuns, undefined, true),
                ),
              );
            } catch (err) {
              reject(err);
            }
          }, 50);
        });
      }
    }

    const id = ++this._mcRequestId;
    return new Promise((resolve, reject) => {
      const onMessage = (ev) => {
        const msg = ev.data || {};
        if (msg.id !== id) return;
        this._mcWorker.removeEventListener('message', onMessage);
        if (msg.type === 'result') resolve(msg.result);
        else reject(new Error(msg.error || 'Monte Carlo worker failed'));
      };
      this._mcWorker.addEventListener('message', onMessage);
      this._mcWorker.postMessage({
        id,
        type: 'run',
        toml: AppState.toml,
        runs: numRuns,
        collectSnapshots: true,
      });
    });
  }

  _renderMcResults(summary) {
    if (!summary) return;

    const scenario = AppState.scenario;
    let html = '';

    // Win probability bar chart.
    html += '<div class="chart-title">Win Probability</div>';
    html += '<div class="chart-container"><canvas id="chart-win-prob" height="120"></canvas></div>';

    // Duration histogram.
    html += '<div class="chart-title">Duration Distribution</div>';
    html += '<div class="chart-container"><canvas id="chart-duration" height="120"></canvas></div>';

    // Summary stats.
    const duration = summary.metric_distributions?.Duration;
    html += '<div class="chart-title">Summary Statistics</div>';
    html += '<div class="mc-summary-stats">';
    html += this._renderStat('Avg Duration', duration?.mean?.toFixed(1) || '?');
    html += this._renderStat('Median', duration?.median?.toFixed(1) || '?');
    html += this._renderStat('5th %ile', duration?.percentile_5?.toFixed(1) || '?');
    html += this._renderStat('95th %ile', duration?.percentile_95?.toFixed(1) || '?');
    html += this._renderStat('Total Runs', summary.total_runs);
    html += this._renderStat('Std Dev', duration?.std_dev?.toFixed(1) || '?');
    html += '</div>';

    // Regional control.
    if (summary.regional_control && Object.keys(summary.regional_control).length > 0) {
      html += '<div class="chart-title" style="margin-top: 16px;">Regional Control (final)</div>';
      html += '<div class="chart-container"><canvas id="chart-regional" height="150"></canvas></div>';
    }

    // Time-sliced regional control heatmap (requires snapshots).
    const heatmap = this._buildRegionalHeatmap();
    if (heatmap) {
      html += '<div class="chart-title" style="margin-top: 16px;">Regional Control Over Time</div>';
      html += '<div class="chart-container"><canvas id="chart-heatmap" height="180"></canvas></div>';
    }

    this.mcResultsContainer.innerHTML = html;

    // Draw charts after DOM update.
    requestAnimationFrame(() => {
      this._drawWinProbChart(summary, scenario);
      this._drawDurationChart(summary);
      if (summary.regional_control) {
        this._drawRegionalChart(summary, scenario);
      }
      if (heatmap) {
        this._drawRegionalHeatmap(heatmap, scenario);
      }
    });
  }

  /**
   * Aggregate per-tick regional control across all MC runs.
   *
   * Returns `{ ticks, regions, dominant }` where:
   *   - `ticks` is the sorted list of snapshot ticks
   *   - `regions` is the sorted list of region ids
   *   - `dominant[regionId][tickIdx] = { faction, prob }` is the
   *     plurality faction at that tick and the share of runs holding it
   *
   * Returns `null` if snapshots aren't available (e.g. older runs).
   */
  _buildRegionalHeatmap() {
    const runs = AppState.mcResult?.runs;
    if (!runs || runs.length === 0) return null;
    const haveSnapshots = runs.some((r) => Array.isArray(r.snapshots) && r.snapshots.length > 0);
    if (!haveSnapshots) return null;

    // Discover the union of snapshot ticks and region ids.
    const tickSet = new Set();
    const regionSet = new Set();
    for (const run of runs) {
      for (const snap of run.snapshots || []) {
        tickSet.add(snap.tick);
        if (snap.region_control) {
          for (const rid of Object.keys(snap.region_control)) regionSet.add(rid);
        }
      }
    }
    if (tickSet.size === 0 || regionSet.size === 0) return null;

    const ticks = Array.from(tickSet).sort((a, b) => a - b);
    const regions = Array.from(regionSet).sort();
    const tickIndex = new Map(ticks.map((t, i) => [t, i]));

    // counts[regionId][tickIdx][factionId] = number of runs
    const counts = {};
    for (const rid of regions) {
      counts[rid] = ticks.map(() => ({}));
    }

    for (const run of runs) {
      for (const snap of run.snapshots || []) {
        const ti = tickIndex.get(snap.tick);
        if (ti === undefined) continue;
        for (const [rid, faction] of Object.entries(snap.region_control || {})) {
          if (!counts[rid]) continue;
          const fid = faction == null ? '__neutral__' : faction;
          counts[rid][ti][fid] = (counts[rid][ti][fid] || 0) + 1;
        }
      }
    }

    const totalRuns = runs.length;
    const dominant = {};
    for (const rid of regions) {
      dominant[rid] = counts[rid].map((tickCounts) => {
        let bestFaction = null;
        let bestCount = 0;
        for (const [fid, n] of Object.entries(tickCounts)) {
          if (n > bestCount) {
            bestCount = n;
            bestFaction = fid;
          }
        }
        return { faction: bestFaction, prob: bestCount / totalRuns };
      });
    }

    return { ticks, regions, dominant };
  }

  _renderStat(label, value) {
    return `<div class="mc-stat"><div class="label">${label}</div><div class="value">${value}</div></div>`;
  }

  // -------------------------------------------------------------------
  // Chart Drawing (Canvas 2D)
  // -------------------------------------------------------------------

  _drawWinProbChart(summary, scenario) {
    const canvas = document.getElementById('chart-win-prob');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.parentElement.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = 120 * dpr;
    canvas.style.height = '120px';
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const h = 120;
    const winRates = summary.win_rates || {};
    const entries = Object.entries(winRates).sort((a, b) => b[1] - a[1]);

    if (entries.length === 0) return;

    const barHeight = 22;
    const gap = 6;
    const labelWidth = 120;
    const padding = 8;
    const barAreaWidth = w - labelWidth - padding * 2 - 50;

    entries.forEach(([fid, rate], i) => {
      const y = padding + i * (barHeight + gap);
      const color = this._safeColor(scenario?.factions?.[fid]?.color);
      const name = scenario?.factions?.[fid]?.name || fid;

      // Label.
      ctx.save();
      ctx.font = '500 11px Inter, system-ui, sans-serif';
      ctx.fillStyle = '#e4e4e7';
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      ctx.fillText(name, labelWidth, y + barHeight / 2, labelWidth - 8);
      ctx.restore();

      // Bar background.
      ctx.fillStyle = 'rgba(39, 39, 42, 0.5)';
      ctx.fillRect(labelWidth + padding, y, barAreaWidth, barHeight);

      // Bar fill.
      ctx.fillStyle = color;
      ctx.fillRect(labelWidth + padding, y, barAreaWidth * rate, barHeight);

      // Percentage label.
      ctx.save();
      ctx.font = '500 11px "JetBrains Mono", monospace';
      ctx.fillStyle = '#e4e4e7';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      ctx.fillText(
        `${(rate * 100).toFixed(1)}%`,
        labelWidth + padding + barAreaWidth + 8,
        y + barHeight / 2
      );
      ctx.restore();
    });
  }

  _drawDurationChart(summary) {
    const canvas = document.getElementById('chart-duration');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.parentElement.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = 120 * dpr;
    canvas.style.height = '120px';
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const h = 120;
    const padding = { top: 8, right: 8, bottom: 24, left: 40 };

    // Get duration data from individual runs.
    const runs = AppState.mcResult?.runs;
    if (!runs || runs.length === 0) return;

    const durations = runs.map((r) => r.final_tick);
    const min = Math.min(...durations);
    const max = Math.max(...durations);

    // Create histogram bins.
    const numBins = Math.min(20, Math.max(5, Math.ceil(Math.sqrt(durations.length))));
    const binWidth = max > min ? (max - min) / numBins : 1;
    const bins = new Array(numBins).fill(0);

    for (const d of durations) {
      const binIdx = Math.min(Math.floor((d - min) / binWidth), numBins - 1);
      bins[binIdx]++;
    }

    const maxCount = Math.max(...bins);
    const chartW = w - padding.left - padding.right;
    const chartH = h - padding.top - padding.bottom;
    const barW = chartW / numBins - 2;

    // Draw bars.
    for (let i = 0; i < numBins; i++) {
      const barH = maxCount > 0 ? (bins[i] / maxCount) * chartH : 0;
      const x = padding.left + i * (chartW / numBins) + 1;
      const y = padding.top + chartH - barH;

      ctx.fillStyle = 'rgba(124, 91, 240, 0.6)';
      ctx.fillRect(x, y, barW, barH);
    }

    // X axis labels.
    ctx.save();
    ctx.font = '400 9px "JetBrains Mono", monospace';
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    ctx.fillText(String(min), padding.left, h - padding.bottom + 6);
    ctx.fillText(String(max), w - padding.right, h - padding.bottom + 6);
    ctx.fillText('ticks', w / 2, h - padding.bottom + 6);
    ctx.restore();

    // Y axis label.
    ctx.save();
    ctx.font = '400 9px "JetBrains Mono", monospace';
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(maxCount), padding.left - 6, padding.top);
    ctx.fillText('0', padding.left - 6, padding.top + chartH);
    ctx.restore();
  }

  _drawRegionalChart(summary, scenario) {
    const canvas = document.getElementById('chart-regional');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.parentElement.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = 150 * dpr;
    canvas.style.height = '150px';
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const padding = 8;
    const barHeight = 14;
    const gap = 4;
    const labelWidth = 100;

    const regions = Object.entries(summary.regional_control);
    regions.forEach(([rid, factionProbs], i) => {
      const y = padding + i * (barHeight + gap + 12);
      const regionName = scenario?.map?.regions?.[rid]?.name || rid;

      // Region label.
      ctx.save();
      ctx.font = '400 10px Inter, system-ui, sans-serif';
      ctx.fillStyle = '#a1a1aa';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'bottom';
      ctx.fillText(regionName, padding, y, labelWidth);
      ctx.restore();

      // Stacked bar.
      const barW = w - padding * 2;
      let offsetX = padding;

      for (const [fid, prob] of Object.entries(factionProbs)) {
        const segW = barW * prob;
        if (segW < 1) continue;
        const color = this._safeColor(scenario?.factions?.[fid]?.color);
        ctx.fillStyle = color;
        ctx.fillRect(offsetX, y + 2, segW, barHeight);
        offsetX += segW;
      }
    });
  }

  /**
   * Draw the time-sliced regional control heatmap.
   *
   * Each row is a region, each column is a snapshot tick. The cell is
   * filled with the dominant (plurality) faction's color, alpha-scaled
   * by the share of runs holding that region at that tick.
   */
  _drawRegionalHeatmap(heatmap, scenario) {
    const canvas = document.getElementById('chart-heatmap');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.parentElement.getBoundingClientRect();
    const h = 180;
    canvas.width = rect.width * dpr;
    canvas.height = h * dpr;
    canvas.style.height = `${h}px`;
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const padding = { top: 8, right: 8, bottom: 22, left: 100 };
    const { ticks, regions, dominant } = heatmap;

    if (ticks.length === 0 || regions.length === 0) return;

    const chartW = w - padding.left - padding.right;
    const chartH = h - padding.top - padding.bottom;
    const rowH = Math.max(8, Math.floor(chartH / regions.length));
    const colW = chartW / ticks.length;

    // Background.
    ctx.fillStyle = 'rgba(39, 39, 42, 0.4)';
    ctx.fillRect(padding.left, padding.top, chartW, rowH * regions.length);

    regions.forEach((rid, rowIdx) => {
      const y = padding.top + rowIdx * rowH;

      // Region label.
      ctx.save();
      ctx.font = '400 10px Inter, system-ui, sans-serif';
      ctx.fillStyle = '#a1a1aa';
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      const regionName = scenario?.map?.regions?.[rid]?.name || rid;
      ctx.fillText(regionName, padding.left - 6, y + rowH / 2, padding.left - 12);
      ctx.restore();

      dominant[rid].forEach((cell, colIdx) => {
        const x = padding.left + colIdx * colW;
        if (!cell.faction) return;
        const baseColor =
          cell.faction === '__neutral__'
            ? '#52525b'
            : this._safeColor(scenario?.factions?.[cell.faction]?.color);
        ctx.fillStyle = this._withAlpha(baseColor, Math.max(0.15, cell.prob));
        ctx.fillRect(x, y, Math.ceil(colW), rowH - 1);
      });
    });

    // X axis ticks (first / middle / last).
    ctx.save();
    ctx.font = '400 9px "JetBrains Mono", monospace';
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    const labelY = padding.top + rowH * regions.length + 4;
    const positions = [0, Math.floor(ticks.length / 2), ticks.length - 1];
    for (const i of positions) {
      const x = padding.left + (i + 0.5) * colW;
      ctx.fillText(`T${ticks[i]}`, x, labelY);
    }
    ctx.restore();
  }

  // -------------------------------------------------------------------
  // Sensitivity Analysis
  // -------------------------------------------------------------------

  _runSensitivity() {
    if (!AppState.toml) return;
    const param = this.sensParam.value;
    const low = parseFloat(this.sensLow.value);
    const high = parseFloat(this.sensHigh.value);
    const steps = parseInt(this.sensSteps.value, 10);
    const runs = parseInt(this.sensRuns.value, 10);

    if (!Number.isFinite(low) || !Number.isFinite(high) || low > high) {
      this.sensResultsContainer.innerHTML =
        '<div class="validation-msg error">Low must be ≤ high</div>';
      return;
    }
    if (!Number.isInteger(steps) || steps < 2) {
      this.sensResultsContainer.innerHTML =
        '<div class="validation-msg error">Steps must be ≥ 2</div>';
      return;
    }

    this.btnSensRun.disabled = true;
    this.btnSensRun.innerHTML = '<span class="spinner"></span> Running...';
    this.sensResultsContainer.innerHTML =
      '<div style="font-size: 0.8125rem; color: var(--text-muted); padding: 8px 0;">Running sweep...</div>';

    // Yield to UI before the (still synchronous) WASM call. Sensitivity
    // is many small MC batches, but each batch lives on the main thread
    // for now — the worker plumbing for this lives in run_monte_carlo.
    setTimeout(() => {
      try {
        const raw = this.wasm.run_sensitivity_wasm(
          AppState.toml,
          param,
          low,
          high,
          steps,
          runs,
          undefined,
        );
        const result = mapsToObjects(raw);
        AppState.sensResult = result;
        this._renderSensitivityResults(result);
      } catch (e) {
        this.sensResultsContainer.innerHTML =
          `<div class="validation-msg error">${this._esc(String(e))}</div>`;
      } finally {
        this.btnSensRun.disabled = false;
        this.btnSensRun.textContent = 'Run Sweep';
      }
    }, 50);
  }

  _renderSensitivityResults(result) {
    const scenario = AppState.scenario;
    const { parameter, baseline_value, varied_values, outcomes } = result;

    let html = '';
    html += `<div class="chart-title" style="margin-top: 8px;">Tornado: ${this._esc(parameter)}</div>`;
    html += `<div style="font-size: 0.75rem; color: var(--text-muted); margin-bottom: 4px;">baseline = ${baseline_value.toFixed(3)}</div>`;
    html += '<div class="chart-container"><canvas id="chart-tornado" height="180"></canvas></div>';

    // Per-step duration table.
    html += '<div class="chart-title" style="margin-top: 12px;">Per-Step Avg Duration</div>';
    html += '<div class="mc-summary-stats">';
    for (let i = 0; i < varied_values.length; i++) {
      const val = varied_values[i];
      const dur = outcomes[i]?.average_duration ?? 0;
      html += this._renderStat(val.toFixed(3), dur.toFixed(1));
    }
    html += '</div>';

    this.sensResultsContainer.innerHTML = html;

    requestAnimationFrame(() => this._drawTornadoChart(result, scenario));
  }

  /**
   * Draw a tornado chart of per-faction win-probability swings across
   * the parameter sweep.
   *
   * For each faction we plot the [min, max] win-rate range observed
   * across the sweep, anchored at the midpoint. Wider bars = more
   * sensitive to the parameter. Bars are sorted by descending range so
   * the most sensitive factions appear at the top, matching the
   * conventional tornado chart layout.
   */
  _drawTornadoChart(result, scenario) {
    const canvas = document.getElementById('chart-tornado');
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.parentElement.getBoundingClientRect();
    const h = 180;
    canvas.width = rect.width * dpr;
    canvas.height = h * dpr;
    canvas.style.height = `${h}px`;
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const padding = { top: 8, right: 30, bottom: 22, left: 110 };

    // Collect per-faction win-rate ranges across the sweep.
    const factions = new Set();
    for (const summary of result.outcomes) {
      for (const fid of Object.keys(summary.win_rates || {})) factions.add(fid);
    }
    const ranges = [];
    for (const fid of factions) {
      let min = Infinity;
      let max = -Infinity;
      for (const summary of result.outcomes) {
        const r = summary.win_rates?.[fid] ?? 0;
        if (r < min) min = r;
        if (r > max) max = r;
      }
      if (min === Infinity) continue;
      ranges.push({ fid, min, max, range: max - min });
    }
    ranges.sort((a, b) => b.range - a.range);

    if (ranges.length === 0) {
      ctx.font = '400 11px Inter, system-ui, sans-serif';
      ctx.fillStyle = '#71717a';
      ctx.fillText('No win-rate variation observed.', padding.left, h / 2);
      return;
    }

    // X axis: 0..1 win rate. Center reference line at the median win
    // rate across all (faction, step) pairs to anchor the tornado.
    const allRates = [];
    for (const r of ranges) {
      allRates.push(r.min, r.max);
    }
    allRates.sort((a, b) => a - b);
    const center = allRates[Math.floor(allRates.length / 2)];

    const chartW = w - padding.left - padding.right;
    const chartH = h - padding.top - padding.bottom;
    const rowH = Math.max(14, Math.floor(chartH / ranges.length) - 4);
    const xScale = (rate) => padding.left + rate * chartW;

    // Center axis.
    ctx.strokeStyle = 'rgba(161, 161, 170, 0.4)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(xScale(center), padding.top);
    ctx.lineTo(xScale(center), padding.top + chartH);
    ctx.stroke();

    ranges.forEach((r, i) => {
      const y = padding.top + i * (rowH + 4);
      const color = this._safeColor(scenario?.factions?.[r.fid]?.color);
      const name = scenario?.factions?.[r.fid]?.name || r.fid;

      // Faction label.
      ctx.save();
      ctx.font = '500 11px Inter, system-ui, sans-serif';
      ctx.fillStyle = '#e4e4e7';
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      ctx.fillText(name, padding.left - 8, y + rowH / 2, padding.left - 16);
      ctx.restore();

      // Range bar.
      const x0 = xScale(r.min);
      const x1 = xScale(r.max);
      ctx.fillStyle = this._withAlpha(color, 0.75);
      ctx.fillRect(x0, y, Math.max(2, x1 - x0), rowH);

      // Numeric range label.
      ctx.save();
      ctx.font = '400 9px "JetBrains Mono", monospace';
      ctx.fillStyle = '#a1a1aa';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      ctx.fillText(
        `${(r.min * 100).toFixed(0)}–${(r.max * 100).toFixed(0)}%`,
        x1 + 4,
        y + rowH / 2,
      );
      ctx.restore();
    });

    // X axis labels (0%, center%, 100%).
    ctx.save();
    ctx.font = '400 9px "JetBrains Mono", monospace';
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';
    const labelY = padding.top + chartH + 4;
    ctx.fillText('0%', xScale(0), labelY);
    ctx.fillText(`${(center * 100).toFixed(0)}%`, xScale(center), labelY);
    ctx.fillText('100%', xScale(1), labelY);
    ctx.restore();
  }

  /** Convert a `#rrggbb` color to `rgba(r,g,b,a)` for alpha blending. */
  _withAlpha(hex, alpha) {
    const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
    if (!m) return hex;
    const r = parseInt(m[1], 16);
    const g = parseInt(m[2], 16);
    const b = parseInt(m[3], 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(3)})`;
  }

  _esc(str) {
    const div = document.createElement('div');
    div.textContent = str;
    // Also escape double-quotes so the output is safe in HTML attribute
    // contexts (matches faction-builder.js _esc). Current callers all use
    // it in text-node positions, but this is defense-in-depth if future
    // callers interpolate into `value="..."` or similar.
    return div.innerHTML.replace(/"/g, '&quot;');
  }

  /** Sanitize a color value for safe use in inline styles. */
  _safeColor(color) {
    if (typeof color === 'string' && /^#[0-9a-fA-F]{6}$/.test(color)) return color;
    return '#7c5bf0';
  }
}
