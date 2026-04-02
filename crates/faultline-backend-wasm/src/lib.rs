//! Browser WASM frontend for Faultline conflict simulation.
//!
//! Provides a minimal wasm-bindgen API for loading, validating, and
//! running scenarios from the browser.

use wasm_bindgen::prelude::*;

use faultline_engine::{Engine, validate_scenario};
use faultline_types::scenario::Scenario;

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
// Single run
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
