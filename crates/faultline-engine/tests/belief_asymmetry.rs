//! Integration tests for the belief-asymmetry phase (Epic M round-one).
//!
//! These tests pin the contract of:
//! - the legacy fast path (no `belief_model` → empty belief state,
//!   identical engine output to pre-Epic-M);
//! - belief initialization at engine startup;
//! - per-tick observation refresh + decay;
//! - `EventEffect::DeceptionOp` planting false beliefs;
//! - `EventEffect::IntelligenceShare` planting truthful beliefs;
//! - belief-source tagging through decay;
//! - cross-run `belief_accuracy` aggregation;
//! - determinism across same-seed runs.

use std::collections::BTreeMap;

use faultline_engine::{Engine, validate_scenario};
use faultline_types::belief::{
    BeliefModelConfig, BeliefSource, DeceptionPayload, IntelligencePayload,
};
use faultline_types::events::{EventDefinition, EventEffect};
use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId, VictoryId};
use faultline_types::map::{
    EnvironmentSchedule, MapConfig, MapSource, Region, TerrainModifier, TerrainType,
};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

fn region(id: &str, controller: Option<&str>, borders: Vec<&str>) -> (RegionId, Region) {
    let rid = RegionId::from(id);
    (
        rid.clone(),
        Region {
            id: rid,
            name: id.into(),
            population: 100_000,
            urbanization: 0.5,
            initial_control: controller.map(FactionId::from),
            strategic_value: 1.0,
            borders: borders.into_iter().map(RegionId::from).collect(),
            centroid: None,
        },
    )
}

fn terrain(rid: &str) -> TerrainModifier {
    TerrainModifier {
        region: RegionId::from(rid),
        terrain_type: TerrainType::Rural,
        movement_modifier: 1.0,
        defense_modifier: 1.0,
        visibility: 1.0,
    }
}

fn force(id: &str, region_id: &str, strength: f64) -> ForceUnit {
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: RegionId::from(region_id),
        strength,
        mobility: 1.0,
        force_projection: None,
        upkeep: 1.0,
        morale_modifier: 0.0,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn faction(id: &str, region_id: &str, force_id: &str, strength: f64) -> (FactionId, Faction) {
    let fid = FactionId::from(id);
    let mut forces = BTreeMap::new();
    let force_unit = force(force_id, region_id, strength);
    forces.insert(force_unit.id.clone(), force_unit);
    (
        fid.clone(),
        Faction {
            id: fid,
            name: id.into(),
            faction_type: FactionType::Insurgent,
            description: String::new(),
            color: "#000000".into(),
            forces,
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 100.0,
            initial_resources: 100.0,
            resource_rate: 5.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
            leadership: None,
            alliance_fracture: None,
            utility: None,
        },
    )
}

fn base_scenario(belief_enabled: bool) -> Scenario {
    let mut regions = BTreeMap::new();
    let (r1, r1d) = region("alpha_home", Some("alpha"), vec!["bravo_home"]);
    let (r2, r2d) = region("bravo_home", Some("bravo"), vec!["alpha_home"]);
    regions.insert(r1, r1d);
    regions.insert(r2, r2d);

    let mut factions = BTreeMap::new();
    let (afid, afaction) = faction("alpha", "alpha_home", "alpha_inf", 100.0);
    let (bfid, bfaction) = faction("bravo", "bravo_home", "bravo_inf", 100.0);
    factions.insert(afid.clone(), afaction);
    factions.insert(bfid.clone(), bfaction);

    let belief_model = if belief_enabled {
        Some(BeliefModelConfig {
            enabled: true,
            ..Default::default()
        })
    } else {
        None
    };

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("alpha_win"),
        VictoryCondition {
            id: VictoryId::from("alpha_win"),
            name: "Alpha control".into(),
            faction: FactionId::from("alpha"),
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );
    victory_conditions.insert(
        VictoryId::from("bravo_win"),
        VictoryCondition {
            id: VictoryId::from("bravo_win"),
            name: "Bravo control".into(),
            faction: FactionId::from("bravo"),
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            schema_version: 1,
            name: "Belief test".into(),
            description: "test".into(),
            author: "test".into(),
            version: "0.0.1".into(),
            tags: vec![],
            confidence: None,
            historical_analogue: None,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![terrain("alpha_home"), terrain("bravo_home")],
        },
        factions,
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.3,
            institutional_trust: 0.7,
            population_segments: vec![],
            global_modifiers: vec![],
            media_landscape: MediaLandscape {
                fragmentation: 0.3,
                disinformation_susceptibility: 0.2,
                state_control: 0.1,
                social_media_penetration: 0.5,
                internet_availability: 0.9,
            },
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks: 30,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(42),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
            belief_model,
        },
        victory_conditions,
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: EnvironmentSchedule { windows: vec![] },
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: BTreeMap::new(),
    }
}

fn add_event(scenario: &mut Scenario, eid: &str, tick: u32, effects: Vec<EventEffect>) {
    use faultline_types::events::EventCondition;
    scenario.events.insert(
        EventId::from(eid),
        EventDefinition {
            id: EventId::from(eid),
            name: eid.into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::TickAtLeast { tick }],
            probability: 1.0,
            repeatable: false,
            effects,
            chain: None,
            defender_options: vec![],
        },
    );
}

#[test]
fn legacy_fast_path_when_belief_disabled() {
    // No belief model declared → engine state stays empty across run.
    let scenario = base_scenario(false);
    let mut engine = Engine::new(scenario).expect("engine init");
    let result = engine.run().expect("run");
    assert!(
        result.belief_accuracy.is_empty(),
        "no belief data when disabled"
    );
    assert!(result.belief_snapshots.is_empty());
}

#[test]
fn belief_init_populates_self_observable_state() {
    // With belief mode enabled, every faction starts with a belief
    // entry at tick 0 covering everything visible at that tick.
    let scenario = base_scenario(true);
    let engine = Engine::new(scenario).expect("engine init");
    let belief_states = &engine.state().belief_states;
    assert_eq!(belief_states.len(), 2, "one belief per faction");
    let alpha_belief = belief_states
        .get(&FactionId::from("alpha"))
        .expect("alpha belief");
    // Alpha's home region is visible to alpha at tick 0.
    assert!(
        alpha_belief
            .regions
            .contains_key(&RegionId::from("alpha_home")),
        "own region visible"
    );
    // Bravo's home is adjacent → also visible.
    assert!(
        alpha_belief
            .regions
            .contains_key(&RegionId::from("bravo_home")),
        "adjacent region visible"
    );
}

#[test]
fn deception_lands_in_target_belief() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "deceive_bravo",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseForceStrength {
                force: ForceId::from("phantom_unit"),
                owner: FactionId::from("alpha"),
                region: RegionId::from("alpha_home"),
                false_strength: 999.0,
            },
        }],
    );
    let mut engine = Engine::new(scenario).expect("engine init");
    // Run a few ticks past the event's earliest firing.
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    let bravo_belief = engine
        .state()
        .belief_states
        .get(&FactionId::from("bravo"))
        .expect("bravo belief");
    let phantom = bravo_belief
        .forces
        .get(&ForceId::from("phantom_unit"))
        .expect("phantom belief was planted");
    assert_eq!(phantom.estimated_strength, 999.0);
    assert_eq!(phantom.source, BeliefSource::Deceived);
}

#[test]
fn intelligence_share_lands_with_truthful_tag() {
    let mut scenario = base_scenario(true);
    // Bravo gets ground-truth Alpha morale.
    add_event(
        &mut scenario,
        "intel_share",
        2,
        vec![EventEffect::IntelligenceShare {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: IntelligencePayload::FactionMorale {
                faction: FactionId::from("alpha"),
            },
        }],
    );
    let mut engine = Engine::new(scenario).expect("engine init");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    let bravo_belief = engine
        .state()
        .belief_states
        .get(&FactionId::from("bravo"))
        .expect("bravo belief");
    let alpha_morale_belief = bravo_belief
        .faction_morale
        .get(&FactionId::from("alpha"))
        .expect("alpha morale belief");
    // The source tag is DirectObservation (truthful intel) — but the
    // belief might have been further refreshed by the belief phase
    // since the event fired. Either way, the source is *not*
    // Deceived.
    assert_ne!(alpha_morale_belief.source, BeliefSource::Deceived);
}

#[test]
fn deception_counter_increments_after_op() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "deceive",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseFactionMorale {
                faction: FactionId::from("bravo"),
                false_morale: 0.95,
            },
        }],
    );
    let mut engine = Engine::new(scenario).expect("engine init");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    let counter = engine
        .state()
        .belief_counters
        .get(&FactionId::from("bravo"))
        .expect("bravo counter");
    assert!(counter.deception_events_received >= 1);
}

#[test]
fn belief_accuracy_report_collected_in_run_result() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "fool_bravo",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseForceStrength {
                force: ForceId::from("phantom"),
                owner: FactionId::from("alpha"),
                region: RegionId::from("alpha_home"),
                false_strength: 500.0,
            },
        }],
    );
    let mut engine = Engine::new(scenario).expect("engine init");
    let result = engine.run().expect("run");
    let bravo_report = result
        .belief_accuracy
        .get(&FactionId::from("bravo"))
        .expect("bravo report");
    assert!(bravo_report.deception_events_received >= 1);
    assert!(
        bravo_report.force_belief_ticks > 0,
        "bravo had at least one tick of force belief"
    );
}

#[test]
fn determinism_across_same_seed_with_belief() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "fool_bravo",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseForceStrength {
                force: ForceId::from("phantom"),
                owner: FactionId::from("alpha"),
                region: RegionId::from("alpha_home"),
                false_strength: 500.0,
            },
        }],
    );
    let mut engine_a = Engine::new(scenario.clone()).expect("init a");
    let result_a = engine_a.run().expect("run a");
    let mut engine_b = Engine::new(scenario).expect("init b");
    let result_b = engine_b.run().expect("run b");
    let json_a = serde_json::to_string(&result_a).expect("ser a");
    let json_b = serde_json::to_string(&result_b).expect("ser b");
    assert_eq!(json_a, json_b, "same seed must produce same RunResult");
}

#[test]
fn belief_decay_marks_unrefreshed_entries_stale() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "fool_bravo",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("alpha"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseFactionResources {
                faction: FactionId::from("alpha"),
                false_resources: 10000.0,
            },
        }],
    );
    let mut engine = Engine::new(scenario).expect("init");
    for _ in 0..10 {
        engine.tick().expect("tick");
    }
    let bravo_belief = engine
        .state()
        .belief_states
        .get(&FactionId::from("bravo"))
        .expect("bravo belief");
    // Resources scalar belief: deceived sticks across decay.
    let alpha_res_belief = bravo_belief
        .faction_resources
        .get(&FactionId::from("alpha"))
        .expect("alpha resource belief");
    assert_eq!(alpha_res_belief.source, BeliefSource::Deceived);
    // Confidence has decayed below 1.0.
    assert!(alpha_res_belief.confidence < 1.0);
    assert!(alpha_res_belief.confidence > 0.0);
}

#[test]
fn validation_rejects_unknown_faction_in_deception() {
    let mut scenario = base_scenario(true);
    add_event(
        &mut scenario,
        "bad_event",
        2,
        vec![EventEffect::DeceptionOp {
            source_faction: FactionId::from("ghost"),
            target_faction: FactionId::from("bravo"),
            payload: DeceptionPayload::FalseFactionMorale {
                faction: FactionId::from("bravo"),
                false_morale: 0.5,
            },
        }],
    );
    assert!(validate_scenario(&scenario).is_err());
}

#[test]
fn validation_rejects_belief_model_invalid_decay() {
    let mut scenario = base_scenario(true);
    if let Some(cfg) = scenario.simulation.belief_model.as_mut() {
        cfg.force_decay_per_tick = 2.0;
    }
    assert!(validate_scenario(&scenario).is_err());
}

#[test]
fn snapshot_interval_captures_stream() {
    let mut scenario = base_scenario(true);
    if let Some(cfg) = scenario.simulation.belief_model.as_mut() {
        cfg.snapshot_interval = 3;
    }
    let mut engine = Engine::new(scenario).expect("init");
    let result = engine.run().expect("run");
    let bravo_snaps = result
        .belief_snapshots
        .get(&FactionId::from("bravo"))
        .expect("bravo snapshots");
    // Run is 30 ticks; with interval 3 we should have at least 10 snapshots.
    assert!(
        bravo_snaps.len() >= 10,
        "expected ≥10 snapshots, got {}",
        bravo_snaps.len()
    );
}

#[test]
fn snapshot_zero_interval_means_no_stream() {
    let scenario = base_scenario(true);
    let mut engine = Engine::new(scenario).expect("init");
    let result = engine.run().expect("run");
    assert!(
        result.belief_snapshots.is_empty(),
        "default snapshot_interval = 0 → empty stream"
    );
}

#[test]
fn legacy_scenario_run_result_has_no_belief_data() {
    let scenario = base_scenario(false);
    let mut engine = Engine::new(scenario).expect("init");
    let result = engine.run().expect("run");
    let json = serde_json::to_string(&result).expect("ser");
    // Confirms the `skip_serializing_if` guards are doing their job.
    // belief_accuracy / belief_snapshots elide entirely from the
    // serialized form — required by the determinism contract for
    // every legacy scenario.
    assert!(!json.contains("belief_accuracy"));
    assert!(!json.contains("belief_snapshots"));
}
