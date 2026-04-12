/**
 * Minimal pub/sub event bus for inter-module communication.
 * All app modules communicate through events rather than direct coupling.
 */
export class EventBus {
  constructor() {
    /** @type {Map<string, Set<Function>>} */
    this._listeners = new Map();
  }

  /**
   * Subscribe to an event.
   * @param {string} event
   * @param {Function} fn
   */
  on(event, fn) {
    if (!this._listeners.has(event)) {
      this._listeners.set(event, new Set());
    }
    this._listeners.get(event).add(fn);
  }

  /**
   * Unsubscribe from an event.
   * @param {string} event
   * @param {Function} fn
   */
  off(event, fn) {
    const fns = this._listeners.get(event);
    if (fns) fns.delete(fn);
  }

  /**
   * Emit an event with optional data payload.
   * @param {string} event
   * @param {*} data
   */
  emit(event, data) {
    const fns = this._listeners.get(event);
    if (fns) {
      for (const fn of fns) {
        try {
          fn(data);
        } catch (e) {
          console.error(`EventBus: listener error on "${event}":`, e);
        }
      }
    }
  }
}
