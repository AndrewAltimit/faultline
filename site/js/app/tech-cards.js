/**
 * Tech Cards sidebar panel — browses the ETRA-derived tech library and
 * injects cards into the TOML editor scenario.
 */
import { AppState } from './state.js';
import { TECH_LIBRARY, groupedCards, insertCardIntoToml } from './tech-library.js';

export class TechCardsPanel {
  /**
   * @param {import('./event-bus.js').EventBus} bus
   */
  constructor(bus) {
    this.bus = bus;
    this.container = document.getElementById('tech-cards-container');
    this.editor = document.getElementById('toml-editor');

    bus.on('scenario:loaded', () => this._render());
    // Trigger an initial render so the panel shows content even before
    // a scenario is loaded.
    this._render();
  }

  _render() {
    if (!this.container) return;
    const scenario = AppState.scenario;
    const factionIds = scenario ? Object.keys(scenario.factions || {}) : [];
    const existingIds = scenario ? Object.keys(scenario.technology || {}) : [];

    const { offensive, defensive } = groupedCards();

    let html = '';
    html += `<div class="tech-panel-header">
      <div class="tech-panel-title">Drone Threat Library</div>
      <div class="tech-panel-subtitle">
        ETRA-derived tech cards. Each card encodes a capability from the
        Locust ETRA assessment as a bundle of statistical effects you
        can grant to any faction.
      </div>
    </div>`;

    html += this._renderGroup('Offensive capabilities', offensive, factionIds, existingIds);
    html += this._renderGroup('Defensive capabilities', defensive, factionIds, existingIds);

    this.container.innerHTML = html;

    // Wire up buttons after DOM update.
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

  _renderGroup(title, cards, factionIds, existingIds) {
    if (!cards.length) return '';
    let html = `<div class="tech-group-title">${title}</div>`;
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
            card.countered_by.length
              ? `<div class="tech-card-counter">Countered by: ${card.countered_by
                  .map((c) => `<code>${escapeHtml(c)}</code>`)
                  .join(', ')}</div>`
              : ''
          }
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
