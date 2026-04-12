/**
 * Tech Cards sidebar panel — browses the ETRA-derived tech library and
 * injects cards into the TOML editor scenario.
 */
import { AppState } from './state.js';
import {
  TECH_LIBRARY,
  DOMAINS,
  groupedCards,
  domainCounts,
  insertCardIntoToml,
} from './tech-library.js';

export class TechCardsPanel {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   */
  constructor(bus) {
    this.bus = bus;
    this.container = document.getElementById('tech-cards-container');
    this.editor = document.getElementById('toml-editor');

    /** Currently selected domain id, or null for "All". */
    this.activeDomain = null;
    /** Current search query string. */
    this.searchQuery = '';
    /** Per-domain collapse state. */
    this.collapsed = { offensive: false, defensive: false };

    bus.on('scenario:loaded', () => this._render());
    this._render();
  }

  _render() {
    if (!this.container) return;
    const scenario = AppState.scenario;
    const factionIds = scenario ? Object.keys(scenario.factions || {}) : [];
    const existingIds = scenario ? Object.keys(scenario.technology || {}) : [];

    const counts = domainCounts();
    const total = Object.values(counts).reduce((a, b) => a + b, 0);

    const { offensive, defensive } = groupedCards({
      domain: this.activeDomain,
      search: this.searchQuery,
    });

    const visibleCount = offensive.length + defensive.length;
    const activeMeta = this.activeDomain
      ? DOMAINS.find((d) => d.id === this.activeDomain)
      : null;

    let html = '';
    html += `<div class="tech-panel-header">
      <div class="tech-panel-title">Threat Capability Library</div>
      <div class="tech-panel-subtitle">
        ${total} cards across ${DOMAINS.length} threat domains. Each card encodes an
        ETRA-derived capability as statistical effects you can grant to
        any faction. ${activeMeta ? escapeHtml(activeMeta.description) : 'Showing all domains.'}
      </div>
    </div>`;

    // Domain tab strip.
    html += '<div class="tech-domain-tabs">';
    html += this._renderDomainTab(null, 'All', total);
    for (const d of DOMAINS) {
      html += this._renderDomainTab(d.id, d.label, counts[d.id] || 0);
    }
    html += '</div>';

    // Search box.
    html += `<div class="tech-search-row">
      <input type="search" class="tech-search" placeholder="Search cards by name, id, or description…"
             value="${escapeAttr(this.searchQuery)}">
      <span class="tech-search-count">${visibleCount} visible</span>
    </div>`;

    if (visibleCount === 0) {
      html += '<div class="empty-state" style="padding: 16px 0;"><span style="font-size: 0.75rem;">No cards match the current filter.</span></div>';
    } else {
      html += this._renderGroup(
        'offensive',
        `Offensive (${offensive.length})`,
        offensive,
        factionIds,
        existingIds,
      );
      html += this._renderGroup(
        'defensive',
        `Defensive (${defensive.length})`,
        defensive,
        factionIds,
        existingIds,
      );
    }

    this.container.innerHTML = html;

    // Wire up tab clicks.
    this.container.querySelectorAll('.tech-domain-tab').forEach((tab) => {
      tab.addEventListener('click', () => {
        const d = tab.dataset.domain || null;
        this.activeDomain = d === '' ? null : d;
        this._render();
      });
    });
    // Search input.
    const searchEl = this.container.querySelector('.tech-search');
    if (searchEl) {
      searchEl.addEventListener('input', (e) => {
        this.searchQuery = e.target.value || '';
        // Re-render but keep focus on the search box.
        this._render();
        const again = this.container.querySelector('.tech-search');
        if (again) {
          again.focus();
          again.setSelectionRange(again.value.length, again.value.length);
        }
      });
    }
    // Group collapse toggles.
    this.container.querySelectorAll('.tech-group-header').forEach((hdr) => {
      hdr.addEventListener('click', () => {
        const key = hdr.dataset.group;
        this.collapsed[key] = !this.collapsed[key];
        this._render();
      });
    });
    // Add-card buttons.
    this.container.querySelectorAll('.tech-card-add').forEach((btn) => {
      btn.addEventListener('click', () => {
        const cardId = btn.dataset.card;
        const fidSelect = this.container.querySelector(
          `select[data-card="${cardId}"]`,
        );
        const fid = fidSelect && fidSelect.value ? fidSelect.value : null;
        this._addCard(cardId, fid);
      });
    });
  }

  _renderDomainTab(id, label, count) {
    const active = (id || null) === this.activeDomain;
    return `<button class="tech-domain-tab ${active ? 'active' : ''}" data-domain="${escapeAttr(id || '')}">
      ${escapeHtml(label)} <span class="tab-count">${count}</span>
    </button>`;
  }

  _renderGroup(key, title, cards, factionIds, existingIds) {
    if (!cards.length) return '';
    const collapsed = this.collapsed[key];
    const chevron = collapsed ? '▸' : '▾';
    let html = `<div class="tech-group-header" data-group="${escapeAttr(key)}">
      <span class="tech-group-chevron">${chevron}</span>
      <span class="tech-group-title">${escapeHtml(title)}</span>
    </div>`;
    if (collapsed) return html;
    for (const card of cards) {
      const present = existingIds.includes(card.id);
      const factionOptions = factionIds.length
        ? `<option value="">All factions</option>` +
          factionIds
            .map((fid) => `<option value="${escapeAttr(fid)}">${escapeAttr(fid)}</option>`)
            .join('')
        : `<option value="">(load scenario first)</option>`;

      html += `
        <div class="tech-card ${present ? 'present' : ''}" data-id="${escapeAttr(card.id)}">
          <div class="tech-card-head">
            <div class="tech-card-name">${escapeHtml(card.name)}${
              present ? ' <span class="tech-card-badge">in scenario</span>' : ''
            }</div>
            <div class="tech-card-category">${formatCategory(card.category)}</div>
          </div>
          <div class="tech-card-desc">${escapeHtml(card.description)}</div>
          <div class="tech-card-meta">
            <span title="Deployment cost (scenario resource units)">Deploy $${card.deployment_cost}</span>
            <span title="Per-tick upkeep">Upkeep ${card.cost_per_tick}/tick</span>
            <span title="Technology readiness level">TRL ${escapeHtml(card.trl)}</span>
          </div>
          <div class="tech-card-ref">${escapeHtml(card.etra_ref)}</div>
          <div class="tech-card-rationale">${escapeHtml(card.rationale)}</div>
          <div class="tech-card-effects">${this._renderEffects(card.effects)}</div>
          ${
            (card.countered_by || []).length
              ? `<div class="tech-card-counter">Countered by: ${(card.countered_by || [])
                  .map((c) => `<code>${escapeHtml(c)}</code>`)
                  .join(', ')}</div>`
              : ''
          }
          ${card.domain ? `<div class="tech-card-domain-tag">${escapeHtml(card.domain)}</div>` : ''}
          <div class="tech-card-actions">
            <select class="tech-card-faction" data-card="${escapeAttr(card.id)}"
                    ${factionIds.length ? '' : 'disabled'}>
              ${factionOptions}
            </select>
            <button class="btn-label tech-card-add" data-card="${escapeAttr(card.id)}"
                    ${factionIds.length ? '' : 'disabled'}>
              ${present ? 'Grant to faction' : 'Add to scenario'}
            </button>
          </div>
        </div>
      `;
    }
    return html;
  }

  _renderEffects(effects) {
    if (!effects || !effects.length) return '';
    const parts = effects.map((e) => {
      const keys = Object.keys(e).filter((k) => k !== 'type');
      const detail = keys
        .map((k) => `<span class="eff-k">${escapeHtml(k)}</span> ${escapeHtml(String(e[k]))}`)
        .join(' · ');
      return `<span class="eff-chip"><b>${escapeHtml(e.type)}</b> ${detail}</span>`;
    });
    return parts.join(' ');
  }

  _addCard(cardId, factionId) {
    if (!this.editor) return;
    const card = TECH_LIBRARY[cardId];
    if (!card) return;
    const current = this.editor.value || '';
    const grantTo = factionId ? [factionId] : this._allFactionIds();
    const { toml, added, granted } = insertCardIntoToml(current, card, grantTo);
    this.editor.value = toml;

    // Emit an input event so the editor's change listeners fire.
    this.editor.dispatchEvent(new Event('input', { bubbles: true }));

    // Feedback.
    const msg = added
      ? granted.length
        ? `Added ${card.name} and granted to ${granted.join(', ')}`
        : `Added ${card.name} to scenario (no faction grant)`
      : granted.length
        ? `Granted ${card.name} to ${granted.join(', ')}`
        : `${card.name} already present`;
    this._flash(msg, added || granted.length > 0);
  }

  _allFactionIds() {
    const scenario = AppState.scenario;
    return scenario ? Object.keys(scenario.factions || {}) : [];
  }

  _flash(message, success) {
    const el = document.createElement('div');
    el.className = 'validation-msg ' + (success ? 'success' : 'warning');
    el.textContent = message;
    el.style.margin = '4px 0';
    this.container.prepend(el);
    setTimeout(() => el.remove(), 3500);
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function escapeHtml(s) {
  if (s == null) return '';
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
function escapeAttr(s) {
  return escapeHtml(s);
}
function formatCategory(cat) {
  if (typeof cat === 'string') return cat;
  if (cat && cat.Custom) return `Custom: ${cat.Custom}`;
  return 'Other';
}
