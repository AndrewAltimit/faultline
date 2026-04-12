//! Browser WASM frontend for Faultline conflict simulation.
//!
//! Provides a wasm-bindgen API for loading, validating, running, and
//! stepping through scenarios from the browser.

use wasm_bindgen::prelude::*;

use faultline_engine::{Engine, validate_scenario};
use faultline_stats::{MonteCarloRunner, sensitivity::run_sensitivity};
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloConfig;

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
///
/// `collect_snapshots` (default `false`) controls whether per-tick
/// snapshots are retained on every run. The browser regional-control
/// heatmap needs them; the win-probability bars do not.
#[wasm_bindgen]
pub fn run_monte_carlo(
    toml_str: &str,
    num_runs: u32,
    seed: Option<u64>,
    collect_snapshots: Option<bool>,
) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    let config = MonteCarloConfig {
        num_runs,
        seed,
        collect_snapshots: collect_snapshots.unwrap_or(false),
        parallel: false,
    };

    let result = MonteCarloRunner::run(&config, &scenario)
        .map_err(|e| JsValue::from_str(&format!("Monte Carlo error: {e}")))?;

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Sensitivity analysis (stateless)
// ---------------------------------------------------------------------------

/// Run a one-parameter sensitivity sweep, executing a Monte Carlo
/// batch at each step in `[low, high]`.
///
/// Returns a [`SensitivityResult`] as JSON. The `param` argument uses
/// the same dotted-path syntax as the `--sensitivity-param` CLI flag
/// (e.g. `political_climate.tension`).
#[wasm_bindgen]
pub fn run_sensitivity_wasm(
    toml_str: &str,
    param: &str,
    low: f64,
    high: f64,
    steps: u32,
    runs_per_step: u32,
    seed: Option<u64>,
) -> Result<JsValue, JsValue> {
    let scenario: Scenario = toml::from_str(toml_str)
        .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

    let config = MonteCarloConfig {
        num_runs: runs_per_step,
        seed,
        collect_snapshots: false,
        parallel: false,
    };

    let result = run_sensitivity(&scenario, &config, param, low, high, steps)
        .map_err(|e| JsValue::from_str(&format!("sensitivity error: {e}")))?;

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Persistent engine (stateful, for play/pause/step)
// ---------------------------------------------------------------------------

/// A persistent simulation engine for step-by-step execution.
///
/// Wraps the core `Engine` so the browser can advance tick-by-tick
/// without re-parsing and re-initializing each time. The scenario is
/// owned by the inner `Engine` (accessible via `engine.scenario()`)
/// and not duplicated here.
#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Create a new persistent engine from a TOML scenario string.
    #[wasm_bindgen(constructor)]
    pub fn new(toml_str: &str, seed: Option<u64>) -> Result<WasmEngine, JsValue> {
        let scenario: Scenario = toml::from_str(toml_str)
            .map_err(|e| JsValue::from_str(&format!("TOML parse error: {e}")))?;

        let actual_seed = seed.unwrap_or(42);

        let engine = Engine::with_seed(scenario, actual_seed)
            .map_err(|e| JsValue::from_str(&format!("engine init error: {e}")))?;

        Ok(WasmEngine { engine })
    }

    /// Advance the simulation by `n` ticks.
    ///
    /// Returns an array of per-tick results. Stops early if a victory
    /// condition is met or `max_ticks` is reached.
    ///
    /// Each `TickResult` includes the events that fired during that tick
    /// (`events_fired` field). The JS frontend is responsible for
    /// accumulating these into a session-level event log if needed.
    pub fn tick_n(&mut self, n: u32) -> Result<JsValue, JsValue> {
        let mut tick_results = Vec::new();

        for _ in 0..n {
            if self.engine.is_finished() {
                break;
            }

            let result = self
                .engine
                .tick()
                .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;

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
        serde_wasm_bindgen::to_value(self.engine.scenario())
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
        self.engine.is_finished()
    }
}

// ---------------------------------------------------------------------------
// Native tests (exercise underlying logic without WASM runtime)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::stats::EventRecord;

    const TUTORIAL_TOML: &str = include_str!("../../../scenarios/tutorial_symmetric.toml");

    fn load_toml(toml_str: &str) -> Scenario {
        toml::from_str(toml_str).expect("should parse TOML")
    }

    // -- Scenario parsing ------------------------------------------------

    #[test]
    fn parse_tutorial_scenario() {
        let scenario = load_toml(TUTORIAL_TOML);
        assert_eq!(scenario.meta.name, "Tutorial \u{2014} Symmetric Conflict");
        assert_eq!(scenario.factions.len(), 2);
        assert_eq!(scenario.map.regions.len(), 4);
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let result: Result<Scenario, _> = toml::from_str("not valid toml {{{{");
        assert!(result.is_err());
    }

    #[test]
    fn validate_tutorial_scenario_passes() {
        let scenario = load_toml(TUTORIAL_TOML);
        assert!(validate_scenario(&scenario).is_ok());
    }

    #[test]
    fn validate_empty_factions_fails() {
        let mut scenario = load_toml(TUTORIAL_TOML);
        scenario.factions.clear();
        assert!(validate_scenario(&scenario).is_err());
    }

    // -- Engine lifecycle (WasmEngine logic paths) -----------------------

    #[test]
    fn engine_lifecycle_create_and_tick() {
        let scenario = load_toml(TUTORIAL_TOML);
        let mut engine = Engine::with_seed(scenario, 42).expect("engine init");

        assert_eq!(engine.current_tick(), 0);
        assert!(!engine.is_finished());

        let result = engine.tick().expect("tick 1");
        assert_eq!(result.tick, 1);
        assert_eq!(engine.current_tick(), 1);
    }

    #[test]
    fn engine_lifecycle_tick_n_batch() {
        let scenario = load_toml(TUTORIAL_TOML);
        let mut engine = Engine::with_seed(scenario, 42).expect("engine init");

        // Simulate tick_n(5) by calling tick() 5 times.
        let mut results = Vec::new();
        let mut event_log: Vec<EventRecord> = Vec::new();
        let mut finished = false;

        for _ in 0..5 {
            if finished {
                break;
            }
            let result = engine.tick().expect("tick should succeed");
            let current_tick = engine.current_tick();
            for eid in &engine.state().events_fired_this_tick {
                event_log.push(EventRecord {
                    tick: current_tick,
                    event_id: eid.clone(),
                });
            }
            if result.outcome.is_some() || current_tick >= engine.max_ticks() {
                finished = true;
            }
            results.push(result);
        }

        assert_eq!(results.len(), 5);
        assert_eq!(results[0].tick, 1);
        assert_eq!(results[4].tick, 5);
    }

    #[test]
    fn engine_lifecycle_snapshot_serializable() {
        let scenario = load_toml(TUTORIAL_TOML);
        let engine = Engine::with_seed(scenario, 42).expect("engine init");

        let snapshot = engine.snapshot();
        let json = serde_json::to_string(&snapshot).expect("snapshot should serialize");
        assert!(json.contains("\"tick\":0"));
        assert!(json.contains("\"region_control\""));
        assert!(json.contains("\"faction_states\""));
    }

    #[test]
    fn engine_lifecycle_scenario_serializable() {
        let scenario = load_toml(TUTORIAL_TOML);
        let json = serde_json::to_string(&scenario).expect("scenario should serialize");
        assert!(json.contains("\"alpha\""));
        assert!(json.contains("\"bravo\""));
    }

    #[test]
    fn engine_lifecycle_finished_flag_sync() {
        let mut scenario = load_toml(TUTORIAL_TOML);
        scenario.simulation.max_ticks = 10;

        let mut engine = Engine::with_seed(scenario, 42).expect("engine init");
        let mut finished = false;

        for _ in 0..10 {
            if finished {
                break;
            }
            let result = engine.tick().expect("tick");
            if result.outcome.is_some() || engine.current_tick() >= engine.max_ticks() {
                finished = true;
            }
        }

        assert!(finished, "finished flag should be true after max_ticks");
        assert!(engine.is_finished(), "engine.is_finished() should agree");
    }

    #[test]
    fn engine_lifecycle_event_log_accumulation() {
        let toml_str = include_str!("../../../scenarios/tutorial_asymmetric.toml");
        let scenario = load_toml(toml_str);

        let mut engine = Engine::with_seed(scenario, 42).expect("engine init");
        let mut event_log: Vec<EventRecord> = Vec::new();

        loop {
            let result = engine.tick().expect("tick");
            let current_tick = engine.current_tick();
            for eid in &engine.state().events_fired_this_tick {
                event_log.push(EventRecord {
                    tick: current_tick,
                    event_id: eid.clone(),
                });
            }
            if result.outcome.is_some() || current_tick >= engine.max_ticks() {
                break;
            }
        }

        // All event records should have valid tick bounds.
        for record in &event_log {
            assert!(record.tick > 0, "event tick should be > 0");
            assert!(
                record.tick <= engine.current_tick(),
                "event tick should be <= final tick"
            );
        }
    }

    // -- Monte Carlo underlying logic ------------------------------------

    #[test]
    fn monte_carlo_runner_with_tutorial_scenario() {
        let scenario = load_toml(TUTORIAL_TOML);
        let config = MonteCarloConfig {
            num_runs: 10,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = MonteCarloRunner::run(&config, &scenario).expect("MC should succeed");

        assert_eq!(result.summary.total_runs, 10);
        assert_eq!(result.runs.len(), 10);

        // Win rates should sum to ~1.0 (allowing for stalemates).
        let total_win_rate: f64 = result.summary.win_rates.values().sum();
        assert!(
            total_win_rate <= 1.0 + f64::EPSILON,
            "win rates should sum to <= 1.0, got {total_win_rate}"
        );

        // Duration stats should be populated.
        assert!(result.summary.average_duration > 0.0);
    }

    #[test]
    fn monte_carlo_result_serializable() {
        let scenario = load_toml(TUTORIAL_TOML);
        let config = MonteCarloConfig {
            num_runs: 5,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = MonteCarloRunner::run(&config, &scenario).expect("MC should succeed");

        let json = serde_json::to_string(&result).expect("MC result should serialize");
        assert!(json.contains("\"win_rates\""));
        assert!(json.contains("\"average_duration\""));
        assert!(json.contains("\"regional_control\""));
    }

    #[test]
    fn monte_carlo_zero_runs_errors() {
        let scenario = load_toml(TUTORIAL_TOML);
        let config = MonteCarloConfig {
            num_runs: 0,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = MonteCarloRunner::run(&config, &scenario);
        assert!(result.is_err(), "zero runs should error");
    }

    #[test]
    fn monte_carlo_invalid_scenario_errors() {
        let mut scenario = load_toml(TUTORIAL_TOML);
        scenario.factions.clear();
        scenario.map.regions.clear();

        let config = MonteCarloConfig {
            num_runs: 5,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = MonteCarloRunner::run(&config, &scenario);
        assert!(result.is_err(), "invalid scenario should error");
    }

    // -- Tick stepping matches full run ----------------------------------

    #[test]
    fn tick_stepping_matches_full_run_result() {
        let scenario = load_toml(TUTORIAL_TOML);

        // Full run.
        let mut engine_full = Engine::with_seed(scenario.clone(), 42).expect("engine init");
        let run_result = engine_full.run().expect("run");

        // Tick-stepped run.
        let mut engine_step = Engine::with_seed(scenario, 42).expect("engine init");
        let mut event_log: Vec<EventRecord> = Vec::new();

        loop {
            let result = engine_step.tick().expect("tick");
            let current_tick = engine_step.current_tick();
            for eid in &engine_step.state().events_fired_this_tick {
                event_log.push(EventRecord {
                    tick: current_tick,
                    event_id: eid.clone(),
                });
            }
            if result.outcome.is_some() || current_tick >= engine_step.max_ticks() {
                break;
            }
        }

        let step_snapshot = engine_step.snapshot();

        // Final ticks should match.
        assert_eq!(
            step_snapshot.tick, run_result.final_state.tick,
            "final tick mismatch"
        );

        // Event log lengths should match.
        assert_eq!(
            event_log.len(),
            run_result.event_log.len(),
            "event log length mismatch"
        );

        // Faction states should match.
        for (fid, fs_step) in &step_snapshot.faction_states {
            let fs_run = run_result
                .final_state
                .faction_states
                .get(fid)
                .expect("faction should exist");
            assert!(
                (fs_step.total_strength - fs_run.total_strength).abs() < f64::EPSILON,
                "strength mismatch for {fid}"
            );
        }
    }

    // -- Fracture scenario coverage -------------------------------------

    #[test]
    fn fracture_scenario_monte_carlo() {
        let toml_str = include_str!("../../../scenarios/us_institutional_fracture.toml");
        let scenario = load_toml(toml_str);
        let config = MonteCarloConfig {
            num_runs: 5,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = MonteCarloRunner::run(&config, &scenario).expect("MC should succeed");

        assert_eq!(result.summary.total_runs, 5);
        assert_eq!(result.runs.len(), 5);

        // Should have 4 faction entries in win_rates (may be 0.0 for some).
        // Some factions may not appear if they never win.
        // Regional control should have all 8 regions.
        assert_eq!(
            result.summary.regional_control.len(),
            8,
            "should have 8 regions in regional_control"
        );
    }
}
