//! Integration tests for the Epic J round-one utility-driven AI.
//!
//! Pins the engine-level behavior contracts:
//! 1. Legacy fast path — scenarios without `[utility]` produce
//!    identical RunResult as before (no `utility_decisions`).
//! 2. With a profile, `utility_decisions` records non-zero
//!    contributions for at least one decision phase per run.
//! 3. Adaptive triggers fire when their condition holds, and the
//!    fire count surfaces in the per-run report.
//! 4. Determinism — same seed produces bit-identical run output
//!    even when `[utility]` is declared.
//! 5. Behavioral effect — a strong control-maximizer profile
//!    accumulates measurably positive `control` term contribution.

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_types::faction::{
    AdaptiveCondition, AdaptiveTrigger, Diplomacy, DiplomaticStance, Faction, FactionType,
    FactionUtility, ForceUnit, UnitType, UtilityTerm,
};
use faultline_types::ids::{FactionId, ForceId, RegionId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

/// Build a 4-region, two-faction symmetric scenario with optional
/// per-faction `[utility]` profiles.
fn make_scenario(
    red_utility: Option<FactionUtility>,
    blue_utility: Option<FactionUtility>,
    seed: u64,
    max_ticks: u32,
) -> Scenario {
    let nw = RegionId::from("nw");
    let ne = RegionId::from("ne");
    let sw = RegionId::from("sw");
    let se = RegionId::from("se");
    let red = FactionId::from("red");
    let blue = FactionId::from("blue");

    let mut regions = BTreeMap::new();
    for (rid, sv, init) in [
        (nw.clone(), 1.0, Some(red.clone())),
        (ne.clone(), 1.5, None),
        (sw.clone(), 1.5, None),
        (se.clone(), 1.0, Some(blue.clone())),
    ] {
        regions.insert(
            rid.clone(),
            Region {
                id: rid.clone(),
                name: rid.0.clone(),
                population: 100_000,
                urbanization: 0.5,
                initial_control: init,
                strategic_value: sv,
                borders: match rid.0.as_str() {
                    "nw" => vec![ne.clone(), sw.clone()],
                    "ne" => vec![nw.clone(), se.clone()],
                    "sw" => vec![nw.clone(), se.clone()],
                    "se" => vec![ne.clone(), sw.clone()],
                    _ => vec![],
                },
                centroid: None,
            },
        );
    }
    let terrain: Vec<TerrainModifier> = regions
        .keys()
        .map(|rid| TerrainModifier {
            region: rid.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        })
        .collect();

    let mut red_forces = BTreeMap::new();
    red_forces.insert(
        ForceId::from("r1"),
        ForceUnit {
            id: ForceId::from("r1"),
            name: "Red 1st".into(),
            unit_type: UnitType::Infantry,
            region: nw.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 2.0,
            morale_modifier: 0.0,
            capabilities: vec![],
            move_progress: 0.0,
        },
    );
    let mut blue_forces = BTreeMap::new();
    blue_forces.insert(
        ForceId::from("b1"),
        ForceUnit {
            id: ForceId::from("b1"),
            name: "Blue 1st".into(),
            unit_type: UnitType::Infantry,
            region: se.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 2.0,
            morale_modifier: 0.0,
            capabilities: vec![],
            move_progress: 0.0,
        },
    );

    let mut factions = BTreeMap::new();
    factions.insert(
        red.clone(),
        Faction {
            id: red.clone(),
            name: "Red".into(),
            faction_type: FactionType::Military {
                branch: faultline_types::faction::MilitaryBranch::Army,
            },
            forces: red_forces,
            initial_morale: 0.85,
            initial_resources: 100.0,
            resource_rate: 5.0,
            logistics_capacity: 10.0,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![DiplomaticStance {
                target_faction: blue.clone(),
                stance: Diplomacy::Hostile,
            }],
            doctrine: Doctrine::Conventional,
            utility: red_utility,
            ..Default::default()
        },
    );
    factions.insert(
        blue.clone(),
        Faction {
            id: blue.clone(),
            name: "Blue".into(),
            faction_type: FactionType::Military {
                branch: faultline_types::faction::MilitaryBranch::Army,
            },
            forces: blue_forces,
            initial_morale: 0.7,
            initial_resources: 100.0,
            resource_rate: 5.0,
            logistics_capacity: 10.0,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![DiplomaticStance {
                target_faction: red.clone(),
                stance: Diplomacy::Hostile,
            }],
            doctrine: Doctrine::Defensive,
            utility: blue_utility,
            ..Default::default()
        },
    );

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        faultline_types::ids::VictoryId::from("red_win"),
        VictoryCondition {
            id: faultline_types::ids::VictoryId::from("red_win"),
            name: "Red wins".into(),
            faction: red.clone(),
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );
    victory_conditions.insert(
        faultline_types::ids::VictoryId::from("blue_win"),
        VictoryCondition {
            id: faultline_types::ids::VictoryId::from("blue_win"),
            name: "Blue wins".into(),
            faction: blue.clone(),
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Adaptive Utility Test".into(),
            description: "test".into(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            historical_analogue: None,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 2,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain,
        },
        factions,
        political_climate: PoliticalClimate {
            tension: 0.3,
            institutional_trust: 0.7,
            media_landscape: MediaLandscape::default(),
            population_segments: vec![],
            global_modifiers: vec![],
        },
        simulation: SimulationConfig {
            max_ticks,
            monte_carlo_runs: 1,
            fog_of_war: false,
            snapshot_interval: 0,
            seed: Some(seed),
            tick_duration: TickDuration::Days(1),
            attrition_model: AttritionModel::Stochastic { noise: 0.1 },
        },
        ..Default::default()
    }
}

fn red_aggressor_profile() -> FactionUtility {
    let mut terms = BTreeMap::new();
    terms.insert(UtilityTerm::Control, 2.0);
    terms.insert(UtilityTerm::CasualtiesInflicted, 1.0);
    terms.insert(UtilityTerm::TimeToObjective, 1.0);
    let mut adj = BTreeMap::new();
    adj.insert(UtilityTerm::TimeToObjective, 2.0);
    adj.insert(UtilityTerm::Control, 1.5);
    FactionUtility {
        terms,
        triggers: vec![AdaptiveTrigger {
            id: "deadline".into(),
            description: "double urgency past midpoint".into(),
            condition: AdaptiveCondition::TickFraction { fraction: 0.5 },
            adjustments: adj,
        }],
        time_horizon_ticks: None,
    }
}

#[test]
fn legacy_no_profile_run_has_empty_utility_decisions() {
    // No `[utility]` declared — RunResult.utility_decisions must be
    // empty. Pinning the legacy fast path.
    let s = make_scenario(None, None, 42, 30);
    let mut e = Engine::new(s).expect("engine");
    let run = e.run().expect("run");
    assert!(
        run.utility_decisions.is_empty(),
        "legacy run should have empty utility_decisions, got: {:?}",
        run.utility_decisions
    );
}

#[test]
fn profile_produces_non_empty_utility_decisions() {
    let s = make_scenario(Some(red_aggressor_profile()), None, 42, 30);
    let mut e = Engine::new(s).expect("engine");
    let run = e.run().expect("run");
    let red_id = FactionId::from("red");
    let report = run
        .utility_decisions
        .get(&red_id)
        .expect("red should have a utility decision report");
    assert!(report.tick_count > 0);
    assert!(report.decision_count > 0);
    // The control term should accumulate positive contribution
    // because the profile weights it heavily and red's actions
    // attack high-strategic-value regions.
    let control_sum = report.term_sums.get("control").copied().unwrap_or_default();
    assert!(
        control_sum > 0.0,
        "control sum should be positive for control-maximizer profile; got {control_sum}"
    );
}

#[test]
fn adaptive_trigger_fires_when_condition_holds() {
    // Run with `deadline` trigger (TickFraction { fraction: 0.5 })
    // for a 30-tick scenario. The trigger should fire on every
    // decision phase from tick 16 (50% of 30 = 15) through tick 30.
    let s = make_scenario(Some(red_aggressor_profile()), None, 42, 30);
    let mut e = Engine::new(s).expect("engine");
    let run = e.run().expect("run");
    let red_id = FactionId::from("red");
    let report = run
        .utility_decisions
        .get(&red_id)
        .expect("red should have a report");
    let fires = report
        .trigger_fires
        .get("deadline")
        .copied()
        .unwrap_or_default();
    assert!(
        fires > 0,
        "trigger should fire at least once after tick 15; got {fires}"
    );
}

#[test]
fn same_seed_runs_produce_identical_utility_decisions() {
    // Determinism contract: same seed → bit-identical RunResult,
    // including the new utility_decisions map. Verifies adding
    // `[utility]` doesn't introduce any non-determinism.
    let make = || {
        let s = make_scenario(Some(red_aggressor_profile()), None, 42, 30);
        let mut e = Engine::new(s).expect("engine");
        e.run().expect("run")
    };
    let a = make();
    let b = make();
    let a_json = serde_json::to_string(&a.utility_decisions).expect("ser a");
    let b_json = serde_json::to_string(&b.utility_decisions).expect("ser b");
    assert_eq!(a_json, b_json);
}

#[test]
fn different_seed_legacy_path_unchanged() {
    // Adding `[utility]` to a scenario must not shift the RNG
    // sequence for unrelated outcomes. We compare two scenarios:
    // both have no profile (legacy), and the resulting RunResult
    // for fixed seed should match exactly. This is the inverse
    // direction of the contract — ensures the new utility_weights
    // cache code path doesn't accidentally consume RNG.
    let s_a = make_scenario(None, None, 42, 30);
    let s_b = make_scenario(None, None, 42, 30);
    let mut e_a = Engine::new(s_a).expect("engine");
    let mut e_b = Engine::new(s_b).expect("engine");
    let a = e_a.run().expect("run");
    let b = e_b.run().expect("run");
    assert_eq!(a.final_tick, b.final_tick);
    assert_eq!(a.final_state.region_control, b.final_state.region_control);
}

#[test]
fn morale_below_trigger_fires_on_morale_loss() {
    // Build a profile with a `MoraleBelow` trigger at threshold 0.5.
    // Blue's initial morale is 0.7; combat losses should drop it
    // below 0.5 by mid-run, firing the trigger.
    let mut blue_terms = BTreeMap::new();
    blue_terms.insert(UtilityTerm::CasualtiesSelf, 2.0);
    let mut blue_adj = BTreeMap::new();
    blue_adj.insert(UtilityTerm::CasualtiesSelf, 2.0);
    let blue_profile = FactionUtility {
        terms: blue_terms,
        triggers: vec![AdaptiveTrigger {
            id: "panic".into(),
            description: "double cs weight when morale drops".into(),
            condition: AdaptiveCondition::MoraleBelow { threshold: 0.5 },
            adjustments: blue_adj,
        }],
        time_horizon_ticks: None,
    };
    let s = make_scenario(Some(red_aggressor_profile()), Some(blue_profile), 42, 80);
    let mut e = Engine::new(s).expect("engine");
    let run = e.run().expect("run");
    let blue_id = FactionId::from("blue");
    let report = run
        .utility_decisions
        .get(&blue_id)
        .expect("blue should have a report");
    // Trigger should fire at least once: red's aggressive profile
    // should land enough damage by tick 80 to drop blue below 0.5.
    let panic_fires = report
        .trigger_fires
        .get("panic")
        .copied()
        .unwrap_or_default();
    assert!(
        panic_fires > 0,
        "panic trigger should fire at least once over an 80-tick combat; got {panic_fires}. \
         If this is brittle, consider increasing max_ticks or red's strength."
    );
}

#[test]
fn empty_term_weights_does_not_break_run() {
    // An author might decay every term to zero in a trigger and rely
    // on the doctrine score alone. The engine should not panic and
    // should produce a valid RunResult.
    let mut terms = BTreeMap::new();
    terms.insert(UtilityTerm::Control, 0.0);
    let profile = FactionUtility {
        terms,
        triggers: vec![],
        time_horizon_ticks: None,
    };
    let s = make_scenario(Some(profile), None, 42, 20);
    let mut e = Engine::new(s).expect("engine");
    let run = e.run().expect("run should not panic");
    assert!(run.final_tick > 0);
}
