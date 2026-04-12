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

    bus.on('sim:tick', (snapshot) => this._onTick(snapshot));
    bus.on('sim:snapshot', (snapshot) => this._onSnapshot(snapshot));
    bus.on('sim:finished', (outcome) => this._onFinished(outcome));
    bus.on('sim:reset', () => this._onReset());
    bus.on('scenario:loaded', () => this._onScenarioLoaded());
  }

  _onScenarioLoaded() {
    this.btnMcRun.disabled = false;
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

    // Use setTimeout to let the UI update before blocking.
    setTimeout(() => {
      try {
        const result = mapsToObjects(this.wasm.run_monte_carlo(AppState.toml, numRuns));
        AppState.mcResult = result;
        this._renderMcResults(result.summary);
        this.bus.emit('mc:complete', result);
      } catch (e) {
        this.mcResultsContainer.innerHTML =
          `<div class="validation-msg error">${this._esc(String(e))}</div>`;
      } finally {
        this.btnMcRun.disabled = false;
        this.btnMcRun.textContent = 'Run MC';
      }
    }, 50);
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
      html += '<div class="chart-title" style="margin-top: 16px;">Regional Control</div>';
      html += '<div class="chart-container"><canvas id="chart-regional" height="150"></canvas></div>';
    }

    this.mcResultsContainer.innerHTML = html;

    // Draw charts after DOM update.
    requestAnimationFrame(() => {
      this._drawWinProbChart(summary, scenario);
      this._drawDurationChart(summary);
      if (summary.regional_control) {
        this._drawRegionalChart(summary, scenario);
      }
    });
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

  _esc(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
  }

  /** Sanitize a color value for safe use in inline styles. */
  _safeColor(color) {
    if (typeof color === 'string' && /^#[0-9a-fA-F]{6}$/.test(color)) return color;
    return '#7c5bf0';
  }
}
