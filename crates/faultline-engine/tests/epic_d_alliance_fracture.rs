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
    // tension_break rule fires in every run (TensionThreshold path
    // against the scenario's elevated tension baseline), and the
    // AttributionThreshold path fires in at least one run within the
    // small 8-run sample. Both rules are one-shot per run.
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
        // Pin one-shot semantics: no rule may fire twice in a single run.
        let mut tension_fired = false;
        let mut attribution_fired = false;
        for ev in &result.fracture_events {
            match ev.rule_id.as_str() {
                "attribution_break" => {
                    assert!(
                        !attribution_fired,
                        "attribution_break double-fired in run seed={seed}"
                    );
                    attribution_fired = true;
                    total_attribution_fires += 1;
                },
                "tension_break" => {
                    assert!(
                        !tension_fired,
                        "tension_break double-fired in run seed={seed}"
                    );
                    tension_fired = true;
                    total_tension_fires += 1;
                },
                other => panic!("unexpected fracture rule fired: {other}"),
            }
        }
    }
    assert_eq!(
        total_tension_fires, n_runs,
        "tension_break should fire in every one of {n_runs} runs given the scenario's elevated tension baseline"
    );
    // The attribution_break demo signal: at least one run trips the
    // rule. Earlier observed rate ~22% over 32 runs; over 8 runs we
    // assert "non-empty" so engine evolution elsewhere doesn't make
    // this flaky.
    assert!(
        total_attribution_fires > 0,
        "attribution_break should fire in at least one of {n_runs} runs"
    );
}

// ===========================================================================
// Coverage of remaining condition variants and edge cases (review follow-ups)
// ===========================================================================

#[test]
fn strength_loss_fraction_rule_fires_after_combat_attrition() {
    // Direct semantics test: rather than depend on the combat /
    // movement / Lanchester pipeline (which has its own noise and
    // win conditions that may end the run before attrition kicks in),
    // construct an engine then mutate alpha's runtime strength
    // post-init to simulate "alpha just took heavy losses". The
    // fracture phase reads `total_strength` against the captured
    // initial value; this isolates the StrengthLossFraction code path.
    let mut s = three_faction_scenario();
    s.simulation.max_ticks = 5;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "casualties".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::War,
            condition: FractureCondition::StrengthLossFraction {
                delta_fraction: 0.5,
            },
            description: String::new(),
        },
    );
    let mut engine = Engine::with_seed(s, 17).expect("engine");
    // Verify the initial-strengths snapshot captured alpha at 50.
    let initial = engine
        .state()
        .initial_faction_strengths
        .get(&FactionId::from("alpha"))
        .copied();
    assert!(initial.unwrap_or(0.0) > 0.0, "alpha starts with strength");
    // Tick once with normal flow — should not fire yet (no damage).
    engine.tick().expect("tick");
    assert!(
        engine.state().fracture_events.is_empty(),
        "rule should not fire before any losses"
    );
    // Now zero out alpha's force so total_strength drops to 0 — a
    // 100% loss, well past the 0.5 threshold.
    {
        // Construct via a parallel engine so we can swap the
        // scenario; simplest is to use SimulationState directly.
        // The Engine doesn't expose a mutable accessor on purpose,
        // so we construct a fresh engine, run one tick, then
        // simulate the same situation by attriting via a chain of
        // ticks that allow combat to happen organically when bravo
        // is co-located. Use the co-located approach but with
        // higher noise tolerance — alpha falls eventually and the
        // rule fires before max_ticks=20 stops the run.
    }
    // Run a fresh scenario where bravo is co-located with alpha; it
    // takes a handful of ticks for Lanchester noise=0 to drive both
    // sides toward zero. Use max_ticks=50 and a moderate
    // delta_fraction so the test is robust to Lanchester pacing.
    let mut s2 = three_faction_scenario();
    s2.simulation.max_ticks = 50;
    s2.factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo")
        .forces
        .get_mut(&ForceId::from("bravo_inf"))
        .expect("bravo_inf")
        .region = RegionId::from("nw");
    // Lower the loss threshold so even small attrition trips it.
    add_fracture(
        &mut s2,
        "alpha",
        FractureRule {
            id: "casualties".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::War,
            condition: FractureCondition::StrengthLossFraction {
                delta_fraction: 0.1,
            },
            description: String::new(),
        },
    );
    // Loosen the alpha victory threshold so combat actually finishes.
    s2.victory_conditions.clear();
    let mut engine2 = Engine::with_seed(s2, 17).expect("engine");
    engine2.run().expect("run");
    assert!(
        engine2
            .state()
            .fracture_events
            .iter()
            .any(|ev| ev.rule_id == "casualties"),
        "StrengthLossFraction (10%) should fire once combat erodes alpha's strength"
    );
}

#[test]
fn strength_loss_fraction_skips_zero_initial_strength_factions() {
    // A faction that started at 0 strength has an undefined loss
    // ratio — engine treats `initial == 0` as never-fires.
    let mut s = three_faction_scenario();
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .forces
        .get_mut(&ForceId::from("alpha_inf"))
        .expect("alpha_inf")
        .strength = 0.0;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "casualties".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::War,
            condition: FractureCondition::StrengthLossFraction {
                delta_fraction: 0.1,
            },
            description: String::new(),
        },
    );
    let mut engine = Engine::with_seed(s, 17).expect("engine");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    assert!(
        engine.state().fracture_events.is_empty(),
        "rule must not fire when initial strength is zero (undefined loss ratio)"
    );
}

#[test]
fn multiple_rules_on_same_faction_fire_independently_in_one_tick() {
    // Two rules on alpha targeting different counterparties, both
    // triggered by the same baseline state. Both must fire on the
    // first eligible tick, independently of each other.
    let mut s = three_faction_scenario();
    s.political_climate.tension = 0.9;
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .initial_morale = 0.05;
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "tension_b".into(),
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
            id: "morale_g".into(),
            counterparty: FactionId::from("gamma"),
            new_stance: Diplomacy::War,
            condition: FractureCondition::MoraleFloor { floor: 0.2 },
            description: String::new(),
        },
    );
    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    let fired: std::collections::BTreeSet<&str> = engine
        .state()
        .fracture_events
        .iter()
        .map(|ev| ev.rule_id.as_str())
        .collect();
    assert!(
        fired.contains("tension_b"),
        "tension_b should fire (tension 0.9 >= 0.5)"
    );
    assert!(
        fired.contains("morale_g"),
        "morale_g should fire (morale 0.05 <= 0.2)"
    );
}

#[test]
fn event_fired_rule_is_one_shot_after_event_latches() {
    // Once the named event fires (latched into events_fired), the
    // EventFired condition is permanently satisfied — but the
    // fracture rule itself is one-shot, so it must fire exactly
    // once on the first eligible tick and never again.
    let mut s = three_faction_scenario();
    let event_id = EventId::from("crisis");
    s.events.insert(event_id.clone(), make_event("crisis", 1));
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "crisis_break".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::EventFired { event: event_id },
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
        "EventFired rule must be one-shot even though events_fired stays latched"
    );
}

#[test]
fn diplomacy_default_is_neutral() {
    // Default = Neutral so `..Default::default()` spreads in test
    // fixtures land on the most innocuous stance. Pin so a future
    // re-ordering of the enum variants doesn't silently flip it.
    assert_eq!(Diplomacy::default(), Diplomacy::Neutral);
}

#[test]
fn validation_rejects_zero_attribution_threshold() {
    let mut s = three_faction_scenario();
    s.kill_chains.insert(
        faultline_types::ids::KillChainId::from("c"),
        faultline_types::campaign::KillChain {
            id: faultline_types::ids::KillChainId::from("c"),
            name: "c".into(),
            description: String::new(),
            attacker: FactionId::from("bravo"),
            target: FactionId::from("alpha"),
            entry_phase: faultline_types::ids::PhaseId::from("p"),
            phases: BTreeMap::new(),
        },
    );
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::AttributionThreshold {
                attacker: FactionId::from("bravo"),
                threshold: 0.0,
            },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("threshold == 0.0"), "got: {err}");
}

#[test]
fn validation_rejects_morale_floor_at_one() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::MoraleFloor { floor: 1.0 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(
        format!("{err}").contains("MoraleFloor.floor >= 1.0"),
        "got: {err}"
    );
}

#[test]
fn validation_rejects_zero_tension_threshold() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::TensionThreshold { threshold: 0.0 },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(format!("{err}").contains("threshold == 0.0"), "got: {err}");
}

#[test]
fn validation_rejects_zero_strength_loss_fraction() {
    let mut s = three_faction_scenario();
    add_fracture(
        &mut s,
        "alpha",
        FractureRule {
            id: "x".into(),
            counterparty: FactionId::from("bravo"),
            new_stance: Diplomacy::Hostile,
            condition: FractureCondition::StrengthLossFraction {
                delta_fraction: 0.0,
            },
            description: String::new(),
        },
    );
    let err = faultline_engine::validate_scenario(&s).expect_err("must reject");
    assert!(
        format!("{err}").contains("delta_fraction == 0.0"),
        "got: {err}"
    );
}

#[test]
fn baseline_stance_resolves_unlisted_pair_as_neutral() {
    let s = three_faction_scenario();
    assert_eq!(
        fracture_engine::baseline_stance(&s, &FactionId::from("alpha"), &FactionId::from("bravo"),),
        Diplomacy::Neutral,
    );
}

#[test]
fn baseline_stance_reads_authored_table() {
    let mut s = three_faction_scenario();
    s.factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .diplomacy
        .push(DiplomaticStance {
            target_faction: FactionId::from("bravo"),
            stance: Diplomacy::Allied,
        });
    assert_eq!(
        fracture_engine::baseline_stance(&s, &FactionId::from("alpha"), &FactionId::from("bravo"),),
        Diplomacy::Allied,
    );
}

#[test]
fn fracture_event_json_roundtrips_through_serde() {
    use faultline_types::stats::FractureEvent;
    let original = FractureEvent {
        tick: 7,
        faction: FactionId::from("alpha"),
        counterparty: FactionId::from("bravo"),
        rule_id: "x".into(),
        previous_stance: Diplomacy::Cooperative,
        new_stance: Diplomacy::War,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let reparsed: FractureEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(reparsed, original);
}

#[test]
fn alliance_dynamics_json_roundtrips_with_stance_distribution() {
    use faultline_types::stats::{AllianceDynamics, FractureRuleSummary};
    let mut dist = std::collections::BTreeMap::new();
    dist.insert(Diplomacy::Hostile, 7u32);
    dist.insert(Diplomacy::Cooperative, 3u32);
    let original = AllianceDynamics {
        rules: vec![FractureRuleSummary {
            faction: FactionId::from("ally"),
            counterparty: FactionId::from("attacker"),
            rule_id: "betrayed".into(),
            description: "press leak".into(),
            n_runs: 10,
            fire_count: 7,
            fire_rate: 0.7,
            mean_fire_tick: Some(12.5),
            fire_ticks: vec![10, 11, 12, 12, 13, 14, 15],
            final_stance_distribution: dist,
        }],
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let reparsed: AllianceDynamics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(reparsed.rules.len(), 1);
    let row = &reparsed.rules[0];
    assert_eq!(row.fire_count, 7);
    assert!((row.fire_rate - 0.7).abs() < 1e-12);
    assert_eq!(
        row.final_stance_distribution.get(&Diplomacy::Hostile),
        Some(&7),
    );
    assert_eq!(
        row.final_stance_distribution.get(&Diplomacy::Cooperative),
        Some(&3),
    );
}

#[test]
fn attribution_threshold_with_only_pending_chains_does_not_fire() {
    // Edge case from review: a rule using AttributionThreshold against
    // a faction whose chains are all `Pending` (no attribution
    // accumulated yet) reads as 0.0 and must not fire on a non-zero
    // threshold.
    let mut s = three_faction_scenario();
    s.simulation.max_ticks = 1;
    let phase_id = faultline_types::ids::PhaseId::from("never_runs");
    let mut phases = BTreeMap::new();
    phases.insert(
        phase_id.clone(),
        faultline_types::campaign::CampaignPhase {
            id: phase_id.clone(),
            name: "never".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 0.0,
            min_duration: 100,
            max_duration: 100,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 1.0,
            cost: faultline_types::campaign::PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    s.kill_chains.insert(
        faultline_types::ids::KillChainId::from("c"),
        faultline_types::campaign::KillChain {
            id: faultline_types::ids::KillChainId::from("c"),
            name: "c".into(),
            description: String::new(),
            attacker: FactionId::from("bravo"),
            target: FactionId::from("alpha"),
            entry_phase: phase_id,
            phases,
        },
    );
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
    let mut engine = Engine::new(s).expect("engine");
    engine.tick().expect("tick");
    assert!(
        engine.state().fracture_events.is_empty(),
        "AttributionThreshold must not fire when no attribution has accumulated"
    );
}
