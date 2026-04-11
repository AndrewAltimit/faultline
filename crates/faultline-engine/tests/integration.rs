//! Integration tests for Phase 2 features: doctrine, event chains,
//! tech-terrain modifiers, civilian activation, and fog of war.

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_events::EventEvaluator;
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{Faction, FactionType, ForceUnit, MilitaryBranch, UnitType};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId, SegmentId, VictoryId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{
    CivilianAction, FactionSympathy, MediaLandscape, PoliticalClimate, PopulationSegment,
};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::tech::{TechCard, TechCategory, TechEffect, TerrainTechModifier};
use faultline_types::victory::{VictoryCondition, VictoryType};

// -----------------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------------

fn base_scenario() -> Scenario {
    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");
    let r3 = RegionId::from("r3");
    let r4 = RegionId::from("r4");
    let alpha = FactionId::from("alpha");
    let bravo = FactionId::from("bravo");

    let mut regions = BTreeMap::new();
    for (rid, name, sv, borders) in [
        (r1.clone(), "Region 1", 5.0, vec![r2.clone(), r3.clone()]),
        (r2.clone(), "Region 2", 2.0, vec![r1.clone(), r4.clone()]),
        (r3.clone(), "Region 3", 2.0, vec![r1.clone(), r4.clone()]),
        (r4.clone(), "Region 4", 3.0, vec![r2.clone(), r3.clone()]),
    ] {
        regions.insert(
            rid.clone(),
            Region {
                id: rid,
                name: name.into(),
                population: 500_000,
                urbanization: 0.5,
                initial_control: None,
                strategic_value: sv,
                borders,
                centroid: None,
            },
        );
    }
    regions.get_mut(&r1).expect("r1 must exist").initial_control = Some(alpha.clone());
    regions.get_mut(&r4).expect("r4 must exist").initial_control = Some(bravo.clone());

    let mut alpha_forces = BTreeMap::new();
    alpha_forces.insert(
        ForceId::from("a_inf"),
        ForceUnit {
            id: ForceId::from("a_inf"),
            name: "Alpha Infantry".into(),
            unit_type: UnitType::Infantry,
            region: r1.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 2.0,
            morale_modifier: 0.0,
            capabilities: vec![],
        },
    );

    let mut bravo_forces = BTreeMap::new();
    bravo_forces.insert(
        ForceId::from("b_inf"),
        ForceUnit {
            id: ForceId::from("b_inf"),
            name: "Bravo Infantry".into(),
            unit_type: UnitType::Infantry,
            region: r4.clone(),
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
            description: "Test alpha".into(),
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
            faction_type: FactionType::Insurgent,
            description: "Test bravo".into(),
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
            condition: VictoryType::StrategicControl { threshold: 1.0 },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Integration Test".into(),
            description: "Test scenario".into(),
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
            terrain: vec![
                TerrainModifier {
                    region: r1,
                    terrain_type: TerrainType::Urban,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                },
                TerrainModifier {
                    region: r2,
                    terrain_type: TerrainType::Forest,
                    movement_modifier: 0.7,
                    defense_modifier: 1.3,
                    visibility: 0.5,
                },
                TerrainModifier {
                    region: r3,
                    terrain_type: TerrainType::Desert,
                    movement_modifier: 1.0,
                    defense_modifier: 0.8,
                    visibility: 1.0,
                },
                TerrainModifier {
                    region: r4,
                    terrain_type: TerrainType::Mountain,
                    movement_modifier: 0.5,
                    defense_modifier: 1.5,
                    visibility: 0.6,
                },
            ],
        },
        factions,
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.5,
            institutional_trust: 0.7,
            media_landscape: MediaLandscape {
                fragmentation: 0.5,
                disinformation_susceptibility: 0.3,
                state_control: 0.4,
                social_media_penetration: 0.8,
                internet_availability: 0.9,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks: 50,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 10,
            seed: Some(42),
            fog_of_war: false,
            attrition_model: AttritionModel::Stochastic { noise: 0.1 },
            snapshot_interval: 10,
        },
        victory_conditions,
    }
}

// -----------------------------------------------------------------------
// Test: doctrine affects AI behavior
// -----------------------------------------------------------------------

#[test]
fn doctrine_produces_different_weights() {
    use faultline_engine::ai::AiWeights;

    let blitz = AiWeights::for_doctrine(&Doctrine::Blitzkrieg);
    let defensive = AiWeights::for_doctrine(&Doctrine::Defensive);
    let guerrilla = AiWeights::for_doctrine(&Doctrine::Guerrilla);

    // Blitzkrieg should have much higher objective weight than Defensive.
    assert!(
        blitz.objective_weight > defensive.objective_weight * 2.0,
        "Blitzkrieg objective_weight ({:.2}) should be > 2x Defensive ({:.2})",
        blitz.objective_weight,
        defensive.objective_weight,
    );

    // Defensive should have much higher risk aversion than Blitzkrieg.
    assert!(
        defensive.risk_aversion > blitz.risk_aversion * 3.0,
        "Defensive risk_aversion ({:.2}) should be > 3x Blitzkrieg ({:.2})",
        defensive.risk_aversion,
        blitz.risk_aversion,
    );

    // Guerrilla should have higher survival weight than Blitzkrieg.
    assert!(
        guerrilla.survival_weight > blitz.survival_weight * 2.0,
        "Guerrilla survival_weight ({:.2}) should be > 2x Blitzkrieg ({:.2})",
        guerrilla.survival_weight,
        blitz.survival_weight,
    );

    // All doctrines should produce distinct weight profiles.
    let all_doctrines = [
        Doctrine::Conventional,
        Doctrine::Guerrilla,
        Doctrine::Defensive,
        Doctrine::Disruption,
        Doctrine::CounterInsurgency,
        Doctrine::Blitzkrieg,
    ];
    for i in 0..all_doctrines.len() {
        for j in (i + 1)..all_doctrines.len() {
            let w_i = AiWeights::for_doctrine(&all_doctrines[i]);
            let w_j = AiWeights::for_doctrine(&all_doctrines[j]);
            let same = (w_i.survival_weight - w_j.survival_weight).abs() < f64::EPSILON
                && (w_i.objective_weight - w_j.objective_weight).abs() < f64::EPSILON
                && (w_i.opportunity_weight - w_j.opportunity_weight).abs() < f64::EPSILON
                && (w_i.risk_aversion - w_j.risk_aversion).abs() < f64::EPSILON;
            assert!(
                !same,
                "{:?} and {:?} should produce different weight profiles",
                all_doctrines[i], all_doctrines[j],
            );
        }
    }
}

// -----------------------------------------------------------------------
// Test: event chains fire correctly
// -----------------------------------------------------------------------

#[test]
fn event_chain_fires_sequentially() {
    let mut scenario = base_scenario();

    // Create a chain: event_a -> event_b -> event_c
    let event_a = EventDefinition {
        id: EventId::from("event_a"),
        name: "Event A".into(),
        description: "First event".into(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.05 }],
        chain: Some(EventId::from("event_b")),
    };
    let event_b = EventDefinition {
        id: EventId::from("event_b"),
        name: "Event B".into(),
        description: "Chained from A".into(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::EventFired {
            event: EventId::from("event_a"),
            fired: true,
        }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.05 }],
        chain: Some(EventId::from("event_c")),
    };
    let event_c = EventDefinition {
        id: EventId::from("event_c"),
        name: "Event C".into(),
        description: "Chained from B".into(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::EventFired {
            event: EventId::from("event_b"),
            fired: true,
        }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.05 }],
        chain: None,
    };

    scenario.events.insert(EventId::from("event_a"), event_a);
    scenario.events.insert(EventId::from("event_b"), event_b);
    scenario.events.insert(EventId::from("event_c"), event_c);

    let mut engine = Engine::new(scenario).expect("engine should initialize");
    let result = engine.tick().expect("tick should succeed");

    // All three should fire in the first tick.
    assert!(
        result.events_fired.contains(&"Event A".to_string()),
        "Event A should fire, got: {:?}",
        result.events_fired
    );
    assert!(
        result.events_fired.contains(&"Event B".to_string()),
        "Event B should fire via chain, got: {:?}",
        result.events_fired
    );
    assert!(
        result.events_fired.contains(&"Event C".to_string()),
        "Event C should fire via chain, got: {:?}",
        result.events_fired
    );

    // Tension should have increased by 0.15 (3 x 0.05).
    let tension = engine.state().political_climate.tension;
    assert!(
        (tension - 0.65).abs() < 0.02,
        "tension should be ~0.65 (0.5 base + 0.15), got {tension:.3}"
    );
}

// -----------------------------------------------------------------------
// Test: event chain cycle detection
// -----------------------------------------------------------------------

#[test]
fn event_chain_cycle_detected() {
    let event_a = EventDefinition {
        id: EventId::from("cycle_a"),
        name: "Cycle A".into(),
        description: String::new(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![],
        probability: 1.0,
        repeatable: false,
        effects: vec![],
        chain: Some(EventId::from("cycle_b")),
    };
    let event_b = EventDefinition {
        id: EventId::from("cycle_b"),
        name: "Cycle B".into(),
        description: String::new(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![],
        probability: 1.0,
        repeatable: false,
        effects: vec![],
        chain: Some(EventId::from("cycle_a")),
    };

    let result = EventEvaluator::new(vec![event_a, event_b]);
    assert!(result.is_err(), "should detect cycle in event chains");
}

// -----------------------------------------------------------------------
// Test: tech-terrain modifiers affect combat
// -----------------------------------------------------------------------

#[test]
fn tech_terrain_modifiers_change_combat_outcome() {
    // Scenario with tech card giving CombatModifier, deployed in Urban
    // (high effectiveness) vs without tech.
    let mut scenario_tech = base_scenario();

    let tech_card = TechCard {
        id: faultline_types::ids::TechCardId::from("combat_drone"),
        name: "Combat Drone".into(),
        description: "Provides combat bonus".into(),
        category: TechCategory::OffensiveDrone,
        effects: vec![TechEffect::CombatModifier { factor: 1.5 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![],
        terrain_modifiers: vec![
            TerrainTechModifier {
                terrain: TerrainType::Urban,
                effectiveness: 1.5,
            },
            TerrainTechModifier {
                terrain: TerrainType::Forest,
                effectiveness: 0.3,
            },
        ],
        coverage_limit: None,
    };

    scenario_tech.technology.insert(
        faultline_types::ids::TechCardId::from("combat_drone"),
        tech_card,
    );
    scenario_tech
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .tech_access = vec![faultline_types::ids::TechCardId::from("combat_drone")];

    let scenario_no_tech = base_scenario();

    // Run both to same tick and compare alpha's strength.
    let mut engine_tech = Engine::with_seed(scenario_tech, 99).expect("engine should initialize");
    let mut engine_no = Engine::with_seed(scenario_no_tech, 99).expect("engine should initialize");

    for _ in 0..20 {
        engine_tech.tick().expect("tick");
        engine_no.tick().expect("tick");
    }

    let tech_alpha = engine_tech
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha");
    let no_alpha = engine_no
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha");

    // With tech bonus, alpha should fare differently in combat.
    let different = tech_alpha.total_strength != no_alpha.total_strength
        || tech_alpha.morale != no_alpha.morale;

    assert!(
        different,
        "Tech card should change combat outcomes.\n\
         With tech: strength={:.1}, morale={:.3}\n\
         No tech:   strength={:.1}, morale={:.3}",
        tech_alpha.total_strength, tech_alpha.morale, no_alpha.total_strength, no_alpha.morale,
    );
}

// -----------------------------------------------------------------------
// Test: civilian segment activation spawns militia
// -----------------------------------------------------------------------

#[test]
fn civilian_activation_spawns_militia() {
    let mut scenario = base_scenario();

    // Add a population segment with very low threshold so it activates quickly.
    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("test_pop"),
            name: "Test Population".into(),
            fraction: 0.5,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.9, // Already above threshold.
            }],
            activation_threshold: 0.8,
            activation_actions: vec![CivilianAction::ArmedResistance {
                target_faction: FactionId::from("alpha"),
                unit_strength: 25.0,
            }],
            volatility: 0.1,
            activated: false,
        });
    scenario.political_climate.tension = 0.8;

    let mut engine = Engine::new(scenario).expect("engine should initialize");

    let initial_forces = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .forces
        .len();

    // Run enough ticks for the political phase to activate the segment.
    for _ in 0..5 {
        engine.tick().expect("tick");
    }

    let final_forces = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .forces
        .len();

    assert!(
        final_forces > initial_forces,
        "civilian activation should spawn militia. Initial forces: {initial_forces}, \
         Final forces: {final_forces}"
    );
}

// -----------------------------------------------------------------------
// Test: fog of war limits AI information
// -----------------------------------------------------------------------

#[test]
fn fog_of_war_limits_visible_regions() {
    use faultline_engine::ai::build_world_view;

    // Create a scenario with a larger map where alpha can't see bravo.
    let mut scenario = base_scenario();

    // Add two more regions to create distance.
    let r5 = RegionId::from("r5");
    let r6 = RegionId::from("r6");

    scenario.map.regions.insert(
        r5.clone(),
        Region {
            id: r5.clone(),
            name: "Region 5".into(),
            population: 100_000,
            urbanization: 0.3,
            initial_control: None,
            strategic_value: 1.0,
            borders: vec![RegionId::from("r4"), r6.clone()],
            centroid: None,
        },
    );
    scenario.map.regions.insert(
        r6.clone(),
        Region {
            id: r6.clone(),
            name: "Region 6".into(),
            population: 100_000,
            urbanization: 0.3,
            initial_control: Some(FactionId::from("bravo")),
            strategic_value: 1.0,
            borders: vec![r5.clone()],
            centroid: None,
        },
    );

    // Update adjacency: r4 borders r5, r5 borders r6.
    scenario
        .map
        .regions
        .get_mut(&RegionId::from("r4"))
        .expect("r4")
        .borders
        .push(r5.clone());

    // Move bravo to r6 (far from alpha).
    scenario
        .factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo")
        .forces
        .get_mut(&ForceId::from("b_inf"))
        .expect("b_inf")
        .region = r6.clone();

    scenario
        .map
        .regions
        .get_mut(&RegionId::from("r4"))
        .expect("r4")
        .initial_control = None;
    scenario
        .map
        .regions
        .get_mut(&r6)
        .expect("r6")
        .initial_control = Some(FactionId::from("bravo"));

    scenario.map.source = MapSource::Grid {
        width: 3,
        height: 2,
    };

    let engine = Engine::new(scenario.clone()).expect("engine should initialize");

    let alpha = FactionId::from("alpha");
    let map = faultline_geo::load_map(&scenario.map).expect("map should load");
    let world_view = build_world_view(&alpha, engine.state(), &scenario, &map);

    // Alpha is in r1 and can see r1, r2, r3 (adjacent). Should NOT see
    // r5 or r6 (too far away, no recon).
    assert!(
        world_view.known_regions.contains_key(&RegionId::from("r1")),
        "alpha should see r1 (own region)"
    );
    assert!(
        world_view.known_regions.contains_key(&RegionId::from("r2")),
        "alpha should see r2 (adjacent)"
    );
    assert!(
        !world_view.known_regions.contains_key(&r6),
        "alpha should NOT see r6 (too far)"
    );

    // Bravo forces in r6 should NOT be detected.
    let bravo_detected = world_view
        .detected_forces
        .iter()
        .any(|df| df.faction == FactionId::from("bravo"));
    assert!(
        !bravo_detected,
        "alpha should not detect bravo forces in distant r6"
    );
}

// -----------------------------------------------------------------------
// Test: asymmetric scenario loads and runs
// -----------------------------------------------------------------------

#[test]
fn asymmetric_scenario_runs_to_completion() {
    let toml_str = std::fs::read_to_string("../../scenarios/tutorial_asymmetric.toml")
        .expect("should read asymmetric scenario file");

    let scenario: Scenario = toml::from_str(&toml_str).expect("should parse TOML");

    // Validate scenario.
    faultline_engine::validate_scenario(&scenario).expect("scenario should be valid");

    // Run a full simulation.
    let mut engine = Engine::with_seed(scenario, 42).expect("engine should initialize");
    let result = engine.run().expect("simulation should complete");

    assert!(
        result.final_tick > 0,
        "simulation should have run at least one tick"
    );
    assert!(
        result.final_tick <= 365,
        "simulation should complete within max_ticks"
    );
}
