//! Integration tests for Epic D round two — alliance fracture mechanic.
//!
//! Covers schema validation rejections, the engine-side fracture phase
//! semantics (one-shot, attribution / morale / tension / event-fired
//! conditions), the `EventEffect::DiplomacyChange` wiring, and the
//! per-run `fracture_events` log.

use std::collections::BTreeMap;
use std::path::Path;

use faultline_engine::Engine;
use faultline_engine::fracture as fracture_engine;
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{
    AllianceFracture, Diplomacy, DiplomaticStance, Faction, FactionType, ForceUnit,
    FractureCondition, FractureRule, UnitType,
};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId, VictoryId};
use faultline_types::map::{
    InfrastructureNode, MapConfig, MapSource, Region, TerrainModifier, TerrainType,
};
use faultline_types::scenario::Scenario;
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

fn region(id: &str, neighbors: &[&str]) -> Region {
    Region {
        id: RegionId::from(id),
        name: id.into(),
        population: 100_000,
        urbanization: 0.5,
        initial_control: None,
        strategic_value: 0.5,
        borders: neighbors.iter().map(|n| RegionId::from(*n)).collect(),
        centroid: None,
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
    }
}

fn make_faction(id: &str, region_id: &str) -> Faction {
    let mut forces = BTreeMap::new();
    let force_id = format!("{id}_inf");
    forces.insert(
        ForceId::from(force_id.as_str()),
        force(&force_id, region_id, 50.0),
    );
    Faction {
        id: FactionId::from(id),
        name: id.into(),
        faction_type: FactionType::Civilian,
        description: String::new(),
        color: "#888888".into(),
        forces,
        tech_access: vec![],
        initial_morale: 0.7,
        logistics_capacity: 10.0,
        initial_resources: 100.0,
        resource_rate: 1.0,
        recruitment: None,
        command_resilience: 0.5,
        intelligence: 0.5,
        diplomacy: vec![],
        doctrine: Doctrine::default(),
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership: None,
        alliance_fracture: None,
    }
}

fn three_faction_scenario() -> Scenario {
    let mut regions: BTreeMap<RegionId, Region> = BTreeMap::new();
    let mut nw = region("nw", &["ne", "sw"]);
    nw.initial_control = Some(FactionId::from("alpha"));
    let mut ne = region("ne", &["nw", "se"]);
    ne.initial_control = Some(FactionId::from("bravo"));
    let mut sw = region("sw", &["nw", "se"]);
    sw.initial_control = Some(FactionId::from("gamma"));
    let se = region("se", &["ne", "sw"]);
    regions.insert(RegionId::from("nw"), nw);
    regions.insert(RegionId::from("ne"), ne);
    regions.insert(RegionId::from("sw"), sw);
    regions.insert(RegionId::from("se"), se);

    let terrain: Vec<TerrainModifier> = ["nw", "ne", "sw", "se"]
        .iter()
        .map(|r| TerrainModifier {
            region: RegionId::from(*r),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        })
        .collect();

    let mut factions = BTreeMap::new();
    factions.insert(FactionId::from("alpha"), make_faction("alpha", "nw"));
    factions.insert(FactionId::from("bravo"), make_faction("bravo", "ne"));
    factions.insert(FactionId::from("gamma"), make_faction("gamma", "sw"));

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("alpha_holds"),
        VictoryCondition {
            id: VictoryId::from("alpha_holds"),
            name: "alpha holds".into(),
            faction: FactionId::from("alpha"),
            condition: VictoryType::StrategicControl { threshold: 0.99 },
        },
    );

    Scenario {
        factions,
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 2,
            },
            regions,
            infrastructure: BTreeMap::<faultline_types::ids::InfraId, InfrastructureNode>::new(),
            terrain,
        },
        simulation: SimulationConfig {
            max_ticks: 20,
            monte_carlo_runs: 1,
            fog_of_war: false,
            snapshot_interval: 0,
            seed: Some(42),
            tick_duration: TickDuration::Days(1),
            attrition_model: AttritionModel::Stochastic { noise: 0.0 },
        },
        victory_conditions,
        ..Default::default()
    }
}

fn add_fracture(scenario: &mut Scenario, faction_id: &str, rule: FractureRule) {
    let f = scenario
        .factions
        .get_mut(&FactionId::from(faction_id))
        .expect("faction exists");
    let af = f
        .alliance_fracture
        .get_or_insert_with(AllianceFracture::default);
    af.rules.push(rule);
}

fn make_event(id: &str, fire_at_tick: u32) -> EventDefinition {
    EventDefinition {
        id: EventId::from(id),
        name: id.into(),
        description: String::new(),
        earliest_tick: Some(fire_at_tick),
        latest_tick: Some(fire_at_tick),
        conditions: vec![EventCondition::TickAtLeast { tick: fire_at_tick }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.0 }],
        chain: None,
        defender_options: vec![],
    }
}

// ===========================================================================
// Validation rejection tests
// ===========================================================================

#[test]
fn rejects_empty_alliance_fracture_block() {
    let mut s = three_faction_scenario();
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .alliance_fracture = Some(AllianceFracture { rules: vec![] });
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("empty `alliance_fracture`"));
}

#[test]
fn rejects_unknown_counterparty() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("nonexistent"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err:?}").contains("nonexistent"));
}

#[test]
fn rejects_self_targeting_rule() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("alpha"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("same faction"));
}

#[test]
fn rejects_duplicate_rule_ids() {
    let mut s = three_faction_scenario();
    let mk = || FractureRule {
        id: "dup".into(),
        counterparty: FactionId::from("bravo"),
        new_stance: Diplomacy::Hostile,
        condition: FractureCondition::TensionThreshold { threshold: 0.5 },
        description: String::new(),
    };
    add_fracture(&mut s, "alpha", mk());
    add_fracture(&mut s, "alpha", mk());
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("declared more than once"));
}

#[test]
fn rejects_empty_rule_id() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: String::new(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("empty `id`"));
}

#[test]
fn rejects_attribution_threshold_referring_to_no_chain_owner() {
    // The validator catches the silent-no-op shape where a rule
    // references an attacker that owns no kill chain.
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::AttributionThreshold {
                attacker: FactionId::from("bravo"),
                threshold: 0.5,
            },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("no kill chain"));
}

#[test]
fn rejects_out_of_range_threshold() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 1.5 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("TensionThreshold.threshold"));
}

#[test]
fn rejects_unknown_event_in_event_fired() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::EventFired {
                event: EventId::from("never_declared"),
            },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("never_declared"));
}

#[test]
fn passes_with_well_formed_alliance_fracture() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "tension_break".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: "fracture under tension".into(),
        },
    );
    faultline_engine::validate_scenario(&s).expect("well-formed scenario should validate");
}

// ===========================================================================
// Engine semantics
// ===========================================================================

#[test]
fn tension_threshold_rule_fires_when_tension_crosses() {
    let mut s = three_faction_scenario();
    s.political_climate.tension = 0.8;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "high_tension".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .diplomacy
        .push(DiplomaticStance {
            target_faction: FactionId::from("bravo"),
            stance: Diplomacy::Cooperative,
        });

    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    assert!(
        !engine.state().fracture_events.is_empty(),
        "rule should have fired"
    );
    let ev = &engine.state().fracture_events[0];
    assert_eq!(ev.rule_id, "high_tension");
    assert_eq!(ev.faction, FactionId::from("alpha"));
    assert_eq!(ev.counterparty, FactionId::from("bravo"));
    assert_eq!(ev.previous_stance, Diplomacy::Cooperative);
    assert_eq!(ev.new_stance, Diplomacy::Hostile);
    assert_eq!(
        engine
            .state()
            .diplomacy_overrides
            .get(&FactionId::from("alpha"))
            .and_then(|m| m.get(&FactionId::from("bravo"))),
        Some(&Diplomacy::Hostile)
    );
}

#[test]
fn fracture_rule_is_one_shot() {
    let mut s = three_faction_scenario();
    s.political_climate.tension = 0.8;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "high_tension".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    let mut engine = Engine::new(s).expect("engine");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    assert_eq!(
        engine.state().fracture_events.len(),
        1,
        "one-shot semantics: rule fires at most once per run"
    );
}

#[test]
fn morale_floor_rule_fires_when_morale_drops() {
    let mut s = three_faction_scenario();
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .initial_morale = 0.1;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "demoralized".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::MoraleFloor { floor: 0.2 },
            description: String::new(),
        },
    );
    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    assert!(
        !engine.state().fracture_events.is_empty(),
        "morale 0.1 <= floor 0.2 should fire"
    );
}

#[test]
fn morale_floor_rule_does_not_fire_above_floor() {
    let mut s = three_faction_scenario();
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .initial_morale = 0.9;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "demoralized".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::MoraleFloor { floor: 0.2 },
            description: String::new(),
        },
    );
    let mut engine = Engine::new(s).expect("engine");
    for _ in 0..3 {
        engine.tick().expect("tick");
    }
    assert!(
        engine.state().fracture_events.is_empty(),
        "morale above floor should not fire"
    );
}

#[test]
fn event_fired_rule_fires_after_event() {
    let mut s = three_faction_scenario();
    let event_id = EventId::from("press_leak");
    s.events
        .insert(event_id.clone(), make_event("press_leak", 1));
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "press_break".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::EventFired { event: event_id },
            description: String::new(),
        },
    );
    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    assert!(
        !engine.state().fracture_events.is_empty(),
        "EventFired rule should fire same tick as the named event"
    );
}

#[test]
fn diplomacy_change_event_writes_override() {
    let mut s = three_faction_scenario();
    let event_id = EventId::from("flip");
    let mut def = make_event("flip", 1);
    def.effects = vec![EventEffect::DiplomacyChange {
        faction_a: FactionId::from("alpha"),
        faction_b: FactionId::from("bravo"),
        new_stance: Diplomacy::War,
    }];
    s.events.insert(event_id, def);
    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    let stance = fracture_engine::current_stance(
        engine.state(),
        engine.scenario(),
        &FactionId::from("alpha"),
        &FactionId::from("bravo"),
    );
    assert_eq!(
        stance,
        Diplomacy::War,
        "DiplomacyChange should populate the override map"
    );
}

#[test]
fn legacy_scenarios_pay_zero_overhead() {
    let s = three_faction_scenario();
    let mut engine = Engine::new(s).expect("engine");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    assert!(engine.state().diplomacy_overrides.is_empty());
    assert!(engine.state().fired_fractures.is_empty());
    assert!(engine.state().fracture_events.is_empty());
}

#[test]
fn fracture_events_are_deterministic_under_fixed_seed() {
    let mut s = three_faction_scenario();
    s.simulation.max_ticks = 5;
    s.political_climate.tension = 0.6;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "tension".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.5 },
            description: String::new(),
        },
    );
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "morale".into(),
            counterparty: FactionId::from("gamma"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::MoraleFloor { floor: 0.0 },
            description: String::new(),
        },
    );

    let mut e1 = Engine::with_seed(s.clone(), 7).expect("engine");
    let r1 = e1.run().expect("run 1");
    let mut e2 = Engine::with_seed(s, 7).expect("engine");
    let r2 = e2.run().expect("run 2");
    assert_eq!(
        r1.fracture_events, r2.fracture_events,
        "fracture trajectory must be deterministic under fixed seed"
    );
}

#[test]
fn bundled_demo_scenario_produces_fracture_signal() {
    // End-to-end smoke test against the bundled scenario. Locks in
    // the analytical signal the demo was built to surface: the
    // tension_break rule fires in *all* runs (proving the
    // TensionThreshold path works against this scenario's elevated
    // tension), and the AttributionThreshold path doesn't crash
    // even when no run trips it.
    let scenario_str = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/coalition_fracture_demo.toml"),
    )
    .expect("read demo scenario");
    let loaded =
        faultline_types::migration::load_scenario_str(&scenario_str).expect("parse demo scenario");
    let scenario = loaded.scenario;
    faultline_engine::validate_scenario(&scenario).expect("demo scenario validates");

    let mut total_attribution_fires: u64 = 0;
    let mut total_tension_fires: u64 = 0;
    let n_runs: u64 = 8;
    for seed in 0..n_runs {
        let mut engine = Engine::with_seed(scenario.clone(), seed).expect("engine");
        let result = engine.run().expect("run");
        for ev in &result.fracture_events {
            match ev.rule_id.as_str() {
                "attribution_break" => total_attribution_fires += 1,
                "tension_break" => total_tension_fires += 1,
                other => panic!("unexpected fracture rule fired: {other}"),
            }
        }
    }
    assert!(
        total_tension_fires > 0,
        "tension_break should fire in at least one of {n_runs} runs"
    );
    assert!(
        total_attribution_fires <= n_runs,
        "attribution_break is one-shot per run"
    );
}
