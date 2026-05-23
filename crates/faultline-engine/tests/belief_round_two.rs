//! Integration tests for the Epic M / J round-two belief mechanics.
//!
//! Pins the engine-level behavior contracts:
//! 1. Validation rejects an `AmbientIntel` referencing an unknown
//!    region (load-time fail-loud, no silent no-op).
//! 2. Intelligence-weighted fidelity: a high-intelligence faction
//!    ends the run with strictly higher mean belief confidence than a
//!    low-intelligence one, and all confidences stay in `[0, 1]`.
//! 3. `AmbientIntel` events are picked up by nearby factions
//!    (non-zero `ambient_intel_received`).
//! 4. Round-two produces `Inferred`-source beliefs; flipping
//!    `intelligence_weighting` off restores round-one fidelity
//!    (no `Inferred` beliefs; perfect confidence).
//! 5. Determinism — same scenario + same seed is bit-identical even
//!    with the round-two belief model running.

use std::path::{Path, PathBuf};

use faultline_engine::{Engine, validate_scenario};
use faultline_types::events::EventEffect;
use faultline_types::ids::{EventId, FactionId, RegionId};
use faultline_types::scenario::Scenario;

fn scenarios_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios")
}

fn load(filename: &str) -> Scenario {
    let path = scenarios_dir().join(filename);
    let toml_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    toml::from_str(&toml_str).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

/// Per-faction mean belief confidence from a run's belief report
/// (`force_confidence_sum / force_belief_ticks`).
fn mean_confidence(run: &faultline_types::stats::RunResult, faction: &str) -> f64 {
    let report = run
        .belief_accuracy
        .get(&FactionId::from(faction))
        .unwrap_or_else(|| panic!("no belief report for {faction}"));
    assert!(
        report.force_belief_ticks > 0,
        "{faction} held no force beliefs"
    );
    report.force_confidence_sum / f64::from(report.force_belief_ticks)
}

#[test]
fn ambient_intel_unknown_region_is_rejected() {
    let mut scenario = load("recon_fidelity_demo.toml");
    // The bundled scenario validates as-is.
    validate_scenario(&scenario).expect("bundled recon_fidelity_demo should validate");

    // Inject an AmbientIntel referencing a region that does not exist.
    let mut bad = scenario
        .events
        .get(&EventId::from("ne_field_intel"))
        .expect("bundled ambient-intel event")
        .clone();
    bad.id = EventId::from("bogus_ambient");
    bad.effects = vec![EventEffect::AmbientIntel {
        region: RegionId::from("atlantis"),
    }];
    scenario.events.insert(EventId::from("bogus_ambient"), bad);

    let err = validate_scenario(&scenario)
        .expect_err("AmbientIntel against an unknown region must be rejected");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("atlantis") && msg.contains("AmbientIntel"),
        "error should name the unknown region and effect, got: {msg}"
    );
}

#[test]
fn high_intelligence_faction_has_higher_belief_confidence() {
    let scenario = load("recon_fidelity_demo.toml");
    let mut engine = Engine::with_seed(scenario, 11).expect("engine builds");
    let run = engine.run().expect("run completes");

    let recon = mean_confidence(&run, "recon_corps"); // intelligence 0.9
    let fog = mean_confidence(&run, "fog_division"); // intelligence 0.2

    assert!(
        recon > fog,
        "high-intel recon_corps ({recon:.3}) should out-believe low-intel fog_division ({fog:.3})"
    );
    // The confidence ceiling is intelligence-derived: recon ~0.885,
    // fog ~0.43. Allow generous slack for decay between observations.
    assert!(
        (0.6..=0.95).contains(&recon),
        "recon confidence {recon:.3} outside expected band"
    );
    assert!(
        (0.3..=0.6).contains(&fog),
        "fog confidence {fog:.3} outside expected band"
    );
    for (fid, report) in &run.belief_accuracy {
        let mean = report.force_confidence_sum / f64::from(report.force_belief_ticks.max(1));
        assert!(
            (0.0..=1.0).contains(&mean),
            "{fid} mean confidence {mean} out of [0,1]"
        );
    }
}

#[test]
fn ambient_intel_is_picked_up_and_beliefs_are_inferred() {
    let scenario = load("recon_fidelity_demo.toml");
    let mut engine = Engine::with_seed(scenario, 11).expect("engine builds");
    let run = engine.run().expect("run completes");

    let total_ambient: u32 = run
        .belief_accuracy
        .values()
        .map(|r| r.ambient_intel_received)
        .sum();
    assert!(
        total_ambient > 0,
        "AmbientIntel events should be picked up by nearby factions"
    );
    let total_inferred: u32 = run
        .belief_accuracy
        .values()
        .map(|r| r.inferred_beliefs_terminal)
        .sum();
    assert!(
        total_inferred > 0,
        "round-two intelligence weighting should produce Inferred-source beliefs"
    );
}

#[test]
fn disabling_intelligence_weighting_restores_round_one_fidelity() {
    let mut scenario = load("recon_fidelity_demo.toml");
    if let Some(cfg) = scenario.simulation.belief_model.as_mut() {
        cfg.intelligence_weighting = false;
    }
    let mut engine = Engine::with_seed(scenario, 11).expect("engine builds");
    let run = engine.run().expect("run completes");

    // No Inferred beliefs at run end — round-one path tags everything
    // DirectObservation/Stale/Deceived. This is the clean gate signal
    // that `intelligence_weighting = false` restored round-one
    // fidelity. (Mean confidence is still below 1.0 because unrefreshed
    // beliefs decay even on the round-one path — staleness is
    // orthogonal to intelligence weighting.)
    let total_inferred: u32 = run
        .belief_accuracy
        .values()
        .map(|r| r.inferred_beliefs_terminal)
        .sum();
    assert_eq!(
        total_inferred, 0,
        "round-one fidelity must not produce Inferred beliefs"
    );

    // Both factions, freshly observing, should reach a higher mean
    // confidence on the round-one path (fresh = 1.0) than the
    // intelligence-capped round-two path. Verify against a round-two
    // run of the same scenario.
    let r2_scenario = load("recon_fidelity_demo.toml");
    let mut r2_engine = Engine::with_seed(r2_scenario, 11).expect("engine builds");
    let r2_run = r2_engine.run().expect("run completes");
    let fog_r1 = mean_confidence(&run, "fog_division");
    let fog_r2 = mean_confidence(&r2_run, "fog_division");
    assert!(
        fog_r1 > fog_r2,
        "round-one fog confidence ({fog_r1:.3}) should exceed intelligence-capped round-two ({fog_r2:.3})"
    );
}

#[test]
fn round_two_run_is_deterministic() {
    let run_once = || {
        let scenario = load("recon_fidelity_demo.toml");
        let mut engine = Engine::with_seed(scenario, 11).expect("engine builds");
        let run = engine.run().expect("run completes");
        serde_json::to_string(&run.belief_accuracy).expect("serialize")
    };
    assert_eq!(run_once(), run_once(), "round-two belief output diverged");
}
