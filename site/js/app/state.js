/**
 * Centralized application state for the Faultline simulator.
 */
export const AppState = {
  /** @type {object|null} Parsed Scenario JSON from WASM */
  scenario: null,

  /** @type {string} Raw TOML text */
  toml: '',

  /** @type {object|null} WasmEngine instance */
  engine: null,

  /** @type {object|null} StateSnapshot currently being displayed */
  currentSnapshot: null,

  /** @type {Array<object>} All snapshots from the current run */
  snapshots: [],

  /** @type {Array<object>} Event log from the current run */
  eventLog: [],

  /** @type {boolean} Whether play loop is active */
  isPlaying: false,

  /** @type {number} Ticks per animation frame */
  playSpeed: 1,

  /** @type {object|null} MonteCarloResult from batch run */
  mcResult: null,
};
