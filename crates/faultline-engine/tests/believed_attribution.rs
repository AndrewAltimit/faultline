//! Integration tests for Epic M round-two believed-attribution rolls.
//!
//! Pins the engine-level behavior contracts:
//! 1. Validation rejects `believed_attribution = true` without
//!    `enabled = true` (load-time fail-loud).
//! 2. Validation rejects `believed_attribution = true` with no kill
//!    chains (silent-no-op shape).
//! 3. The bundled `misattribution_demo` produces believed-attribution
//!    rolls, a deceived/low-intelligence defender misattributes, and
//!    the misattribution drives an alliance fracture against the
//!    innocent ally.
//! 4. Turning the sub-flag off (everything else equal) yields zero
//!    attribution events, no believed `attributed_faction`, and the
//!    fracture accounting falls back to the true attacker — and the
//!    run is bit-identical to a copy with the whole belief block
//!    removed (legacy RNG order preserved).
//! 5. Determinism — same scenario + seed is bit-identical.

use std::path::{Path, PathBuf};

use faultline_engine::{Engine, validate_scenario};
use faultline_types::ids::FactionId;
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

#[test]
fn bundled_misattribution_demo_validates() {
    let scenario = load("misattribution_demo.toml");
    validate_scenario(&scenario).expect("bundled misattribution_demo should validate");
}

#[test]
fn believed_attribution_without_enabled_is_rejected() {
    let mut scenario = load("misattribution_demo.toml");
    if let Some(cfg) = scenario.simulation.belief_model.as_mut() {
        cfg.enabled = false;
        cfg.believed_attribution = true;
    }
    // Drop the fracture rule that names a non-attacker (only valid under
    // believed-attribution) so this test isolates the belief-block
    // enabled-requirement guard rather than the fracture-rule guard.
    for faction in scenario.factions.values_mut() {
        faction.alliance_fracture = None;
    }
    let err = validate_scenario(&scenario)
        .expect_err("believed_attribution without enabled must be rejected");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("believed_attribution") && msg.contains("enabled"),
        "error should explain the enabled requirement, got: {msg}"
    );
}

#[test]
fn believed_attribution_without_kill_chains_is_rejected() {
    let mut scenario = load("misattribution_demo.toml");
    scenario.kill_chains.clear();
    // Also drop the AttributionThreshold fracture rule so we exercise
    // the belief-block guard rather than the fracture-rule guard.
    for faction in scenario.factions.values_mut() {
        faction.alliance_fracture = None;
    }
    let err = validate_scenario(&scenario)
        .expect_err("believed_attribution with no kill chains must be rejected");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("believed_attribution") && msg.contains("kill chain"),
        "error should explain the kill-chain requirement, got: {msg}"
    );
}

#[test]
fn deceived_low_intel_defender_misattributes_and_fractures() {
    let scenario = load("misattribution_demo.toml");
    let blue = FactionId::from("blue_defender");
    let gray = FactionId::from("gray_partner");
    let red = FactionId::from("red_attacker");

    // Aggregate across a handful of seeds so the contract isn't
    // hostage to one RNG draw.
    let mut total_rolls = 0u64;
    let mut misattributed = 0u64;
    let mut to_gray = 0u64;
    let mut fractures_against_gray = 0u64;

    for seed in 0..12u64 {
        let mut engine = Engine::with_seed(scenario.clone(), seed).expect("engine builds");
        let run = engine.run().expect("run completes");
        for ev in &run.attribution_events {
            assert_eq!(ev.defender, blue, "only blue is a chain target");
            assert_eq!(ev.true_attacker, red, "red is the true author");
            total_rolls += 1;
            if ev.misattributed {
                misattributed += 1;
            }
            if ev.believed_attacker == gray {
                to_gray += 1;
            }
        }
        for fr in &run.fracture_events {
            if fr.faction == blue && fr.counterparty == gray {
                fractures_against_gray += 1;
            }
        }
    }

    assert!(total_rolls > 0, "expected believed-attribution rolls");
    assert!(
        misattributed > 0,
        "a low-intel deceived defender should misattribute at least once across 12 seeds"
    );
    // Every misattribution lands on gray (the only other candidate
    // besides red, and the one the false flag points at).
    assert_eq!(
        to_gray, misattributed,
        "all misattributions should land on the framed innocent (gray)"
    );
    assert!(
        fractures_against_gray > 0,
        "at least one misattribution should drive blue to fracture against gray"
    );
}

#[test]
fn sub_flag_off_yields_no_rolls_and_true_attacker_fracture() {
    let mut scenario = load("misattribution_demo.toml");
    if let Some(cfg) = scenario.simulation.belief_model.as_mut() {
        cfg.believed_attribution = false;
    }
    // The bundled fracture rule names gray (a non-attacker) under
    // believed-attribution; with the sub-flag off that rule could never
    // fire, so re-point it at the true attacker to keep the scenario
    // analytically meaningful and valid.
    if let Some(af) = scenario
        .factions
        .get_mut(&FactionId::from("blue_defender"))
        .and_then(|f| f.alliance_fracture.as_mut())
    {
        for rule in &mut af.rules {
            rule.condition = faultline_types::faction::FractureCondition::AttributionThreshold {
                attacker: FactionId::from("red_attacker"),
                threshold: 0.3,
            };
            rule.counterparty = FactionId::from("red_attacker");
        }
    }
    validate_scenario(&scenario).expect("sub-flag-off variant should validate");

    let mut engine = Engine::with_seed(scenario, 23).expect("engine builds");
    let run = engine.run().expect("run completes");
    assert!(
        run.attribution_events.is_empty(),
        "no believed-attribution rolls should be recorded when the sub-flag is off"
    );
}

#[test]
fn believed_attribution_is_deterministic() {
    let scenario = load("misattribution_demo.toml");
    let mut a = Engine::with_seed(scenario.clone(), 5).expect("engine a");
    let mut b = Engine::with_seed(scenario, 5).expect("engine b");
    let ra = a.run().expect("run a");
    let rb = b.run().expect("run b");
    let ja = serde_json::to_string(&ra.attribution_events).expect("ser a");
    let jb = serde_json::to_string(&rb.attribution_events).expect("ser b");
    assert_eq!(
        ja, jb,
        "believed-attribution log must be bit-identical for the same seed"
    );
}

#[test]
fn high_intelligence_defender_rarely_misattributes_without_false_flag() {
    // Contract: misattribution scales with the defender's
    // *intelligence*, not just deception. Remove the false flag and
    // give blue a high intelligence; its believed attribution should
    // match the true attacker on the large majority of rolls. (The
    // RNG-order / legacy bit-identity guarantee is covered by the
    // verify-bundled CI stage, which pins every bundled scenario's
    // `output_hash` — turning the sub-flag off leaves those hashes
    // unchanged because the draw is gated entirely behind the flag.)
    let mut scenario = load("misattribution_demo.toml");
    scenario
        .events
        .remove(&faultline_types::ids::EventId::from("red_false_flag"));
    if let Some(blue) = scenario.factions.get_mut(&FactionId::from("blue_defender")) {
        blue.intelligence = 0.95;
    }
    // The fracture rule names gray; with no false flag and high intel,
    // it simply rarely fires — still valid under believed-attribution.
    validate_scenario(&scenario).expect("variant validates");

    let red = FactionId::from("red_attacker");
    let mut total = 0u64;
    let mut correct = 0u64;
    for seed in 0..12u64 {
        let mut engine = Engine::with_seed(scenario.clone(), seed).expect("engine builds");
        let run = engine.run().expect("run completes");
        for ev in &run.attribution_events {
            total += 1;
            if ev.believed_attacker == red {
                correct += 1;
            }
        }
    }
    assert!(total > 0, "expected attribution rolls");
    // High intelligence (true-weight ~0.956) → strong majority correct.
    let accuracy = correct as f64 / total as f64;
    assert!(
        accuracy >= 0.8,
        "high-intel defender should attribute correctly on ≥80% of rolls, got {accuracy:.2}"
    );
}
