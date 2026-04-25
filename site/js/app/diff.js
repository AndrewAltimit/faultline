/**
 * Minimal LCS-based unified diff for plain-text scenarios.
 *
 * No external dependencies — Faultline's frontend is vanilla ES modules
 * and pulling in a diff library for a single editor feature isn't worth
 * the bytes.
 *
 * The algorithm here is the textbook O(n*m) LCS table walk. TOML
 * scenarios are hundreds of lines, not thousands, so this is plenty
 * fast on the main thread; if a scenario ever grows past ~5k lines we
 * should switch to Myers and run it off the main thread, but that's
 * speculative.
 */

/**
 * Compute the LCS table between two arrays of strings.
 * Returns the (n+1)*(m+1) table of common-subsequence lengths.
 */
function lcsTable(a, b) {
  const n = a.length;
  const m = b.length;
  const dp = new Array(n + 1);
  for (let i = 0; i <= n; i++) {
    dp[i] = new Int32Array(m + 1);
  }
  for (let i = 1; i <= n; i++) {
    const ai = a[i - 1];
    for (let j = 1; j <= m; j++) {
      dp[i][j] = ai === b[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }
  return dp;
}

/**
 * Walk the LCS table back to produce a sequence of operations:
 *   { op: 'eq', a: line, b: line }
 *   { op: 'del', a: line }
 *   { op: 'add', b: line }
 *
 * Returned in source order.
 */
function diffLines(a, b) {
  const dp = lcsTable(a, b);
  const ops = [];
  let i = a.length;
  let j = b.length;
  while (i > 0 && j > 0) {
    if (a[i - 1] === b[j - 1]) {
      ops.push({ op: 'eq', a: a[i - 1], b: b[j - 1], ai: i - 1, bi: j - 1 });
      i--;
      j--;
    } else if (dp[i - 1][j] >= dp[i][j - 1]) {
      ops.push({ op: 'del', a: a[i - 1], ai: i - 1 });
      i--;
    } else {
      ops.push({ op: 'add', b: b[j - 1], bi: j - 1 });
      j--;
    }
  }
  while (i > 0) {
    ops.push({ op: 'del', a: a[i - 1], ai: i - 1 });
    i--;
  }
  while (j > 0) {
    ops.push({ op: 'add', b: b[j - 1], bi: j - 1 });
    j--;
  }
  return ops.reverse();
}

function escapeHtml(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Render a unified-style diff with N lines of context around each hunk.
 * Uses simple HTML — caller is responsible for placing it inside a
 * scrollable container.
 *
 * @param {string} baselineText
 * @param {string} variantText
 * @param {{baselineLabel?: string, variantLabel?: string, context?: number}} [opts]
 */
export function renderDiff(baselineText, variantText, opts = {}) {
  const ctx = Math.max(0, opts.context ?? 3);
  const baselineLabel = opts.baselineLabel || 'baseline';
  const variantLabel = opts.variantLabel || 'variant';

  const a = (baselineText || '').split('\n');
  const b = (variantText || '').split('\n');

  // Fast-path for identical content. Avoids spinning up the LCS table
  // for the common case of pin == current (e.g. the user diffs against
  // a pin they just created).
  if (baselineText === variantText) {
    return `<div class="diff-empty">No differences between <b>${escapeHtml(baselineLabel)}</b> and <b>${escapeHtml(variantLabel)}</b>.</div>`;
  }

  const ops = diffLines(a, b);

  // Group ops into hunks — runs of non-eq plus `ctx` lines of context
  // on each side.
  const isChange = (o) => o.op !== 'eq';
  const hunks = [];
  let i = 0;
  while (i < ops.length) {
    if (!isChange(ops[i])) {
      i++;
      continue;
    }
    // Find the extent of this run plus context. Hunks merge if their
    // contexts overlap so we don't print the same line twice.
    let start = Math.max(0, i - ctx);
    let end = i;
    while (end < ops.length) {
      if (isChange(ops[end])) {
        end++;
        continue;
      }
      // Look ahead — if another change happens within `2 * ctx` eq lines,
      // keep the hunk going.
      let look = end;
      while (look < ops.length && !isChange(ops[look]) && look - end < 2 * ctx) {
        look++;
      }
      if (look < ops.length && isChange(ops[look])) {
        end = look;
      } else {
        break;
      }
    }
    end = Math.min(ops.length, end + ctx);
    hunks.push(ops.slice(start, end));
    i = end;
  }

  let html = `<div class="diff-meta">--- ${escapeHtml(baselineLabel)}\n+++ ${escapeHtml(variantLabel)}</div>`;
  if (hunks.length === 0) {
    return `${html}<div class="diff-empty">No differences.</div>`;
  }

  for (const hunk of hunks) {
    // Compute hunk header @@ -ai,len +bi,len @@.
    let aStart = null;
    let bStart = null;
    let aLen = 0;
    let bLen = 0;
    for (const o of hunk) {
      if (o.op === 'eq') {
        if (aStart === null) aStart = o.ai;
        if (bStart === null) bStart = o.bi;
        aLen++;
        bLen++;
      } else if (o.op === 'del') {
        if (aStart === null) aStart = o.ai;
        aLen++;
      } else {
        if (bStart === null) bStart = o.bi;
        bLen++;
      }
    }
    aStart = aStart == null ? 0 : aStart + 1;
    bStart = bStart == null ? 0 : bStart + 1;
    html += `<div class="diff-hunk-header">@@ -${aStart},${aLen} +${bStart},${bLen} @@</div>`;
    for (const o of hunk) {
      if (o.op === 'eq') {
        html += `<div class="diff-line diff-eq"><span class="diff-marker"> </span><span class="diff-text">${escapeHtml(o.a)}</span></div>`;
      } else if (o.op === 'del') {
        html += `<div class="diff-line diff-del"><span class="diff-marker">-</span><span class="diff-text">${escapeHtml(o.a)}</span></div>`;
      } else {
        html += `<div class="diff-line diff-add"><span class="diff-marker">+</span><span class="diff-text">${escapeHtml(o.b)}</span></div>`;
      }
    }
  }
  return html;
}

/**
 * Quick "are these different?" check that skips the LCS work. Useful
 * for enabling/disabling a diff button.
 */
export function hasDifferences(a, b) {
  return (a || '') !== (b || '');
}
