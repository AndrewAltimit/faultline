/**
 * Results dashboard: event log, state inspector, Monte Carlo charts.
 * All charts rendered with Canvas 2D (no external dependencies).
 */
import { AppState } from './state.js';
import { mapsToObjects } from './wasm-util.js';
import { buildRegionalHeatmap, buildTornadoRanges } from './heatmap-data.js';
import { PinnedStore } from './pinned.js';
import { renderComparison } from './comparison.js';

function escapeHtml(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function fmtCell(value, confidence) {
  const v = typeof value === 'number' ? value.toFixed(2) : '?';
  const tag = confidence === 'High' ? 'H' : confidence === 'Medium' ? 'M' : 'L';
  return `${v} <span class="conf-${tag}">[${tag}]</span>`;
}

function fmtCostRatio(v) {
  if (!isFinite(v) || v <= 0) return '—';
  if (v >= 1000) return `${(v / 1000).toFixed(1)}k×`;
  return `${v.toFixed(0)}×`;
}

function fmtMoney(v) {
  if (v == null || !isFinite(v)) return '?';
  if (v >= 1e9) return `${(v / 1e9).toFixed(1)}B`;
  if (v >= 1e6) return `${(v / 1e6).toFixed(1)}M`;
  if (v >= 1e3) return `${(v / 1e3).toFixed(1)}k`;
  return v.toFixed(0);
}

function orderPhasesByChain(chain) {
  if (!chain || !chain.phases) return [];
  const seen = new Set();
  const order = [];
  const visit = (pid) => {
    if (seen.has(pid) || !chain.phases[pid]) return;
    seen.add(pid);
    order.push(pid);
    const branches = chain.phases[pid].branches || [];
    for (const b of branches) visit(b.next_phase);
  };
  visit(chain.entry_phase);
  // Append any phases not reachable by branches in declared order.
  for (const pid of Object.keys(chain.phases)) {
    if (!seen.has(pid)) order.push(pid);
  }
  return order;
}

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

    // Pinned results store + container.
    this.pinned = new PinnedStore();
    this.pinnedContainer = document.getElementById('pinned-results');
    this.btnPinResult = document.getElementById('btn-pin-result');
    this.comparisonContainer = document.getElementById('comparison-results');
    this._compareSelection = { a: null, b: null };
    if (this.btnPinResult) {
      this.btnPinResult.addEventListener('click', () => this._pinCurrent());
    }
    this.pinned.subscribe(() => this._renderPinned());

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
    this._setPinButtonEnabled(false);
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
        this._setPinButtonEnabled(true);
        this.bus.emit('mc:complete', result);
      })
      .catch((e) => {
        this.mcResultsContainer.innerHTML =
          `<div class="validation-msg error">${this._esc(String(e))}</div>`;
        this._setPinButtonEnabled(false);
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
    const heatmap = buildRegionalHeatmap(AppState.mcResult?.runs);
    if (heatmap) {
      html += '<div class="chart-title" style="margin-top: 16px;">Regional Control Over Time</div>';
      html += '<div class="chart-container"><canvas id="chart-heatmap" height="180"></canvas></div>';
    }

    // Campaign / feasibility / seam panels.
    html += this._renderFeasibilityMatrix(summary);
    html += this._renderCampaignPanels(summary, scenario);
    html += this._renderSeamPanel(summary);

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


  _renderStat(label, value) {
    return `<div class="mc-stat"><div class="label">${label}</div><div class="value">${value}</div></div>`;
  }

  // -------------------------------------------------------------------
  // Campaign / feasibility / seam panels
  // -------------------------------------------------------------------

  _renderFeasibilityMatrix(summary) {
    const rows = summary.feasibility_matrix || [];
    if (!rows.length) return '';

    let h = '<div class="chart-title" style="margin-top: 16px;">Feasibility Matrix</div>';
    h += '<div class="feasibility-table-wrap">';
    h += '<table class="feasibility-table"><thead><tr>';
    h += '<th>Chain</th><th title="Average phase base success probability">Tech</th>';
    h += '<th title="Operational complexity">Complex</th><th title="Probability defender detects">Detect</th>';
    h += '<th title="End-to-end success rate">Success</th><th title="Damage + institutional erosion">Severity</th>';
    h += '<th title="1 - mean attribution confidence">Attrib</th><th title="Defender $ / Attacker $">Cost ×</th>';
    h += '</tr></thead><tbody>';
    for (const r of rows) {
      h += `<tr>
        <td class="chain-name">${escapeHtml(r.chain_name)}</td>
        <td>${fmtCell(r.technology_readiness, r.confidence?.technology_readiness)}</td>
        <td>${fmtCell(r.operational_complexity, r.confidence?.operational_complexity)}</td>
        <td>${fmtCell(r.detection_probability, r.confidence?.detection_probability)}</td>
        <td>${fmtCell(r.success_probability, r.confidence?.success_probability)}</td>
        <td>${fmtCell(r.consequence_severity, r.confidence?.consequence_severity)}</td>
        <td>${r.attribution_difficulty.toFixed(2)}</td>
        <td class="cost-asym">${fmtCostRatio(r.cost_asymmetry_ratio)}</td>
      </tr>`;
    }
    h += '</tbody></table></div>';
    h += '<div class="chart-subtitle">Confidence: <span class="conf-H">H</span> high · <span class="conf-M">M</span> medium · <span class="conf-L">L</span> low (MC variance)</div>';
    return h;
  }

  _renderCampaignPanels(summary, scenario) {
    const cs = summary.campaign_summaries || {};
    const chainIds = Object.keys(cs);
    if (!chainIds.length) return '';

    let h = '<div class="chart-title" style="margin-top: 16px;">Kill Chain Phase Breakdown</div>';
    for (const cid of chainIds) {
      const c = cs[cid];
      const chain = scenario?.kill_chains?.[cid];
      const name = chain?.name || cid;
      h += `<div class="campaign-panel">
        <div class="campaign-header">
          <div class="campaign-name">${escapeHtml(name)}</div>
          <div class="campaign-metrics">
            <span>Success <b>${(c.overall_success_rate * 100).toFixed(1)}%</b></span>
            <span>Detection <b>${(c.detection_rate * 100).toFixed(1)}%</b></span>
            <span>Attribution conf <b>${c.mean_attribution_confidence.toFixed(2)}</b></span>
          </div>
        </div>
        <div class="campaign-cost-row">
          <div><span class="label">Attacker spend</span><b>$${fmtMoney(c.mean_attacker_spend)}</b></div>
          <div><span class="label">Defender spend</span><b>$${fmtMoney(c.mean_defender_spend)}</b></div>
          <div class="cost-ratio"><span class="label">Asymmetry</span><b>${fmtCostRatio(c.cost_asymmetry_ratio)}</b></div>
        </div>
        ${this._renderPhaseFlow(c.phase_stats, chain)}
      </div>`;
    }
    return h;
  }

  _renderPhaseFlow(phaseStats, chain) {
    if (!phaseStats) return '';
    // Order phases: use chain.entry_phase + DFS via branches if we have chain,
    // otherwise fall back to declared key order.
    let ordered;
    if (chain && chain.entry_phase) {
      ordered = orderPhasesByChain(chain);
    } else {
      ordered = Object.keys(phaseStats);
    }
    let h = '<div class="phase-flow">';
    ordered.forEach((pid, idx) => {
      const ps = phaseStats[pid];
      if (!ps) return;
      const succ = ps.success_rate;
      const det = ps.detection_rate;
      const nr = ps.not_reached_rate;
      const label = chain?.phases?.[pid]?.name || pid;
      const barHue = 120 * succ; // red → green
      h += `<div class="phase-node">
        <div class="phase-label" title="${escapeHtml(pid)}">${escapeHtml(label)}</div>
        <div class="phase-bars">
          <div class="phase-bar-fill" style="width:${(succ * 100).toFixed(0)}%;background:hsl(${barHue},70%,45%)"></div>
        </div>
        <div class="phase-stats">
          <span class="stat-succ">${(succ * 100).toFixed(0)}%</span>
          <span class="stat-det">det ${(det * 100).toFixed(0)}%</span>
          <span class="stat-nr">nr ${(nr * 100).toFixed(0)}%</span>
        </div>
      </div>`;
      if (idx < ordered.length - 1) {
        h += '<div class="phase-arrow">→</div>';
      }
    });
    h += '</div>';
    return h;
  }

  _renderSeamPanel(summary) {
    const seams = summary.seam_scores || {};
    const ids = Object.keys(seams);
    if (!ids.length) return '';

    let h = '<div class="chart-title" style="margin-top: 16px;">Doctrinal Seam Analysis</div>';
    h += '<table class="feasibility-table"><thead><tr><th>Chain</th><th>Cross-domain phases</th><th>Mean domains/phase</th><th>Seam share of successes</th></tr></thead><tbody>';
    for (const cid of ids) {
      const s = seams[cid];
      h += `<tr>
        <td class="chain-name">${escapeHtml(cid)}</td>
        <td>${s.cross_domain_phase_count}</td>
        <td>${s.mean_domains_per_phase.toFixed(2)}</td>
        <td>${(s.seam_exploitation_share * 100).toFixed(1)}%</td>
      </tr>`;
    }
    h += '</tbody></table>';

    // Domain frequency chips.
    for (const cid of ids) {
      const s = seams[cid];
      const freqs = Object.entries(s.domain_frequency || {});
      if (!freqs.length) continue;
      h += `<div class="chart-subtitle"><b>${escapeHtml(cid)}</b> domain frequency: `;
      h += freqs.map(([d, n]) => `<span class="domain-chip">${escapeHtml(d)} ×${n}</span>`).join(' ');
      h += '</div>';
    }
    return h;
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

    const ranges = buildTornadoRanges(result);

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

  // -------------------------------------------------------------------
  // Pinned results + side-by-side comparison
  // -------------------------------------------------------------------

  _setPinButtonEnabled(enabled) {
    if (!this.btnPinResult) return;
    this.btnPinResult.disabled = !enabled;
  }

  _pinCurrent() {
    const summary = AppState.mcResult?.summary;
    if (!summary) return;
    const scenarioName = AppState.scenario?.meta?.name || 'scenario';
    this.pinned.add({
      scenarioName,
      toml: AppState.toml || '',
      summary,
    });
  }

  _renderPinned() {
    if (!this.pinnedContainer) return;
    const pins = this.pinned.list();
    if (!pins.length) {
      this.pinnedContainer.innerHTML =
        '<div class="empty-state" style="padding: 8px 0;"><span style="font-size: 0.75rem;">Pin a Monte Carlo result above to compare scenarios.</span></div>';
      this._renderComparisonPanel();
      return;
    }

    let html = '<div class="pinned-list">';
    for (const p of pins) {
      const winRates = p.summary?.win_rates || {};
      const top = Object.entries(winRates).sort((a, b) => b[1] - a[1]).slice(0, 2);
      const winSummary = top.length
        ? top.map(([fid, rate]) => `<span class="pin-win">${this._esc(fid)} ${(rate * 100).toFixed(0)}%</span>`).join(' · ')
        : '<span class="pin-win-none">no win rates</span>';
      const aSel = this._compareSelection.a === p.id ? ' pin-sel-a' : '';
      const bSel = this._compareSelection.b === p.id ? ' pin-sel-b' : '';
      const ageMin = Math.max(1, Math.round((Date.now() - p.capturedAt) / 60000));
      html += `
        <div class="pinned-card${aSel}${bSel}" data-pin="${this._esc(p.id)}">
          <div class="pinned-card-head">
            <div class="pinned-label" title="${this._esc(p.scenarioName)}">${this._esc(p.label)}</div>
            <button class="pin-x" data-action="remove" title="Remove pin">×</button>
          </div>
          <div class="pinned-meta">
            <span>${p.summary?.total_runs ?? '?'} runs</span>
            <span>${ageMin}m ago</span>
          </div>
          <div class="pinned-winrates">${winSummary}</div>
          <div class="pinned-actions">
            <button class="pin-btn" data-action="select-a">Set A</button>
            <button class="pin-btn" data-action="select-b">Set B</button>
            <button class="pin-btn" data-action="load">Load TOML</button>
          </div>
        </div>`;
    }
    html += '</div>';
    html += '<div class="pinned-compare-row">';
    html += `<button class="btn-label" id="btn-compare-pins" ${this._canCompare() ? '' : 'disabled'}>Compare A vs B</button>`;
    html += '<button class="btn-label" id="btn-clear-compare">Clear selection</button>';
    html += '</div>';
    this.pinnedContainer.innerHTML = html;

    // Bind per-card actions.
    this.pinnedContainer.querySelectorAll('.pinned-card').forEach((card) => {
      const id = card.dataset.pin;
      card.querySelectorAll('button[data-action]').forEach((btn) => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          const action = btn.dataset.action;
          if (action === 'remove') {
            this.pinned.remove(id);
            if (this._compareSelection.a === id) this._compareSelection.a = null;
            if (this._compareSelection.b === id) this._compareSelection.b = null;
          } else if (action === 'select-a') {
            this._compareSelection.a = id;
            if (this._compareSelection.b === id) this._compareSelection.b = null;
            this._renderPinned();
          } else if (action === 'select-b') {
            this._compareSelection.b = id;
            if (this._compareSelection.a === id) this._compareSelection.a = null;
            this._renderPinned();
          } else if (action === 'load') {
            this._loadPinIntoEditor(id);
          }
        });
      });
    });

    const btnCompare = this.pinnedContainer.querySelector('#btn-compare-pins');
    if (btnCompare) {
      btnCompare.addEventListener('click', () => this._renderComparisonPanel());
    }
    const btnClear = this.pinnedContainer.querySelector('#btn-clear-compare');
    if (btnClear) {
      btnClear.addEventListener('click', () => {
        this._compareSelection = { a: null, b: null };
        this._renderPinned();
        this._renderComparisonPanel();
      });
    }

    this._renderComparisonPanel();
  }

  _canCompare() {
    return !!(this._compareSelection.a && this._compareSelection.b);
  }

  _renderComparisonPanel() {
    if (!this.comparisonContainer) return;
    if (!this._canCompare()) {
      this.comparisonContainer.innerHTML = '';
      return;
    }
    const a = this.pinned.get(this._compareSelection.a);
    const b = this.pinned.get(this._compareSelection.b);
    if (!a || !b) {
      this.comparisonContainer.innerHTML = '';
      return;
    }
    this.comparisonContainer.innerHTML = renderComparison(
      { label: a.label, summary: a.summary },
      { label: b.label, summary: b.summary },
    );
  }

  _loadPinIntoEditor(id) {
    const p = this.pinned.get(id);
    if (!p || !p.toml) return;
    this.bus.emit('editor:load-toml', { toml: p.toml, source: `pin:${p.label}` });
  }
}
