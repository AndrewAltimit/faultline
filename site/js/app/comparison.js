/**
 * Side-by-side Monte Carlo comparison.
 *
 * Mirrors the delta semantics of `faultline_stats::counterfactual::ComparisonReport`
 * so that what the dashboard shows lines up with what `--counterfactual` /
 * `--compare` emit on the CLI:
 *
 *   - Win-rate deltas are `variant - baseline`. Factions present on only
 *     one side appear with the missing side treated as 0.
 *   - Chain deltas pair up by chain id; missing entries are treated as 0
 *     to surface the asymmetry rather than silently drop the row.
 *   - All deltas are point estimates. Wilson CIs from each side are
 *     shown for context (overlap = "the difference may be sampling noise").
 *
 * One UX deviation from the CLI report: results pinned at different
 * times can have different `total_runs` and different scenario seeds, so
 * we surface a banner when sample sizes diverge — the analyst should
 * know that a "delta" between unequal-sample pins mixes scenario and
 * sampling variance.
 */

function escapeHtml(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function fmtPct(p) {
  if (p == null || !isFinite(p)) return '—';
  return `${(p * 100).toFixed(1)}%`;
}

function fmtSignedPct(d) {
  if (d == null || !isFinite(d)) return '—';
  const pct = d * 100;
  const sign = pct > 0 ? '+' : '';
  return `${sign}${pct.toFixed(1)}pp`;
}

function fmtSignedFloat(d, digits = 2) {
  if (d == null || !isFinite(d)) return '—';
  const sign = d > 0 ? '+' : '';
  return `${sign}${d.toFixed(digits)}`;
}

function fmtCi(ci) {
  if (!ci || !Array.isArray(ci) || ci.length !== 2) return '';
  const [lo, hi] = ci;
  return `[${(lo * 100).toFixed(1)}–${(hi * 100).toFixed(1)}%]`;
}

function deltaClass(d) {
  if (d == null || !isFinite(d) || Math.abs(d) < 1e-9) return 'delta-zero';
  return d > 0 ? 'delta-pos' : 'delta-neg';
}

/**
 * Compute the same delta shape that `compute_delta` in
 * `crates/faultline-stats/src/counterfactual.rs` produces.
 *
 * @param {object} baseline MonteCarloSummary
 * @param {object} variant  MonteCarloSummary
 */
export function computeDelta(baseline, variant) {
  const winRateDeltas = {};
  const factionIds = new Set([
    ...Object.keys(baseline?.win_rates || {}),
    ...Object.keys(variant?.win_rates || {}),
  ]);
  for (const fid of factionIds) {
    const b = baseline?.win_rates?.[fid] ?? 0;
    const v = variant?.win_rates?.[fid] ?? 0;
    winRateDeltas[fid] = v - b;
  }

  const chainDeltas = {};
  const chainIds = new Set([
    ...Object.keys(baseline?.campaign_summaries || {}),
    ...Object.keys(variant?.campaign_summaries || {}),
  ]);
  for (const cid of chainIds) {
    const b = baseline?.campaign_summaries?.[cid];
    const v = variant?.campaign_summaries?.[cid];
    const bs = b?.overall_success_rate ?? 0;
    const vs = v?.overall_success_rate ?? 0;
    const bd = b?.detection_rate ?? 0;
    const vd = v?.detection_rate ?? 0;
    const br = b?.cost_asymmetry_ratio ?? 0;
    const vr = v?.cost_asymmetry_ratio ?? 0;
    const ba = b?.mean_attacker_spend ?? 0;
    const va = v?.mean_attacker_spend ?? 0;
    const bdef = b?.mean_defender_spend ?? 0;
    const vdef = v?.mean_defender_spend ?? 0;
    chainDeltas[cid] = {
      overall_success_rate_delta: vs - bs,
      detection_rate_delta: vd - bd,
      cost_asymmetry_ratio_delta: vr - br,
      attacker_spend_delta: va - ba,
      defender_spend_delta: vdef - bdef,
    };
  }

  return {
    mean_duration_delta:
      (variant?.average_duration ?? 0) - (baseline?.average_duration ?? 0),
    win_rate_deltas: winRateDeltas,
    chain_deltas: chainDeltas,
  };
}

function renderHeaderCell(label, sub) {
  return `<th>
    <div class="cmp-col-label">${escapeHtml(label)}</div>
    ${sub ? `<div class="cmp-col-sub">${escapeHtml(sub)}</div>` : ''}
  </th>`;
}

function renderWinRateRow(fid, baseline, variant, delta) {
  const b = baseline?.win_rates?.[fid] ?? 0;
  const v = variant?.win_rates?.[fid] ?? 0;
  const bCi = baseline?.win_rate_cis?.[fid];
  const vCi = variant?.win_rate_cis?.[fid];
  return `<tr>
    <td class="cmp-row-label">${escapeHtml(fid)}</td>
    <td>
      <div class="cmp-val">${fmtPct(b)}</div>
      ${bCi ? `<div class="cmp-ci">${fmtCi(bCi)}</div>` : ''}
    </td>
    <td>
      <div class="cmp-val">${fmtPct(v)}</div>
      ${vCi ? `<div class="cmp-ci">${fmtCi(vCi)}</div>` : ''}
    </td>
    <td class="${deltaClass(delta)}"><b>${fmtSignedPct(delta)}</b></td>
  </tr>`;
}

function renderChainRow(cid, baseline, variant, chainDelta) {
  const b = baseline?.campaign_summaries?.[cid];
  const v = variant?.campaign_summaries?.[cid];
  return `<tr>
    <td class="cmp-row-label">${escapeHtml(cid)}</td>
    <td>
      <div class="cmp-val">${fmtPct(b?.overall_success_rate)}</div>
      <div class="cmp-ci">det ${fmtPct(b?.detection_rate)}</div>
    </td>
    <td>
      <div class="cmp-val">${fmtPct(v?.overall_success_rate)}</div>
      <div class="cmp-ci">det ${fmtPct(v?.detection_rate)}</div>
    </td>
    <td class="${deltaClass(chainDelta?.overall_success_rate_delta)}">
      <b>${fmtSignedPct(chainDelta?.overall_success_rate_delta)}</b>
      <div class="cmp-ci">det ${fmtSignedPct(chainDelta?.detection_rate_delta)}</div>
    </td>
  </tr>`;
}

/**
 * Render the side-by-side comparison HTML.
 *
 * @param {{label: string, summary: object}} baseline
 * @param {{label: string, summary: object}} variant
 */
export function renderComparison(baseline, variant) {
  const delta = computeDelta(baseline.summary, variant.summary);

  const sampleMismatch = baseline.summary?.total_runs !== variant.summary?.total_runs;

  const factionIds = Array.from(
    new Set([
      ...Object.keys(baseline.summary?.win_rates || {}),
      ...Object.keys(variant.summary?.win_rates || {}),
    ]),
  ).sort();

  const chainIds = Array.from(
    new Set([
      ...Object.keys(baseline.summary?.campaign_summaries || {}),
      ...Object.keys(variant.summary?.campaign_summaries || {}),
    ]),
  ).sort();

  let html = '<div class="cmp-panel">';
  html += '<div class="cmp-header">';
  html += `<div class="cmp-header-title">${escapeHtml(baseline.label)} <span class="cmp-vs">vs</span> ${escapeHtml(variant.label)}</div>`;
  html += '</div>';

  if (sampleMismatch) {
    html += `<div class="cmp-warn">Sample sizes differ: baseline ${baseline.summary?.total_runs ?? '?'} runs, variant ${variant.summary?.total_runs ?? '?'} runs. Deltas mix scenario and sampling variance.</div>`;
  }

  // Headline metrics row.
  html += '<div class="cmp-headline">';
  html += `<div class="cmp-stat"><div class="label">Mean duration</div>
    <div class="value">${(baseline.summary?.average_duration ?? 0).toFixed(1)} → ${(variant.summary?.average_duration ?? 0).toFixed(1)}</div>
    <div class="${deltaClass(delta.mean_duration_delta)}">${fmtSignedFloat(delta.mean_duration_delta, 1)} ticks</div>
  </div>`;
  html += `<div class="cmp-stat"><div class="label">Total runs</div>
    <div class="value">${baseline.summary?.total_runs ?? '?'} / ${variant.summary?.total_runs ?? '?'}</div>
  </div>`;
  html += '</div>';

  // Win rates table.
  if (factionIds.length) {
    html += '<div class="chart-title" style="margin-top: 12px;">Win Rates</div>';
    html += '<table class="cmp-table"><thead><tr>';
    html += '<th></th>';
    html += renderHeaderCell(baseline.label, '95% Wilson CI');
    html += renderHeaderCell(variant.label, '95% Wilson CI');
    html += renderHeaderCell('Δ', 'variant − baseline');
    html += '</tr></thead><tbody>';
    for (const fid of factionIds) {
      html += renderWinRateRow(fid, baseline.summary, variant.summary, delta.win_rate_deltas[fid]);
    }
    html += '</tbody></table>';
  }

  // Per-chain feasibility deltas.
  if (chainIds.length) {
    html += '<div class="chart-title" style="margin-top: 12px;">Kill Chain Feasibility</div>';
    html += '<table class="cmp-table"><thead><tr>';
    html += '<th>Chain</th>';
    html += renderHeaderCell(baseline.label, 'success / detection');
    html += renderHeaderCell(variant.label, 'success / detection');
    html += renderHeaderCell('Δ', 'success / detection');
    html += '</tr></thead><tbody>';
    for (const cid of chainIds) {
      html += renderChainRow(cid, baseline.summary, variant.summary, delta.chain_deltas[cid]);
    }
    html += '</tbody></table>';
  }

  // Cost-asymmetry deltas — separate table for legibility.
  if (chainIds.length) {
    html += '<div class="chart-title" style="margin-top: 12px;">Cost Asymmetry (defender $ / attacker $)</div>';
    html += '<table class="cmp-table"><thead><tr>';
    html += '<th>Chain</th>';
    html += renderHeaderCell(baseline.label, '');
    html += renderHeaderCell(variant.label, '');
    html += renderHeaderCell('Δ', '');
    html += '</tr></thead><tbody>';
    for (const cid of chainIds) {
      const b = baseline.summary?.campaign_summaries?.[cid];
      const v = variant.summary?.campaign_summaries?.[cid];
      const d = delta.chain_deltas[cid]?.cost_asymmetry_ratio_delta ?? 0;
      const fmtRatio = (r) => (r == null || !isFinite(r) || r <= 0 ? '—' : `${r.toFixed(0)}×`);
      html += `<tr>
        <td class="cmp-row-label">${escapeHtml(cid)}</td>
        <td><div class="cmp-val">${fmtRatio(b?.cost_asymmetry_ratio)}</div></td>
        <td><div class="cmp-val">${fmtRatio(v?.cost_asymmetry_ratio)}</div></td>
        <td class="${deltaClass(d)}"><b>${fmtSignedFloat(d, 0)}×</b></td>
      </tr>`;
    }
    html += '</tbody></table>';
  }

  html += '</div>';
  return html;
}
