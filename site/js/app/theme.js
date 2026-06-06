/**
 * Light / dark theme toggle.
 *
 * The site is dark by default. This module resolves the active theme from
 * (in priority order) an explicit user choice saved in `localStorage`, then
 * the OS `prefers-color-scheme` hint, then dark; applies it by setting
 * `data-theme` on the document element; wires a header toggle control; and
 * emits a `theme:changed` event so canvas charts/maps can redraw with the
 * freshly-applied palette.
 *
 * Dark is the default and renders bit-identically to before this module
 * existed: when `data-theme` is `dark` (or absent) none of the light
 * overrides in css/styles.css apply.
 *
 * The pure resolution helpers (`resolveInitialTheme`, `normalizeTheme`,
 * `nextTheme`) take their inputs as arguments so they can be unit-tested
 * under Node without a DOM.
 */

export const THEMES = ['dark', 'light'];
export const STORAGE_KEY = 'faultline.theme';

/** Coerce an arbitrary value to a valid theme, defaulting to dark. */
export function normalizeTheme(value) {
  return value === 'light' ? 'light' : 'dark';
}

/** The theme you land on after toggling from `current`. */
export function nextTheme(current) {
  return normalizeTheme(current) === 'dark' ? 'light' : 'dark';
}

/**
 * Resolve the theme to apply on first paint.
 *
 * @param {string|null|undefined} stored  value read from localStorage (or null)
 * @param {boolean} prefersLight  result of a `prefers-color-scheme: light` query
 * @returns {'dark'|'light'}
 */
export function resolveInitialTheme(stored, prefersLight) {
  if (stored === 'light' || stored === 'dark') return stored;
  return prefersLight ? 'light' : 'dark';
}

function readStored() {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function writeStored(theme) {
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Private mode / storage disabled — non-fatal; the choice just won't persist.
  }
}

function prefersLightScheme() {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-color-scheme: light)').matches
  );
}

/** Inline SVGs for the toggle. Sun = currently dark (click for light). */
const ICON_SUN =
  '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>';
const ICON_MOON =
  '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>';

export class ThemeController {
  /**
   * @param {import('./event-bus.js').EventBus} [bus] optional bus; when
   *   provided, a `theme:changed` event carrying the new theme string is
   *   emitted on every change so listeners (charts) can redraw.
   * @param {() => void} [onChange] optional side-effect run after each
   *   change (e.g. invalidate the cached chart palette).
   */
  constructor(bus, onChange) {
    this.bus = bus || null;
    this.onChange = typeof onChange === 'function' ? onChange : null;
    this.theme = resolveInitialTheme(readStored(), prefersLightScheme());
    this._apply(this.theme, /* persist */ false, /* notify */ false);
  }

  /** Currently active theme. */
  current() {
    return this.theme;
  }

  /** Set the active theme explicitly. */
  set(theme) {
    this._apply(normalizeTheme(theme), true, true);
  }

  /** Flip between dark and light. */
  toggle() {
    this._apply(nextTheme(this.theme), true, true);
  }

  _apply(theme, persist, notify) {
    this.theme = theme;
    if (typeof document !== 'undefined' && document.documentElement) {
      document.documentElement.setAttribute('data-theme', theme);
    }
    if (persist) writeStored(theme);
    this._syncButton();
    if (this.onChange) this.onChange();
    if (notify && this.bus) this.bus.emit('theme:changed', theme);
  }

  /**
   * Create the toggle button and append it to `container`. Returns the
   * button element. Safe to call once; the controller keeps a reference so
   * subsequent theme changes update the icon/label.
   *
   * @param {HTMLElement} container
   */
  mountToggle(container) {
    if (!container || typeof document === 'undefined') return null;
    const btn = document.createElement('button');
    btn.className = 'theme-toggle';
    btn.type = 'button';
    btn.addEventListener('click', () => this.toggle());
    this._btn = btn;
    container.appendChild(btn);
    this._syncButton();
    return btn;
  }

  _syncButton() {
    if (!this._btn) return;
    const isDark = this.theme === 'dark';
    // Show the icon for the theme you'd switch TO.
    this._btn.innerHTML = isDark ? ICON_SUN : ICON_MOON;
    const target = isDark ? 'light' : 'dark';
    this._btn.setAttribute('aria-label', `Switch to ${target} theme`);
    this._btn.setAttribute('title', `Switch to ${target} theme`);
    this._btn.setAttribute('aria-pressed', String(!isDark));
  }
}
