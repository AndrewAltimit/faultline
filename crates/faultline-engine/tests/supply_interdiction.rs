//! Integration tests for the supply-network interdiction phase.
//!
//! Pin the high-leverage observable behaviors:
//! - a faction with no owned supply network sees `pressure = 1.0`
//!   (legacy contract — every existing scenario must be unchanged);
//! - a pristine supply network yields `pressure = 1.0`;
//! - severing an edge drops pressure proportionally;
//! - the owning faction's per-tick income is multiplied by pressure;
//! - validation rejects `kind = "supply"` without an `owner`;
//! - same-seed runs produce bit-identical supply-pressure reports
//!   (the determinism contract `--verify` depends on).

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{Faction, FactionType, ForceUnit, MilitaryBranch, UnitType};
use faultline_types::ids::{
    EdgeId, EventId, FactionId, ForceId, NetworkId, NodeId, RegionId, VictoryId,
};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::network::{Network, NetworkEdge, NetworkNode};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_region(id: &str, borders: Vec<RegionId>) -> Region {
    Region {
        id: RegionId::from(id),
        name: id.into(),
        population: 100_000,
        urbanization: 0.5,
        initial_control: Some(FactionId::from("blue")),
        strategic_value: 1.0,
        borders,
        centroid: None,
    }
}

fn make_force(id: &str, region: &RegionId, strength: f64) -> ForceUnit {
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: region.clone(),
        strength,
        mobility: 1.0,
        force_projection: None,
        upkeep: 1.0,
        morale_modifier: 0.0,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn make_blue() -> Faction {
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("garrison"),
        make_force("garrison", &RegionId::from("r1"), 100.0),
    );
    Faction {
        id: FactionId::from("blue"),
        name: "Blue".into(),
        faction_type: FactionType::Military {
            branch: MilitaryBranch::Army,
        },
        description: String::new(),
        color: "#000".into(),
        forces,
        tech_access: vec![],
        initial_morale: 0.8,
        logistics_capacity: 50.0,
        // High initial pool so income deltas don't get clobbered by
        // upkeep zeroing out the pool early in the run.
        initial_resources: 10_000.0,
        // Tunable resource_rate per test — overridden as needed.
        resource_rate: 100.0,
        recruitment: None,
        command_resilience: 0.0,
        intelligence: 0.5,
        diplomacy: vec![],
        doctrine: Doctrine::Conventional,
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership: None,
        alliance_fracture: None,
        utility: None,
    }
}

fn empty_scenario(seed: u64, max_ticks: u32) -> Scenario {
    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");
    let mut regions = BTreeMap::new();
    regions.insert(r1.clone(), make_region("r1", vec![r2.clone()]));
    regions.insert(r2.clone(), make_region("r2", vec![r1.clone()]));

    let mut factions = BTreeMap::new();
    factions.insert(FactionId::from("blue"), make_blue());
    // Add a passive second faction so victory_check's "last faction
    // standing" path doesn't end the run on tick 1. This faction owns
    // a token force in r2 (which it controls — initial_control = blue
    // sets blue as the controller, but a force in the region keeps
    // the second faction alive). The two factions never engage
    // because they're not in the same region — combat needs co-located
    // forces.
    let mut grey_forces = BTreeMap::new();
    grey_forces.insert(
        ForceId::from("grey_unit"),
        make_force("grey_unit", &RegionId::from("r2"), 1.0),
    );
    let mut grey = make_blue();
    grey.id = FactionId::from("grey");
    grey.name = "Grey".into();
    grey.forces = grey_forces;
    grey.initial_resources = 0.0;
    grey.resource_rate = 0.0;
    factions.insert(FactionId::from("grey"), grey);

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("hold"),
        VictoryCondition {
            id: VictoryId::from("hold"),
            name: "Hold".into(),
            faction: FactionId::from("blue"),
            // Set duration > max_ticks so victory never triggers and
            // the scenario runs the full tick budget — gives every
            // test the same number of attrition phases to observe.
            condition: VictoryType::HoldRegions {
                regions: vec![r1.clone(), r2.clone()],
                duration: max_ticks + 1,
            },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "supply test".into(),
            description: String::new(),
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
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![
                TerrainModifier {
                    region: r1,
                    terrain_type: TerrainType::Rural,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                },
                TerrainModifier {
                    region: r2,
                    terrain_type: TerrainType::Rural,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                },
            ],
        },
        factions,
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.0,
            institutional_trust: 0.5,
            media_landscape: MediaLandscape {
                fragmentation: 0.0,
                disinformation_susceptibility: 0.0,
                state_control: 0.0,
                social_media_penetration: 0.0,
                internet_availability: 0.0,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(seed),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
            belief_model: None,
        },
        victory_conditions,
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: BTreeMap::new(),
    }
}

/// Build a single-edge supply network owned by `blue` with capacity
/// `cap`. Two-node `a -> b` topology so disruption / capacity events
/// can target either endpoint.
fn supply_network(cap: f64) -> Network {
    let mut nodes = BTreeMap::new();
    for id in ["a", "b"] {
        nodes.insert(
            NodeId::from(id),
            NetworkNode {
                id: NodeId::from(id),
                name: id.into(),
                ..Default::default()
            },
        );
    }
    let mut edges = BTreeMap::new();
    edges.insert(
        EdgeId::from("ab"),
        NetworkEdge {
            id: EdgeId::from("ab"),
            from: NodeId::from("a"),
            to: NodeId::from("b"),
            capacity: cap,
            ..Default::default()
        },
    );
    Network {
        id: NetworkId::from("supply"),
        name: "Supply".into(),
        kind: "supply".into(),
        owner: Some(FactionId::from("blue")),
        nodes,
        edges,
        ..Default::default()
    }
}

fn cut_edge_event(at_tick: u32, factor: f64) -> EventDefinition {
    EventDefinition {
        id: EventId::from("cut"),
        name: "Cut".into(),
        description: "".into(),
        earliest_tick: Some(at_tick),
        latest_tick: Some(at_tick),
        conditions: vec![EventCondition::TickAtLeast { tick: at_tick }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkEdgeCapacity {
            network: NetworkId::from("supply"),
            edge: EdgeId::from("ab"),
            factor,
        }],
        chain: None,
        defender_options: vec![],
    }
}

// ---------------------------------------------------------------------------
// Behavior tests
// ---------------------------------------------------------------------------

#[test]
fn legacy_scenario_with_no_supply_network_sees_pressure_one() {
    // No `[networks.*]` declared at all. Run a short simulation and
    // verify the per-faction supply_pressure_reports map is empty —
    // legacy scenarios must produce no entry, so the report section
    // elides cleanly. This is the zero-overhead contract for every
    // scenario predating the supply-interdiction phase.
    let scenario = empty_scenario(7, 5);
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");
    assert!(
        result.supply_pressure_reports.is_empty(),
        "no networks → no supply-pressure reports; got: {:?}",
        result.supply_pressure_reports
    );
}

#[test]
fn pristine_supply_network_yields_full_pressure() {
    // A supply network with no interdiction events should keep
    // pressure at 1.0 every tick. The report is emitted (the faction
    // owns a supply network) but min/mean should both be 1.0.
    let mut scenario = empty_scenario(11, 5);
    scenario
        .networks
        .insert(NetworkId::from("supply"), supply_network(50.0));
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");
    let report = result
        .supply_pressure_reports
        .get(&FactionId::from("blue"))
        .expect("blue should have a supply-pressure report");
    assert!(
        (report.mean_pressure - 1.0).abs() < 1e-9,
        "pristine network → mean = 1.0; got {}",
        report.mean_pressure
    );
    assert!(
        (report.min_pressure - 1.0).abs() < 1e-9,
        "pristine network → min = 1.0; got {}",
        report.min_pressure
    );
    assert_eq!(
        report.pressured_ticks, 0,
        "pristine network → 0 pressured ticks; got {}",
        report.pressured_ticks
    );
}

#[test]
fn severed_edge_drops_pressure_proportionally() {
    // Cut the single edge to factor 0.5 on tick 2. From tick 2
    // onwards the residual is 50% of baseline → pressure 0.5.
    // Ticks 1 sees pressure 1.0; ticks 2..=5 see 0.5.
    let mut scenario = empty_scenario(13, 5);
    scenario
        .networks
        .insert(NetworkId::from("supply"), supply_network(50.0));
    scenario
        .events
        .insert(EventId::from("cut"), cut_edge_event(2, 0.5));
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");
    let report = result
        .supply_pressure_reports
        .get(&FactionId::from("blue"))
        .expect("blue should have a supply-pressure report");
    // Mean across 5 attrition ticks: (1.0 + 0.5 + 0.5 + 0.5 + 0.5) / 5 = 0.6
    assert!(
        (report.mean_pressure - 0.6).abs() < 1e-9,
        "expected mean_pressure ≈ 0.6 with one tick at 1.0 and four at 0.5; got {}",
        report.mean_pressure
    );
    assert!(
        (report.min_pressure - 0.5).abs() < 1e-9,
        "expected min_pressure = 0.5; got {}",
        report.min_pressure
    );
    assert_eq!(
        report.pressured_ticks, 4,
        "ticks 2..=5 are below threshold 0.9; got {}",
        report.pressured_ticks
    );
}

#[test]
fn full_severance_zeroes_income_for_owner() {
    // Cut to factor 0.0 on tick 1. Income from tick 1 onward is
    // multiplied by 0.0; only the initial resource pool remains.
    // Compare resource trajectory between (a) no supply network and
    // (b) supply network with full severance.
    let scenario_a = empty_scenario(17, 4);
    let mut scenario_b = scenario_a.clone();

    // Both arms: rate = 100 per tick, upkeep = 1.0 from one force.
    // Arm A: no networks → income = 100 * 4 = 400 added on top of 10,000.
    let mut engine_a = Engine::new(scenario_a.clone()).expect("validate");
    let result_a = engine_a.run().expect("run");
    let final_resources_a = result_a
        .final_state
        .faction_states
        .get(&FactionId::from("blue"))
        .map(|fs| fs.resources)
        .expect("blue state should exist");

    // Arm B: supply network severed at tick 1 (pressure = 0.0 from t=1).
    scenario_b
        .networks
        .insert(NetworkId::from("supply"), supply_network(50.0));
    scenario_b
        .events
        .insert(EventId::from("cut"), cut_edge_event(1, 0.0));
    let mut engine_b = Engine::new(scenario_b).expect("validate");
    let result_b = engine_b.run().expect("run");
    let final_resources_b = result_b
        .final_state
        .faction_states
        .get(&FactionId::from("blue"))
        .map(|fs| fs.resources)
        .expect("blue state should exist");

    // Reference: with no severance and `resource_rate = 100`, four
    // ticks of attrition add ~400 to the pool; with full severance,
    // arm B adds nothing (and pays upkeep). So arm B must end with
    // strictly fewer resources than arm A.
    assert!(
        final_resources_b < final_resources_a,
        "severed supply must reduce final resources: arm A = {final_resources_a}, arm B = {final_resources_b}",
    );

    // And quantify it: arm A added ≈ resource_rate * max_ticks - upkeep
    // ≈ 100*4 - 1*4 = 396 to the initial 10_000; arm B added 0 - 1*4
    // = -4 to the initial 10_000. So the gap is ~400. Allow slack for
    // any incidental accounting.
    let gap = final_resources_a - final_resources_b;
    assert!(
        gap >= 350.0,
        "expected income gap ≥ 350 (rate=100/tick × 4 ticks); got {gap}",
    );
}

// ---------------------------------------------------------------------------
// Validation tests
// ---------------------------------------------------------------------------

#[test]
fn validation_rejects_kind_supply_without_owner() {
    let mut scenario = empty_scenario(0, 1);
    let mut net = supply_network(10.0);
    // Strip the owner — should now be rejected by validation.
    net.owner = None;
    scenario.networks.insert(NetworkId::from("supply"), net);
    let err = faultline_engine::validate_scenario(&scenario)
        .expect_err("kind=supply without owner must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("supply") && msg.contains("owner"),
        "error should mention supply + owner; got: {msg}"
    );
}

#[test]
fn validation_accepts_non_supply_kind_without_owner() {
    let mut scenario = empty_scenario(0, 1);
    let mut net = supply_network(10.0);
    // The supply_network helper hardcodes id = "supply"; rename it
    // here to keep the table key and inner id matching, otherwise
    // NetworkIdMismatch fires before our supply-vs-non-supply check.
    net.id = NetworkId::from("comms");
    net.kind = "comms".into();
    net.owner = None;
    scenario.networks.insert(NetworkId::from("comms"), net);
    faultline_engine::validate_scenario(&scenario)
        .expect("non-supply kind without owner is allowed");
}

#[test]
fn validation_supply_kind_is_case_insensitive() {
    // "SUPPLY" with no owner should also reject.
    let mut scenario = empty_scenario(0, 1);
    let mut net = supply_network(10.0);
    net.kind = "Supply".into();
    net.owner = None;
    scenario.networks.insert(NetworkId::from("supply"), net);
    let err = faultline_engine::validate_scenario(&scenario)
        .expect_err("Supply (capitalized) should also reject");
    assert!(format!("{err}").contains("supply"));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn same_seed_produces_identical_supply_reports() {
    // The supply phase is a pure function of `(scenario, state)` —
    // no RNG, no allocation that's order-dependent. Same seed →
    // same per-tick pressure → same report.
    let mut scenario = empty_scenario(2026, 8);
    scenario
        .networks
        .insert(NetworkId::from("supply"), supply_network(50.0));
    scenario
        .events
        .insert(EventId::from("cut"), cut_edge_event(3, 0.3));

    let result_a = Engine::new(scenario.clone())
        .expect("validate")
        .run()
        .expect("run");
    let result_b = Engine::new(scenario).expect("validate").run().expect("run");

    let report_a = result_a
        .supply_pressure_reports
        .get(&FactionId::from("blue"))
        .expect("report a");
    let report_b = result_b
        .supply_pressure_reports
        .get(&FactionId::from("blue"))
        .expect("report b");

    assert_eq!(report_a.samples, report_b.samples);
    assert!((report_a.mean_pressure - report_b.mean_pressure).abs() < 1e-12);
    assert!((report_a.min_pressure - report_b.min_pressure).abs() < 1e-12);
    assert_eq!(report_a.pressured_ticks, report_b.pressured_ticks);
}
