use std::collections::BTreeMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{
    Faction, FactionType, ForceUnit, MilitaryBranch, RecruitmentConfig, UnitType,
};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId, VictoryId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::strategy::FactionAction;
use faultline_types::victory::{VictoryCondition, VictoryType};

use crate::engine::Engine;
use crate::tick;

/// Build a minimal 2-faction, 4-region scenario for testing.
///
/// Layout: nw -- ne
///         |      |
///         sw -- se
///
/// Faction "alpha" has one infantry in nw, controls nw.
/// Faction "bravo" has one infantry in se, controls se.
fn make_test_scenario() -> Scenario {
    let nw = RegionId::from("nw");
    let ne = RegionId::from("ne");
    let sw = RegionId::from("sw");
    let se = RegionId::from("se");

    let alpha = FactionId::from("alpha");
    let bravo = FactionId::from("bravo");

    let mut regions = BTreeMap::new();
    regions.insert(
        nw.clone(),
        Region {
            id: nw.clone(),
            name: "North-West".into(),
            population: 500_000,
            urbanization: 0.5,
            initial_control: Some(alpha.clone()),
            strategic_value: 1.0,
            borders: vec![ne.clone(), sw.clone()],
            centroid: None,
        },
    );
    regions.insert(
        ne.clone(),
        Region {
            id: ne.clone(),
            name: "North-East".into(),
            population: 500_000,
            urbanization: 0.5,
            initial_control: None,
            strategic_value: 1.0,
            borders: vec![nw.clone(), se.clone()],
            centroid: None,
        },
    );
    regions.insert(
        sw.clone(),
        Region {
            id: sw.clone(),
            name: "South-West".into(),
            population: 500_000,
            urbanization: 0.5,
            initial_control: None,
            strategic_value: 1.0,
            borders: vec![nw.clone(), se.clone()],
            centroid: None,
        },
    );
    regions.insert(
        se.clone(),
        Region {
            id: se.clone(),
            name: "South-East".into(),
            population: 500_000,
            urbanization: 0.5,
            initial_control: Some(bravo.clone()),
            strategic_value: 1.0,
            borders: vec![ne.clone(), sw.clone()],
            centroid: None,
        },
    );

    let terrain = vec![
        TerrainModifier {
            region: nw.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: ne.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: sw.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: se.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
    ];

    // Alpha forces.
    let mut alpha_forces = BTreeMap::new();
    alpha_forces.insert(
        ForceId::from("alpha_inf"),
        ForceUnit {
            id: ForceId::from("alpha_inf"),
            name: "Alpha Infantry".into(),
            unit_type: UnitType::Infantry,
            region: nw.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 2.0,
            morale_modifier: 0.0,
            capabilities: vec![],
        },
    );

    // Bravo forces.
    let mut bravo_forces = BTreeMap::new();
    bravo_forces.insert(
        ForceId::from("bravo_inf"),
        ForceUnit {
            id: ForceId::from("bravo_inf"),
            name: "Bravo Infantry".into(),
            unit_type: UnitType::Infantry,
            region: se.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 2.0,
            morale_modifier: 0.0,
            capabilities: vec![],
        },
    );

    let mut factions = BTreeMap::new();
    factions.insert(
        alpha.clone(),
        Faction {
            id: alpha.clone(),
            name: "Alpha".into(),
            faction_type: FactionType::Military {
                branch: MilitaryBranch::Army,
            },
            description: "Test faction alpha".into(),
            color: "#3366CC".into(),
            forces: alpha_forces,
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 50.0,
            initial_resources: 200.0,
            resource_rate: 10.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
        },
    );
    factions.insert(
        bravo.clone(),
        Faction {
            id: bravo.clone(),
            name: "Bravo".into(),
            faction_type: FactionType::Military {
                branch: MilitaryBranch::Army,
            },
            description: "Test faction bravo".into(),
            color: "#CC3333".into(),
            forces: bravo_forces,
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 50.0,
            initial_resources: 200.0,
            resource_rate: 10.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
        },
    );

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("alpha_win"),
        VictoryCondition {
            id: VictoryId::from("alpha_win"),
            name: "Alpha Control".into(),
            faction: alpha.clone(),
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );
    victory_conditions.insert(
        VictoryId::from("bravo_win"),
        VictoryCondition {
            id: VictoryId::from("bravo_win"),
            name: "Bravo Control".into(),
            faction: bravo,
            condition: VictoryType::StrategicControl { threshold: 0.75 },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Test Scenario".into(),
            description: "Minimal 2-faction test".into(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
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
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.3,
            institutional_trust: 0.7,
            media_landscape: MediaLandscape {
                fragmentation: 0.3,
                disinformation_susceptibility: 0.2,
                state_control: 0.1,
                social_media_penetration: 0.5,
                internet_availability: 0.9,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks: 100,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(42),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 10,
        },
        victory_conditions,
    }
}

#[allow(dead_code)]
fn make_test_engine() -> Engine {
    let scenario = make_test_scenario();
    Engine::new(scenario).expect("test engine creation should succeed")
}

#[test]
fn event_phase_fires_eligible_event() {
    let mut scenario = make_test_scenario();
    // Add an event with probability 1.0 and TensionAbove(0.0).
    let event = EventDefinition {
        id: EventId::from("test_event"),
        name: "Test Event".into(),
        description: "Always fires".into(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::TensionAbove { threshold: 0.0 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.1 }],
        chain: None,
    };
    scenario
        .events
        .insert(EventId::from("test_event"), event.clone());

    let mut engine = Engine::new(scenario.clone()).expect("engine creation should succeed");

    // Run a tick which includes the event phase internally.
    let result = engine.tick().expect("tick should succeed");

    assert!(
        !result.events_fired.is_empty(),
        "event with probability 1.0 and TensionAbove(0.0) should fire"
    );
    assert!(
        result.events_fired.iter().any(|e| e == "Test Event"),
        "fired events should contain our test event, got: {:?}",
        result.events_fired,
    );
}

#[test]
fn movement_phase_moves_unit() {
    let scenario = make_test_scenario();
    let map = faultline_geo::load_map(&scenario.map).expect("map should load");
    let engine = Engine::new(scenario).expect("engine should create");

    // Queue a MoveUnit action for alpha_inf from nw to ne.
    let alpha = FactionId::from("alpha");
    let mut queued = BTreeMap::new();
    queued.insert(
        alpha.clone(),
        vec![FactionAction::MoveUnit {
            force: ForceId::from("alpha_inf"),
            destination: RegionId::from("ne"),
        }],
    );

    // Get mutable access to state and run movement phase directly.
    // We use a single tick then check. Instead, build state manually.
    let mut state = engine.state().clone();
    tick::movement_phase(&mut state, &map, &queued);

    let alpha_state = state
        .faction_states
        .get(&alpha)
        .expect("alpha faction should exist");
    let force = alpha_state
        .forces
        .get(&ForceId::from("alpha_inf"))
        .expect("alpha_inf should exist");
    assert_eq!(
        force.region,
        RegionId::from("ne"),
        "unit should have moved to ne"
    );
}

#[test]
fn combat_phase_reduces_strength() {
    let mut scenario = make_test_scenario();

    // Place both forces in the same region (nw).
    scenario
        .factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo should exist")
        .forces
        .get_mut(&ForceId::from("bravo_inf"))
        .expect("bravo_inf should exist")
        .region = RegionId::from("nw");

    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();
    let mut rng = ChaCha8Rng::seed_from_u64(42);

    let str_before_a = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .total_strength;
    let str_before_b = state
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo should exist")
        .total_strength;

    let combats = tick::combat_phase(&mut state, &scenario, &mut rng);

    assert!(combats > 0, "should resolve at least one combat");

    let str_after_a = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .total_strength;
    let str_after_b = state
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo should exist")
        .total_strength;

    assert!(
        str_after_a < str_before_a,
        "alpha should lose strength: before={str_before_a} after={str_after_a}"
    );
    assert!(
        str_after_b < str_before_b,
        "bravo should lose strength: before={str_before_b} after={str_after_b}"
    );
}

#[test]
fn attrition_phase_consumes_resources() {
    let scenario = make_test_scenario();
    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();

    let resources_before = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .resources;

    tick::attrition_phase(&mut state, &scenario);

    let resources_after = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .resources;

    // Resources change = +resource_rate - upkeep.
    // resource_rate=10, upkeep=2 => net +8, but resources should differ.
    assert!(
        (resources_after - resources_before).abs() > f64::EPSILON,
        "resources should change after attrition phase: \
         before={resources_before} after={resources_after}"
    );
}

#[test]
fn attrition_phase_recruits_units() {
    let mut scenario = make_test_scenario();

    // Add recruitment config to alpha.
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .recruitment = Some(RecruitmentConfig {
        rate: 1.0,
        population_threshold: 0.0,
        unit_type: UnitType::Infantry,
        base_strength: 10.0,
        cost: 5.0,
    });

    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();

    let force_count_before = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .forces
        .len();

    // Set tick > 0 so recruit id includes tick.
    state.tick = 1;
    tick::attrition_phase(&mut state, &scenario);

    let force_count_after = state
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .forces
        .len();

    assert!(
        force_count_after > force_count_before,
        "recruitment should create a new unit: \
         before={force_count_before} after={force_count_after}"
    );
}

#[test]
fn victory_check_strategic_control() {
    let scenario = make_test_scenario();
    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();

    // Give alpha control of 3 out of 4 regions (75%).
    let alpha = FactionId::from("alpha");
    state
        .region_control
        .insert(RegionId::from("nw"), Some(alpha.clone()));
    state
        .region_control
        .insert(RegionId::from("ne"), Some(alpha.clone()));
    state
        .region_control
        .insert(RegionId::from("sw"), Some(alpha.clone()));
    state.region_control.insert(RegionId::from("se"), None);

    let outcome = tick::victory_check(&state, &scenario);
    assert!(
        outcome.is_some(),
        "should detect victory when faction controls 75%+ regions"
    );
    let outcome = outcome.expect("just checked is_some");
    assert_eq!(outcome.victor, Some(alpha), "alpha should be the victor");
}

#[test]
fn victory_check_returns_none_when_not_met() {
    let scenario = make_test_scenario();
    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let state = engine.state();

    // Initial state: alpha controls nw, bravo controls se. Neither has 75%.
    let outcome = tick::victory_check(state, &scenario);
    assert!(
        outcome.is_none(),
        "should not detect victory in initial balanced state"
    );
}

#[test]
fn political_phase_updates_tension() {
    let scenario = make_test_scenario();
    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();

    let tension_before = state.political_climate.tension;

    let mut rng = ChaCha8Rng::seed_from_u64(42);
    tick::political_phase(&mut state, &scenario, &mut rng);

    let tension_after = state.political_climate.tension;
    assert!(
        (tension_after - tension_before).abs() > f64::EPSILON,
        "tension should change after political phase: \
         before={tension_before} after={tension_after}"
    );
}

#[test]
fn update_region_control_assigns_to_strongest() {
    let mut scenario = make_test_scenario();

    // Place alpha with strength 100 and bravo with strength 50 in ne.
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha should exist")
        .forces
        .insert(
            ForceId::from("alpha_extra"),
            ForceUnit {
                id: ForceId::from("alpha_extra"),
                name: "Alpha Extra".into(),
                unit_type: UnitType::Infantry,
                region: RegionId::from("ne"),
                strength: 100.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 2.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );
    scenario
        .factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo should exist")
        .forces
        .insert(
            ForceId::from("bravo_extra"),
            ForceUnit {
                id: ForceId::from("bravo_extra"),
                name: "Bravo Extra".into(),
                unit_type: UnitType::Infantry,
                region: RegionId::from("ne"),
                strength: 50.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );

    let engine = Engine::new(scenario.clone()).expect("engine should create");
    let mut state = engine.state().clone();

    tick::update_region_control(&mut state, &scenario);

    let ne_control = state
        .region_control
        .get(&RegionId::from("ne"))
        .expect("ne should be in region_control");
    assert_eq!(
        *ne_control,
        Some(FactionId::from("alpha")),
        "ne should be controlled by the faction with most strength"
    );
}
