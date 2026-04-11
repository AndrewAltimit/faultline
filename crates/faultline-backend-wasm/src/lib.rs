//! Browser WASM frontend for Faultline conflict simulation.
//!
//! Provides a wasm-bindgen API for loading, validating, running, and
//! stepping through scenarios from the browser.

use wasm_bindgen::prelude::*;

use faultline_engine::{Engine, validate_scenario};
use faultline_stats::MonteCarloRunner;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{EventRecord, MonteCarloConfig};

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the WASM module (console logging, panic hook, etc.).
#[wasm_bindgen]
pub fn init() {
    // Log initialization to the browser console.
    web_sys::console::log_1(&"faultline-backend-wasm initialized".into());
}

// ---------------------------------------------------------------------------
// Scenario loading
// ---------------------------------------------------------------------------

/// Parse a TOML scenario string and return its JSON representation.
///
/// # Errors
///
/// Returns a `JsValue` error string if parsing fails.
#[wasm_bindgen]
pub fn load_scenario(toml_str: &str) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    serde_wasm_bindgen::to_value(&scenario)
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a scenario TOML string for structural correctness.
///
/// Returns a JSON object `{ "valid": true }` on success, or an error
/// string describing the first validation failure.
#[wasm_bindgen]
pub fn validate_scenario_wasm(toml_str: &str) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    validate_scenario(&scenario)
        .map_err(|e| JsValue::from_str(&format!("validation error: {e}")))?;

    serde_wasm_bindgen::to_value(&serde_json::json!({ "valid": true }))
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Single run (stateless)
// ---------------------------------------------------------------------------

/// Run a single simulation from a TOML scenario string.
///
/// Returns the [`RunResult`] as a JSON-serialized `JsValue`.
///
/// # Errors
///
/// Returns a `JsValue` error string on parse or engine failure.
#[wasm_bindgen]
pub fn run_single(toml_str: &str, seed: Option<u64>) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    let actual_seed = seed.unwrap_or(42);

    let mut engine = Engine::with_seed(scenario, actual_seed)
        .map_err(|e| JsValue::from_str(&format!("engine init error: {e}")))?;

    let result = engine
        .run()
        .map_err(|e| JsValue::from_str(&format!("engine run error: {e}")))?;

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Monte Carlo (stateless batch)
// ---------------------------------------------------------------------------

/// Run a Monte Carlo batch of simulations and return aggregated results.
///
/// Returns a [`MonteCarloResult`] as JSON (includes all individual
/// `RunResult`s plus `MonteCarloSummary`).
#[wasm_bindgen]
pub fn run_monte_carlo(
    toml_str: &str,
    num_runs: u32,
    seed: Option<u64>,
) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    let config = MonteCarloConfig {
        num_runs,
        seed,
        collect_snapshots: false,
        parallel: false,
    };

    let result = MonteCarloRunner::run(&config, &scenario)
        .map_err(|e| JsValue::from_str(&format!("Monte Carlo error: {e}")))?;

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Persistent engine (stateful, for play/pause/step)
// ---------------------------------------------------------------------------

/// A persistent simulation engine for step-by-step execution.
///
/// Wraps the core `Engine` so the browser can advance tick-by-tick
/// without re-parsing and re-initializing each time.
#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine,
    scenario: Scenario,
    event_log: Vec<EventRecord>,
    finished: bool,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Create a new persistent engine from a TOML scenario string.
    #[wasm_bindgen(constructor)]
    pub fn new(toml_str: &str, seed: Option<u64>) -> Result<WasmEngine, JsValue> {
        let scenario: Scenario = toml::from_str(toml_str)
            .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

        let actual_seed = seed.unwrap_or(42);

        let engine = Engine::with_seed(scenario.clone(), actual_seed)
            .map_err(|e| JsValue::from_str(&format!("engine init error: {e}")))?;

        Ok(WasmEngine {
            engine,
            scenario,
            event_log: Vec::new(),
            finished: false,
        })
    }

    /// Advance the simulation by `n` ticks.
    ///
    /// Returns an array of per-tick results. Stops early if a victory
    /// condition is met or `max_ticks` is reached.
    pub fn tick_n(&mut self, n: u32) -> Result<JsValue, JsValue> {
        let mut tick_results = Vec::new();

        for _ in 0..n {
            if self.finished {
                break;
            }

            let result = self
                .engine
                .tick()
                .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;

            // Accumulate event log entries.
            let current_tick = self.engine.current_tick();
            let state = self.engine.state();
            for eid in &state.events_fired_this_tick {
                self.event_log.push(EventRecord {
                    tick: current_tick,
                    event_id: eid.clone(),
                });
            }

            if result.outcome.is_some() || current_tick >= self.engine.max_ticks() {
                self.finished = true;
            }

            tick_results.push(result);
        }

        serde_wasm_bindgen::to_value(&tick_results)
            .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
    }

    /// Get a snapshot of the current simulation state.
    pub fn get_state(&self) -> Result<JsValue, JsValue> {
        let snapshot = self.engine.snapshot();
        serde_wasm_bindgen::to_value(&snapshot)
            .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
    }

    /// Get the parsed scenario as JSON.
    pub fn get_scenario(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.scenario)
            .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
    }

    /// Get the accumulated event log.
    pub fn get_event_log(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.event_log)
            .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
    }

    /// Get the current tick number.
    pub fn current_tick(&self) -> u32 {
        self.engine.current_tick()
    }

    /// Get the maximum tick count.
    pub fn max_ticks(&self) -> u32 {
        self.engine.max_ticks()
    }

    /// Check whether the simulation has finished.
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}
