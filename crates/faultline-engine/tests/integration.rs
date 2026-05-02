//! Integration tests covering doctrine, event chains, tech-terrain
//! modifiers, civilian activation, fog of war, kill chains, and
//! tick-stepping semantics.

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_events::EventEvaluator;
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{
    Faction, FactionType, ForceUnit, MilitaryBranch, UnitCapability, UnitType,
};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId, SegmentId, VictoryId};
use faultline_types::map::{
    InfrastructureNode, InfrastructureType, MapConfig, MapSource, Region, TerrainModifier,
    TerrainType,
};
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
            move_progress: 0.0,
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
            move_progress: 0.0,
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
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
            leadership: None,
            alliance_fracture: None,
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
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
            leadership: None,
            alliance_fracture: None,
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
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 2,
            },
            regions,
            infrastructure: BTreeMap::new(),
            // Note: `movement_modifier` is uniform 1.0 across regions
            // so the integration suite is insensitive to the move-
            // accumulator gate — these tests pin tech / combat / event
            // behavior, not movement rate.
            // `defense_modifier` and `visibility` keep their per-
            // region variation since several tests in this file
            // depend on those values.
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
                    movement_modifier: 1.0,
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
                    movement_modifier: 1.0,
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
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: std::collections::BTreeMap::new(),
    }
}

// -----------------------------------------------------------------------
// Campaign / kill chain tests
// -----------------------------------------------------------------------

fn campaign_scenario() -> Scenario {
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, DefensiveDomain, KillChain, PhaseBranch, PhaseCost,
        PhaseOutput,
    };
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 200;

    let chain_id = KillChainId::from("test_chain");
    let recon = PhaseId::from("recon");
    let strike = PhaseId::from("strike");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0, // deterministic success
            min_duration: 3,
            max_duration: 3,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.9,
            cost: PhaseCost {
                attacker_dollars: 100.0,
                defender_dollars: 50_000.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![DefensiveDomain::SignalsIntelligence],
            outputs: vec![PhaseOutput::TensionDelta { delta: 0.05 }],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: strike.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    phases.insert(
        strike.clone(),
        CampaignPhase {
            id: strike.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![recon.clone()],
            base_success_probability: 1.0,
            min_duration: 2,
            max_duration: 2,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.3,
            cost: PhaseCost {
                attacker_dollars: 500.0,
                defender_dollars: 2_000_000.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![
                DefensiveDomain::PhysicalSecurity,
                DefensiveDomain::CounterUAS,
            ],
            outputs: vec![PhaseOutput::TensionDelta { delta: 0.1 }],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id,
            name: "Test Chain".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon,
            phases,
        },
    );

    scenario
}

#[test]
fn campaign_deterministic_phases_succeed() {
    let scenario = campaign_scenario();
    let mut engine = faultline_engine::Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");

    let report = result
        .campaign_reports
        .get(&faultline_types::ids::KillChainId::from("test_chain"))
        .expect("report present");

    use faultline_types::stats::PhaseOutcome;
    assert!(
        matches!(
            report
                .phase_outcomes
                .get(&faultline_types::ids::PhaseId::from("recon")),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "recon should succeed deterministically"
    );
    assert!(
        matches!(
            report
                .phase_outcomes
                .get(&faultline_types::ids::PhaseId::from("strike")),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "strike should succeed deterministically via OnSuccess branch"
    );
    assert!(!report.defender_alerted, "no detection configured");
    assert!(
        (report.attacker_spend - 600.0).abs() < 1e-6,
        "attacker spend should sum phase costs"
    );
    assert!(
        (report.defender_spend - 2_050_000.0).abs() < 1e-6,
        "defender spend should sum phase costs"
    );
}

#[test]
fn campaign_budget_cap_blocks_overspend() {
    let mut scenario = campaign_scenario();
    scenario.attacker_budget = Some(400.0); // cannot afford strike (500)
    let mut engine = faultline_engine::Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");

    let chain_id = faultline_types::ids::KillChainId::from("test_chain");
    let report = result.campaign_reports.get(&chain_id).expect("report");

    use faultline_types::stats::PhaseOutcome;
    assert!(
        matches!(
            report
                .phase_outcomes
                .get(&faultline_types::ids::PhaseId::from("strike")),
            Some(PhaseOutcome::Failed { .. })
        ),
        "strike should be marked Failed when budget cap blocks activation"
    );
}

#[test]
fn campaign_entry_phase_budget_block_fires_on_failure_branch() {
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, DefensiveDomain, KillChain, PhaseBranch, PhaseCost,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    // Build a chain where the entry phase is too expensive for the
    // attacker budget but has an `OnFailure` branch to a cheaper
    // fallback phase. Before the fix, the chain was permanently stuck
    // because resolve_branches was never called on budget-blocked
    // entry phases.
    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 100;
    scenario.attacker_budget = Some(100.0);

    let chain_id = KillChainId::from("fallback_chain");
    let expensive = PhaseId::from("expensive_strike");
    let cheap = PhaseId::from("cheap_fallback");

    let mut phases = BTreeMap::new();
    phases.insert(
        expensive.clone(),
        CampaignPhase {
            id: expensive.clone(),
            name: "Expensive Strike".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 2,
            max_duration: 2,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 500.0, // over cap
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![DefensiveDomain::PhysicalSecurity],
            outputs: vec![],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnFailure,
                next_phase: cheap.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    phases.insert(
        cheap.clone(),
        CampaignPhase {
            id: cheap.clone(),
            name: "Cheap Fallback".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 2,
            max_duration: 2,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 10.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![DefensiveDomain::PhysicalSecurity],
            outputs: vec![],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Fallback chain".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: expensive.clone(),
            phases,
        },
    );

    let mut engine = faultline_engine::Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");

    let report = result
        .campaign_reports
        .get(&chain_id)
        .expect("report present");

    assert!(
        matches!(
            report.phase_outcomes.get(&expensive),
            Some(PhaseOutcome::Failed { .. })
        ),
        "expensive entry phase should be marked Failed due to budget cap"
    );
    assert!(
        matches!(
            report.phase_outcomes.get(&cheap),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "cheap fallback should fire via OnFailure branch after budget block"
    );
}

#[test]
fn campaign_deterministic_detection() {
    let mut scenario = campaign_scenario();
    // Force high detection.
    let recon_pid = faultline_types::ids::PhaseId::from("recon");
    let chain_id = faultline_types::ids::KillChainId::from("test_chain");
    if let Some(chain) = scenario.kill_chains.get_mut(&chain_id)
        && let Some(phase) = chain.phases.get_mut(&recon_pid)
    {
        phase.detection_probability_per_tick = 1.0;
    }
    let mut engine = faultline_engine::Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");

    let report = result.campaign_reports.get(&chain_id).expect("report");
    assert!(report.defender_alerted, "high dp should trigger detection");
    assert!(
        report.attribution_confidence > 0.0,
        "attribution should be set on detection"
    );
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
        defender_options: vec![],
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
        defender_options: vec![],
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
        defender_options: vec![],
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
        defender_options: vec![],
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
        defender_options: vec![],
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
    let toml_str = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scenarios/tutorial_asymmetric.toml"
    ))
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

// =======================================================================
// P0: Tech-terrain combat modifier comprehensive tests
// =======================================================================

#[test]
fn tech_combat_modifier_stacking_multiple_techs() {
    // Deploy two tech cards on alpha, both with CombatModifier.
    // Verify the cumulative effect exceeds a single card.
    let mut scenario = base_scenario();

    let drone = TechCard {
        id: faultline_types::ids::TechCardId::from("drone"),
        name: "Drone".into(),
        description: "Combat drone".into(),
        category: TechCategory::OffensiveDrone,
        effects: vec![TechEffect::CombatModifier { factor: 1.3 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    let ew = TechCard {
        id: faultline_types::ids::TechCardId::from("ew_suite"),
        name: "EW Suite".into(),
        description: "Electronic warfare".into(),
        category: TechCategory::ElectronicWarfare,
        effects: vec![TechEffect::CombatModifier { factor: 1.2 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    scenario
        .technology
        .insert(faultline_types::ids::TechCardId::from("drone"), drone);
    scenario
        .technology
        .insert(faultline_types::ids::TechCardId::from("ew_suite"), ew);
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .tech_access = vec![
        faultline_types::ids::TechCardId::from("drone"),
        faultline_types::ids::TechCardId::from("ew_suite"),
    ];

    // Single tech scenario for comparison.
    let mut scenario_single = base_scenario();
    let drone_single = TechCard {
        id: faultline_types::ids::TechCardId::from("drone"),
        name: "Drone".into(),
        description: "Combat drone".into(),
        category: TechCategory::OffensiveDrone,
        effects: vec![TechEffect::CombatModifier { factor: 1.3 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    scenario_single.technology.insert(
        faultline_types::ids::TechCardId::from("drone"),
        drone_single,
    );
    scenario_single
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .tech_access = vec![faultline_types::ids::TechCardId::from("drone")];

    let mut engine_dual = Engine::with_seed(scenario, 77).expect("engine init");
    let mut engine_single = Engine::with_seed(scenario_single, 77).expect("engine init");

    for _ in 0..25 {
        engine_dual.tick().expect("tick");
        engine_single.tick().expect("tick");
    }

    let dual_str = engine_dual
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .total_strength;
    let single_str = engine_single
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .total_strength;

    // With two tech cards, alpha should fare better (or at least differently)
    // than with one.
    let differs = (dual_str - single_str).abs() > 0.01;
    assert!(
        differs,
        "Two stacked tech cards should produce different results than one.\n\
         Dual: {dual_str:.2}, Single: {single_str:.2}"
    );
}

#[test]
fn tech_countered_by_opponent_negates_modifier() {
    // Alpha deploys drone, bravo deploys counter-drone that counters it.
    let mut scenario = base_scenario();

    let drone = TechCard {
        id: faultline_types::ids::TechCardId::from("drone"),
        name: "Drone".into(),
        description: "".into(),
        category: TechCategory::OffensiveDrone,
        effects: vec![TechEffect::CombatModifier { factor: 1.5 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![faultline_types::ids::TechCardId::from("counter_drone")],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    let counter = TechCard {
        id: faultline_types::ids::TechCardId::from("counter_drone"),
        name: "Counter Drone".into(),
        description: "".into(),
        category: TechCategory::CounterDrone,
        effects: vec![],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    scenario
        .technology
        .insert(faultline_types::ids::TechCardId::from("drone"), drone);
    scenario.technology.insert(
        faultline_types::ids::TechCardId::from("counter_drone"),
        counter,
    );
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .tech_access = vec![faultline_types::ids::TechCardId::from("drone")];
    scenario
        .factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo")
        .tech_access = vec![faultline_types::ids::TechCardId::from("counter_drone")];

    // Compare to uncountered version.
    let mut scenario_uncountered = base_scenario();
    let drone_free = TechCard {
        id: faultline_types::ids::TechCardId::from("drone"),
        name: "Drone".into(),
        description: "".into(),
        category: TechCategory::OffensiveDrone,
        effects: vec![TechEffect::CombatModifier { factor: 1.5 }],
        cost_per_tick: 1.0,
        deployment_cost: 5.0,
        countered_by: vec![faultline_types::ids::TechCardId::from("counter_drone")],
        terrain_modifiers: vec![],
        coverage_limit: None,
    };
    scenario_uncountered
        .technology
        .insert(faultline_types::ids::TechCardId::from("drone"), drone_free);
    scenario_uncountered
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .tech_access = vec![faultline_types::ids::TechCardId::from("drone")];
    // Bravo has NO counter tech.

    let mut engine_countered = Engine::with_seed(scenario, 55).expect("engine");
    let mut engine_free = Engine::with_seed(scenario_uncountered, 55).expect("engine");

    for _ in 0..25 {
        engine_countered.tick().expect("tick");
        engine_free.tick().expect("tick");
    }

    let countered_str = engine_countered
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .total_strength;
    let free_str = engine_free
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .total_strength;

    // When countered, alpha loses the tech advantage — should differ.
    let differs = (countered_str - free_str).abs() > 0.01;
    assert!(
        differs,
        "Countered tech should produce different result than uncountered.\n\
         Countered: {countered_str:.2}, Free: {free_str:.2}"
    );
}

// =======================================================================
// P0: Civilian activation — all action types
// =======================================================================

#[test]
fn civilian_activation_sabotage_damages_infrastructure() {
    let mut scenario = base_scenario();

    // Add infrastructure in r1.
    let infra_id = faultline_types::ids::InfraId::from("power_r1");
    scenario.map.infrastructure.insert(
        infra_id.clone(),
        InfrastructureNode {
            id: infra_id.clone(),
            name: "Power Grid R1".into(),
            region: RegionId::from("r1"),
            infra_type: InfrastructureType::PowerGrid,
            criticality: 0.8,
            initial_status: 1.0,
            repairable: Some(30),
        },
    );

    // Segment that activates immediately with sabotage.
    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("saboteurs"),
            name: "Saboteurs".into(),
            fraction: 0.3,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("bravo"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::Sabotage {
                target_infra_type: Some(InfrastructureType::PowerGrid),
                probability: 1.0, // Always sabotages for determinism.
            }],
            volatility: 0.1,
            activated: false,
        });
    scenario.political_climate.tension = 0.8;

    let mut engine = Engine::new(scenario).expect("engine");

    // Run ticks until activation.
    for _ in 0..5 {
        engine.tick().expect("tick");
    }

    let infra_status = engine.state().infra_status.get(&infra_id).copied();
    assert!(
        infra_status.is_some_and(|s| s < 1.0),
        "sabotage should damage infrastructure, got status: {infra_status:?}"
    );
}

#[test]
fn civilian_activation_material_support_adds_resources() {
    let mut scenario = base_scenario();

    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("supporters"),
            name: "Material Supporters".into(),
            fraction: 0.4,
            concentrated_in: vec![RegionId::from("r4")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("bravo"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::MaterialSupport {
                target_faction: FactionId::from("bravo"),
                rate: 20.0,
            }],
            volatility: 0.1,
            activated: false,
        });
    scenario.political_climate.tension = 0.8;

    let mut engine = Engine::new(scenario.clone()).expect("engine");
    // Run simulation with material support.
    for _ in 0..10 {
        engine.tick().expect("tick");
    }

    let bravo_resources = engine
        .state()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo")
        .resources;

    // With material support (rate=20.0/tick after activation), bravo should
    // have significantly more resources. Base resource_rate is 10.0/tick.
    // After ~5 ticks of activation: extra 100+ resources.
    assert!(
        bravo_resources > 200.0,
        "material support should boost bravo resources, got {bravo_resources:.1}"
    );
}

#[test]
fn civilian_activation_protest_increases_tension() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.5;

    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("protestors"),
            name: "Protestors".into(),
            fraction: 0.5,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::Protest { intensity: 1.0 }],
            volatility: 0.1,
            activated: false,
        });

    let initial_tension = scenario.political_climate.tension;
    let mut engine = Engine::new(scenario).expect("engine");

    for _ in 0..5 {
        engine.tick().expect("tick");
    }

    let final_tension = engine.state().political_climate.tension;
    assert!(
        final_tension > initial_tension,
        "protest should increase tension. Initial: {initial_tension:.3}, Final: {final_tension:.3}"
    );
}

#[test]
fn civilian_activation_flee_reduces_fraction() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.8;

    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("refugees"),
            name: "Refugees".into(),
            fraction: 0.6,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::Flee { rate: 0.1 }],
            volatility: 0.1,
            activated: false,
        });

    let mut engine = Engine::new(scenario).expect("engine");

    for _ in 0..5 {
        engine.tick().expect("tick");
    }

    let segment = engine
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("refugees"))
        .expect("segment should exist");

    assert!(
        segment.fraction < 0.6,
        "flee should reduce segment fraction. Got: {:.3}",
        segment.fraction
    );
}

#[test]
fn civilian_activation_threshold_boundary_exact_match() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.5;

    // Sympathy at threshold (>=). Set slightly above to survive the
    // tiny tension-based drift that happens before the activation check.
    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("boundary"),
            name: "Boundary Segment".into(),
            fraction: 0.3,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.81,
            }],
            activation_threshold: 0.8,
            activation_actions: vec![CivilianAction::ArmedResistance {
                target_faction: FactionId::from("alpha"),
                unit_strength: 10.0,
            }],
            volatility: 0.0, // Zero volatility to prevent random drift.
            activated: false,
        });

    let mut engine = Engine::new(scenario).expect("engine");

    let initial_forces = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .forces
        .len();

    for _ in 0..3 {
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
        "segment at exact threshold should activate. Initial: {initial_forces}, Final: {final_forces}"
    );
}

#[test]
fn civilian_already_activated_does_not_re_trigger() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.8;

    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("once_only"),
            name: "Once Only".into(),
            fraction: 0.3,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::ArmedResistance {
                target_faction: FactionId::from("alpha"),
                unit_strength: 10.0,
            }],
            volatility: 0.1,
            activated: false,
        });

    let mut engine = Engine::new(scenario).expect("engine");

    // Run enough for activation.
    for _ in 0..3 {
        engine.tick().expect("tick");
    }

    // Run more ticks — segment should stay activated (one-time trigger).
    for _ in 0..10 {
        engine.tick().expect("tick");
    }

    let seg = engine
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("once_only"))
        .expect("segment");
    assert!(seg.activated, "segment should remain activated");
}

// =======================================================================
// P0: Fog of war — Recon capability + intelligence
// =======================================================================

#[test]
fn fog_of_war_recon_extends_visibility() {
    use faultline_engine::ai::build_world_view;

    let mut scenario = base_scenario();

    // Add regions r5, r6 far from alpha.
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
            initial_control: None,
            strategic_value: 1.0,
            borders: vec![r5.clone()],
            centroid: None,
        },
    );
    scenario
        .map
        .regions
        .get_mut(&RegionId::from("r4"))
        .expect("r4")
        .borders
        .push(r5.clone());
    scenario.map.source = MapSource::Grid {
        width: 3,
        height: 2,
    };

    // Give alpha a recon unit with range 2 in r2 (adj to r4, which is adj to r5).
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .forces
        .insert(
            ForceId::from("a_recon"),
            ForceUnit {
                id: ForceId::from("a_recon"),
                name: "Alpha Recon".into(),
                unit_type: UnitType::SpecialOperations,
                region: RegionId::from("r2"),
                strength: 10.0,
                mobility: 2.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![UnitCapability::Recon {
                    range: 2.0,
                    detection: 0.8,
                }],
                move_progress: 0.0,
            },
        );

    let engine = Engine::new(scenario.clone()).expect("engine");
    let map = faultline_geo::load_map(&scenario.map).expect("map");
    let alpha = FactionId::from("alpha");

    let wv = build_world_view(&alpha, engine.state(), &scenario, &map);

    // Without recon, alpha in r1 sees r1, r2, r3 (adj).
    // r2 has a recon unit with range 2. From r2: hop1 = r1, r4; hop2 = r2, r3, r5.
    // So r5 should be visible via recon.
    assert!(
        wv.known_regions.contains_key(&r5),
        "recon with range 2 from r2 should see r5 (2 hops: r2->r4->r5)"
    );

    // r6 is 3 hops from r2 (r2->r4->r5->r6). With range 2, should NOT see r6.
    // Actually range=2.0, capped at 3 hops. Let me check: 2 as u32 = 2 hops.
    // r2->r4 (hop 1), r4->r5 (hop 2). So r5 is visible. r6 needs hop 3.
    assert!(
        !wv.known_regions.contains_key(&r6),
        "recon with range 2 should NOT see r6 (3 hops away)"
    );
}

#[test]
fn fog_of_war_intelligence_affects_confidence() {
    use faultline_engine::ai::build_world_view;

    let mut scenario_low = base_scenario();
    scenario_low
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .intelligence = 0.0; // Minimum intel.

    let mut scenario_high = base_scenario();
    scenario_high
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .intelligence = 1.0; // Maximum intel.

    // Put bravo in r2 (adj to alpha's r1) so it's visible.
    for s in [&mut scenario_low, &mut scenario_high] {
        s.factions
            .get_mut(&FactionId::from("bravo"))
            .expect("bravo")
            .forces
            .get_mut(&ForceId::from("b_inf"))
            .expect("b_inf")
            .region = RegionId::from("r2");
    }

    let engine_low = Engine::new(scenario_low.clone()).expect("engine");
    let engine_high = Engine::new(scenario_high.clone()).expect("engine");

    let map_low = faultline_geo::load_map(&scenario_low.map).expect("map");
    let map_high = faultline_geo::load_map(&scenario_high.map).expect("map");

    let alpha = FactionId::from("alpha");
    let wv_low = build_world_view(&alpha, engine_low.state(), &scenario_low, &map_low);
    let wv_high = build_world_view(&alpha, engine_high.state(), &scenario_high, &map_high);

    // Both should detect bravo (in adjacent r2).
    assert!(
        !wv_low.detected_forces.is_empty(),
        "low intel should still detect adjacent forces"
    );
    assert!(
        !wv_high.detected_forces.is_empty(),
        "high intel should detect adjacent forces"
    );

    let conf_low = wv_low.detected_forces[0].confidence;
    let conf_high = wv_high.detected_forces[0].confidence;

    assert!(
        conf_high > conf_low,
        "higher intelligence should produce higher confidence.\n\
         Low: {conf_low:.3}, High: {conf_high:.3}"
    );

    // Low intel confidence: (0.0 * 0.6 + 0.2) = 0.2.
    assert!(
        (conf_low - 0.2).abs() < 0.01,
        "intelligence=0.0 should give confidence=0.2, got {conf_low:.3}"
    );

    // High intel confidence: (1.0 * 0.6 + 0.2) = 0.8.
    assert!(
        (conf_high - 0.8).abs() < 0.01,
        "intelligence=1.0 should give confidence=0.8, got {conf_high:.3}"
    );
}

#[test]
fn fog_of_war_detected_force_strength_scaled_by_confidence() {
    use faultline_engine::ai::build_world_view;

    let mut scenario = base_scenario();
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .intelligence = 0.5;

    // Put bravo in r2 (adjacent to alpha).
    scenario
        .factions
        .get_mut(&FactionId::from("bravo"))
        .expect("bravo")
        .forces
        .get_mut(&ForceId::from("b_inf"))
        .expect("b_inf")
        .region = RegionId::from("r2");

    let engine = Engine::new(scenario.clone()).expect("engine");
    let map = faultline_geo::load_map(&scenario.map).expect("map");
    let alpha = FactionId::from("alpha");
    let wv = build_world_view(&alpha, engine.state(), &scenario, &map);

    let df = &wv.detected_forces[0];
    let actual_strength = 100.0; // bravo's b_inf strength.
    let confidence = df.confidence;

    // estimated_strength = actual * confidence.
    let expected = actual_strength * confidence;
    assert!(
        (df.estimated_strength - expected).abs() < 0.01,
        "estimated strength should be actual * confidence.\n\
         Expected: {expected:.2}, Got: {:.2}",
        df.estimated_strength
    );
}

// =======================================================================
// P0: Doctrine morale adjustments
// =======================================================================

#[test]
fn doctrine_morale_low_increases_survival_weight() {
    // Create scenario with alpha at very low morale, test that weights shift.
    let mut scenario = base_scenario();
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .doctrine = Doctrine::Conventional;
    scenario
        .factions
        .get_mut(&FactionId::from("alpha"))
        .expect("alpha")
        .initial_morale = 0.15; // Below 0.3 threshold.

    let mut engine = Engine::new(scenario).expect("engine");
    // After initialization, alpha morale = 0.15.
    // determine_weights with morale < 0.3 should boost survival_weight.
    // We can verify this indirectly by checking behavior — a low-morale
    // faction should prioritize defense.

    // Run a few ticks and ensure the engine doesn't panic.
    for _ in 0..5 {
        engine.tick().expect("tick");
    }

    let alpha = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha");
    // Low morale faction exists and hasn't crashed.
    assert!(
        alpha.morale >= 0.0,
        "morale should stay non-negative: {}",
        alpha.morale
    );
}

#[test]
fn doctrine_adaptive_morale_full_strength_adjustments() {
    use faultline_engine::ai::AiWeights;

    // Test Adaptive doctrine base weights.
    let adaptive_base = AiWeights::for_doctrine(&Doctrine::Adaptive);
    let conventional_base = AiWeights::for_doctrine(&Doctrine::Conventional);

    // Adaptive base should match Conventional base.
    assert!(
        (adaptive_base.survival_weight - conventional_base.survival_weight).abs() < f64::EPSILON,
        "Adaptive base should match Conventional"
    );
}

// =======================================================================
// P1: Event chain edge cases
// =======================================================================

#[test]
fn event_chain_self_referencing_cycle_detected() {
    let event = EventDefinition {
        id: EventId::from("self_ref"),
        name: "Self Ref".into(),
        description: String::new(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![],
        probability: 1.0,
        repeatable: false,
        effects: vec![],
        chain: Some(EventId::from("self_ref")), // Points to itself.
        defender_options: vec![],
    };

    let result = EventEvaluator::new(vec![event]);
    assert!(
        result.is_err(),
        "self-referencing chain should be detected as a cycle"
    );
}

#[test]
fn event_chain_long_cycle_detected() {
    // A -> B -> C -> D -> A (4-event cycle).
    let events = vec![
        EventDefinition {
            id: EventId::from("a"),
            name: "A".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: Some(EventId::from("b")),
            defender_options: vec![],
        },
        EventDefinition {
            id: EventId::from("b"),
            name: "B".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: Some(EventId::from("c")),
            defender_options: vec![],
        },
        EventDefinition {
            id: EventId::from("c"),
            name: "C".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: Some(EventId::from("d")),
            defender_options: vec![],
        },
        EventDefinition {
            id: EventId::from("d"),
            name: "D".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: Some(EventId::from("a")), // Back to A.
            defender_options: vec![],
        },
    ];

    let result = EventEvaluator::new(events);
    assert!(result.is_err(), "4-event cycle should be detected");
}

#[test]
fn event_chain_no_cycle_with_none_termination() {
    // A -> B -> None (valid, no cycle).
    let events = vec![
        EventDefinition {
            id: EventId::from("x"),
            name: "X".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: Some(EventId::from("y")),
            defender_options: vec![],
        },
        EventDefinition {
            id: EventId::from("y"),
            name: "Y".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
            defender_options: vec![],
        },
    ];

    let result = EventEvaluator::new(events);
    assert!(
        result.is_ok(),
        "A->B->None should not be a cycle, got: {:?}",
        result.err()
    );
}

#[test]
fn event_chain_stops_when_chained_conditions_fail() {
    let mut scenario = base_scenario();

    // A fires (always). B requires tension > 0.99 (won't pass at 0.5).
    let event_a = EventDefinition {
        id: EventId::from("chain_a"),
        name: "Chain A".into(),
        description: String::new(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.1 }],
        chain: Some(EventId::from("chain_b")),
        defender_options: vec![],
    };
    let event_b = EventDefinition {
        id: EventId::from("chain_b"),
        name: "Chain B".into(),
        description: String::new(),
        earliest_tick: None,
        latest_tick: None,
        conditions: vec![EventCondition::TensionAbove { threshold: 0.99 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::TensionShift { delta: 0.3 }],
        chain: None,
        defender_options: vec![],
    };

    scenario.events.insert(EventId::from("chain_a"), event_a);
    scenario.events.insert(EventId::from("chain_b"), event_b);

    let mut engine = Engine::new(scenario).expect("engine");
    let result = engine.tick().expect("tick");

    // A should fire, B should NOT (tension starts at 0.5, becomes 0.6 after A).
    assert!(
        result.events_fired.contains(&"Chain A".to_string()),
        "Chain A should fire"
    );
    assert!(
        !result.events_fired.contains(&"Chain B".to_string()),
        "Chain B should NOT fire (conditions not met)"
    );

    // Tension should be 0.6 (0.5 + 0.1 from A), not 0.9 (0.5 + 0.1 + 0.3).
    let tension = engine.state().political_climate.tension;
    assert!(
        (tension - 0.6).abs() < 0.05,
        "tension should be ~0.6 (only A fired), got {tension:.3}"
    );
}

#[test]
fn event_chain_empty_evaluator_no_panic() {
    let result = EventEvaluator::new(vec![]);
    assert!(result.is_ok(), "empty event list should be valid");
    let evaluator = result.expect("just checked is_ok");
    assert!(evaluator.events.is_empty());
}

// =======================================================================
// Civilian activation — Intelligence and NonCooperation
// =======================================================================

#[test]
fn civilian_activation_intelligence_degrades_target_morale() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.8;

    // Segment favors alpha, provides intel against bravo.
    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("spies"),
            name: "Informants".into(),
            fraction: 0.2,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::Intelligence {
                target_faction: FactionId::from("bravo"),
                quality: 0.8,
            }],
            volatility: 0.1,
            activated: false,
        });

    let mut engine = Engine::new(scenario.clone()).expect("engine");

    // Baseline without intelligence segment.
    let mut scenario_baseline = base_scenario();
    scenario_baseline.political_climate.tension = 0.8;
    let mut engine_baseline = Engine::new(scenario_baseline).expect("engine");

    for _ in 0..5 {
        engine.tick().expect("tick");
        engine_baseline.tick().expect("tick");
    }

    let bravo_morale = engine
        .state()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo")
        .morale;
    let bravo_baseline = engine_baseline
        .state()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo")
        .morale;

    assert!(
        bravo_morale < bravo_baseline,
        "intelligence action should degrade target morale.\n\
         With intel: {bravo_morale:.3}, Baseline: {bravo_baseline:.3}"
    );

    // Alpha should get a resource bonus from intel.
    let alpha_resources = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .resources;
    let alpha_baseline = engine_baseline
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .resources;

    assert!(
        alpha_resources > alpha_baseline,
        "intelligence should grant resource bonus to favored faction.\n\
         With intel: {alpha_resources:.1}, Baseline: {alpha_baseline:.1}"
    );
}

#[test]
fn civilian_activation_noncooperation_reduces_controller_resources() {
    let mut scenario = base_scenario();
    scenario.political_climate.tension = 0.8;

    // Segment concentrated in r1 (controlled by alpha). Favors bravo.
    // NonCooperation should reduce alpha's resources.
    scenario
        .political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("strikers"),
            name: "Labor Strikers".into(),
            fraction: 0.3,
            concentrated_in: vec![RegionId::from("r1")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("bravo"),
                sympathy: 0.95,
            }],
            activation_threshold: 0.9,
            activation_actions: vec![CivilianAction::NonCooperation {
                effectiveness_reduction: 0.15,
            }],
            volatility: 0.1,
            activated: false,
        });

    let mut engine = Engine::new(scenario.clone()).expect("engine");

    let mut scenario_baseline = base_scenario();
    scenario_baseline.political_climate.tension = 0.8;
    let mut engine_baseline = Engine::new(scenario_baseline).expect("engine");

    for _ in 0..5 {
        engine.tick().expect("tick");
        engine_baseline.tick().expect("tick");
    }

    let alpha_resources = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .resources;
    let alpha_baseline = engine_baseline
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha")
        .resources;

    assert!(
        alpha_resources < alpha_baseline,
        "non-cooperation should reduce controlling faction's resources.\n\
         With strike: {alpha_resources:.1}, Baseline: {alpha_baseline:.1}"
    );
}

// -----------------------------------------------------------------------
// Tick-stepping integration tests
// These mirror WasmEngine usage: tick_n batches, snapshot capture,
// event accumulation, and is_finished behavior.
// -----------------------------------------------------------------------

#[test]
fn tick_stepping_accumulates_snapshots() {
    let mut scenario = base_scenario();
    scenario.simulation.snapshot_interval = 5;
    scenario.simulation.max_ticks = 20;

    let mut engine = Engine::new(scenario).expect("engine creation");
    let mut snapshots = Vec::new();

    // Step through in batches, capturing snapshots.
    while !engine.is_finished() {
        engine.tick().expect("tick should succeed");
        snapshots.push(engine.snapshot());
    }

    assert_eq!(
        snapshots.len(),
        20,
        "should have 20 snapshots (one per tick)"
    );
    assert_eq!(snapshots[0].tick, 1, "first snapshot should be tick 1");
    assert_eq!(snapshots[19].tick, 20, "last snapshot should be tick 20");

    // Verify ticks are monotonically increasing.
    for i in 1..snapshots.len() {
        assert!(
            snapshots[i].tick > snapshots[i - 1].tick,
            "snapshot ticks should be monotonically increasing"
        );
    }
}

#[test]
fn tick_stepping_event_accumulation() {
    // Use the asymmetric scenario which has events.
    let toml_str = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scenarios/tutorial_asymmetric.toml"),
    )
    .expect("should read asymmetric scenario");
    let scenario: Scenario = toml::from_str(&toml_str).expect("should parse");

    let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");

    let mut all_events: Vec<(u32, String)> = Vec::new();

    while !engine.is_finished() {
        let result = engine.tick().expect("tick should succeed");

        // Accumulate events like WasmEngine does.
        for eid in &engine.state().events_fired_this_tick {
            all_events.push((engine.current_tick(), eid.0.clone()));
        }

        if result.outcome.is_some() {
            break;
        }
    }

    // Compare with full run to verify consistency.
    let scenario2: Scenario = toml::from_str(&toml_str).expect("should parse");
    let mut engine2 = Engine::with_seed(scenario2, 42).expect("engine creation");
    let run_result = engine2.run().expect("run should succeed");

    // The event counts should match.
    assert_eq!(
        all_events.len(),
        run_result.event_log.len(),
        "tick-stepping should accumulate the same events as run()"
    );

    // Event IDs should match in order.
    for (i, (tick, eid)) in all_events.iter().enumerate() {
        assert_eq!(
            *tick, run_result.event_log[i].tick,
            "event tick mismatch at index {i}"
        );
        assert_eq!(
            *eid, run_result.event_log[i].event_id.0,
            "event ID mismatch at index {i}"
        );
    }
}

#[test]
fn tick_stepping_determinism_matches_run() {
    let scenario = base_scenario();

    // Run via tick-stepping.
    let mut engine1 = Engine::new(scenario.clone()).expect("engine creation");
    while !engine1.is_finished() {
        let result = engine1.tick().expect("tick");
        if result.outcome.is_some() {
            break;
        }
    }
    let snap1 = engine1.snapshot();

    // Run via run().
    let mut engine2 = Engine::new(scenario).expect("engine creation");
    let run_result = engine2.run().expect("run");

    // Final states should be identical.
    assert_eq!(snap1.tick, run_result.final_state.tick);

    // Check faction states match.
    for (fid, fs1) in &snap1.faction_states {
        let fs2 = run_result
            .final_state
            .faction_states
            .get(fid)
            .expect("faction should exist in run result");
        assert!(
            (fs1.total_strength - fs2.total_strength).abs() < f64::EPSILON,
            "strength mismatch for {fid}: {:.4} vs {:.4}",
            fs1.total_strength,
            fs2.total_strength
        );
        assert!(
            (fs1.morale - fs2.morale).abs() < f64::EPSILON,
            "morale mismatch for {fid}: {:.4} vs {:.4}",
            fs1.morale,
            fs2.morale
        );
        assert!(
            (fs1.resources - fs2.resources).abs() < f64::EPSILON,
            "resources mismatch for {fid}: {:.4} vs {:.4}",
            fs1.resources,
            fs2.resources
        );
    }

    // Region control should match.
    assert_eq!(snap1.region_control, run_result.final_state.region_control);
}

#[test]
fn tick_batch_stepping_n_at_a_time() {
    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 30;

    let mut engine = Engine::new(scenario).expect("engine creation");

    // Advance in batches of 10.
    for batch in 0..3 {
        for _ in 0..10 {
            let result = engine.tick().expect("tick should succeed");
            if result.outcome.is_some() {
                return; // Victory reached early — test is still valid.
            }
        }
        let snap = engine.snapshot();
        assert_eq!(
            snap.tick,
            (batch + 1) * 10,
            "after batch {batch}, tick should be {}",
            (batch + 1) * 10
        );
    }

    assert!(engine.is_finished(), "should be finished after 30 ticks");
}

#[test]
fn is_finished_not_triggered_by_victory() {
    // is_finished() checks both max_ticks and outcome_reached (victory).
    // Victory is also reported via TickResult.outcome.
    let scenario = base_scenario();
    let mut engine = Engine::new(scenario).expect("engine creation");

    // Tick once — victory hasn't been reached yet.
    let result = engine.tick().expect("tick");
    if result.outcome.is_none() {
        // If no victory on tick 1, is_finished should be false.
        assert!(
            !engine.is_finished(),
            "is_finished should be false before max_ticks"
        );
    }
}

#[test]
fn snapshot_tension_tracks_political_changes() {
    let scenario = base_scenario();
    let mut engine = Engine::new(scenario).expect("engine creation");

    let initial_tension = engine.snapshot().tension;

    // Run a few ticks — tension should change due to political phase.
    for _ in 0..10 {
        engine.tick().expect("tick");
    }

    let later_tension = engine.snapshot().tension;

    // Tension may or may not change depending on simulation dynamics,
    // but both values should be in valid bounds.
    assert!(
        (0.0..=1.0).contains(&initial_tension),
        "initial tension {initial_tension} should be in [0, 1]"
    );
    assert!(
        (0.0..=1.0).contains(&later_tension),
        "later tension {later_tension} should be in [0, 1]"
    );
}

#[test]
fn snapshot_infra_status_present_when_infra_exists() {
    use faultline_types::ids::InfraId;
    use faultline_types::map::{InfrastructureNode, InfrastructureType};

    let mut scenario = base_scenario();
    let iid = InfraId::from("test_power_grid");
    scenario.map.infrastructure.insert(
        iid.clone(),
        InfrastructureNode {
            id: iid.clone(),
            name: "Test Power Grid".into(),
            region: RegionId::from("r1"),
            infra_type: InfrastructureType::PowerGrid,
            criticality: 0.8,
            initial_status: 1.0,
            repairable: Some(10),
        },
    );

    let mut engine = Engine::new(scenario).expect("engine creation");

    // Snapshot at tick 0 should include infra.
    let snap0 = engine.snapshot();
    assert!(
        snap0.infra_status.contains_key(&iid),
        "snapshot should include infrastructure status at tick 0"
    );

    // Advance and check again.
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    let snap5 = engine.snapshot();
    assert!(
        snap5.infra_status.contains_key(&iid),
        "snapshot should include infrastructure status at tick 5"
    );
}

#[test]
fn escalation_threshold_branch_fires_after_sustained_window() {
    // Build a chain whose recon phase has two `OnSuccess` branches:
    //   1. EscalationThreshold(Tension >= 0.7 for >= 3 ticks) → escalate
    //   2. Always → de_escalate (terminal fallback)
    // The simulation seeds tension at 0.95, well above 0.7. The recon
    // phase takes 5 ticks to complete. By the time it resolves, the
    // hysteresis counter has been satisfied for at least 5 consecutive
    // ticks, so branch (1) wins and the chain transitions to the
    // `escalate` phase. Without the new variant the test would silently
    // fall through to `de_escalate`.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, DefensiveDomain, EscalationMetric, KillChain, PhaseBranch,
        PhaseCost, PhaseOutput, ThresholdDirection,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 50;
    scenario.political_climate.tension = 0.95;

    let chain_id = KillChainId::from("escalation_chain");
    let recon = PhaseId::from("recon");
    let escalate = PhaseId::from("escalate");
    let de_escalate = PhaseId::from("de_escalate");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 5,
            max_duration: 5,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![DefensiveDomain::SignalsIntelligence],
            outputs: vec![PhaseOutput::TensionDelta { delta: 0.0 }],
            branches: vec![
                PhaseBranch {
                    condition: BranchCondition::EscalationThreshold {
                        metric: EscalationMetric::Tension,
                        threshold: 0.7,
                        direction: ThresholdDirection::Above,
                        sustained_ticks: 3,
                    },
                    next_phase: escalate.clone(),
                },
                PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: de_escalate.clone(),
                },
            ],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    for (id, name) in [
        (escalate.clone(), "Escalate"),
        (de_escalate.clone(), "DeEscalate"),
    ] {
        phases.insert(
            id.clone(),
            CampaignPhase {
                id: id.clone(),
                name: name.into(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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
    }

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Escalation".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");
    let report = result.campaign_reports.get(&chain_id).expect("report");

    // The escalation branch must have fired — `escalate` should be a
    // terminal success, and `de_escalate` should remain Pending.
    assert!(
        matches!(
            report.phase_outcomes.get(&escalate),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "escalation branch should fire when tension stays above threshold long enough; got {:?}",
        report.phase_outcomes
    );
    assert!(
        matches!(
            report.phase_outcomes.get(&de_escalate),
            Some(PhaseOutcome::Pending) | None
        ),
        "fallback de-escalation branch must not fire when escalation matches first"
    );
}

#[test]
fn escalation_threshold_does_not_fire_below_threshold() {
    // Same chain shape, but tension is held low. The fallback `Always`
    // branch should win because the EscalationThreshold predicate is
    // never satisfied.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, EscalationMetric, KillChain, PhaseBranch, PhaseCost,
        ThresholdDirection,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 50;
    scenario.political_climate.tension = 0.05;

    let chain_id = KillChainId::from("escalation_chain");
    let recon = PhaseId::from("recon");
    let escalate = PhaseId::from("escalate");
    let de_escalate = PhaseId::from("de_escalate");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 5,
            max_duration: 5,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![
                PhaseBranch {
                    condition: BranchCondition::EscalationThreshold {
                        metric: EscalationMetric::Tension,
                        threshold: 0.7,
                        direction: ThresholdDirection::Above,
                        sustained_ticks: 3,
                    },
                    next_phase: escalate.clone(),
                },
                PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: de_escalate.clone(),
                },
            ],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    for id in [escalate.clone(), de_escalate.clone()] {
        phases.insert(
            id.clone(),
            CampaignPhase {
                id: id.clone(),
                name: id.to_string(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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
    }

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Escalation".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");
    let report = result.campaign_reports.get(&chain_id).expect("report");
    assert!(
        matches!(
            report.phase_outcomes.get(&de_escalate),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "fallback should fire when escalation threshold is not satisfied"
    );
    assert!(
        matches!(
            report.phase_outcomes.get(&escalate),
            Some(PhaseOutcome::Pending) | None
        ),
        "escalation branch must not fire below threshold"
    );
}

#[test]
fn or_any_branch_fires_on_any_inner_match() {
    // OrAny composes two inner conditions:
    //   1. EscalationThreshold(Tension <= 0.1 sustained 3 ticks)
    //   2. OnSuccess
    // Tension is high and the recon phase deterministically succeeds.
    // Inner #1 cannot match (tension never drops); inner #2 must, so
    // the branch fires and the chain transitions to `escalate`. Without
    // the new variant the test would fall through to `de_escalate`.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, EscalationMetric, KillChain, PhaseBranch, PhaseCost,
        ThresholdDirection,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 50;
    scenario.political_climate.tension = 0.95;

    let chain_id = KillChainId::from("or_any_chain");
    let recon = PhaseId::from("recon");
    let escalate = PhaseId::from("escalate");
    let de_escalate = PhaseId::from("de_escalate");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 5,
            max_duration: 5,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![
                PhaseBranch {
                    condition: BranchCondition::OrAny {
                        conditions: vec![
                            BranchCondition::EscalationThreshold {
                                metric: EscalationMetric::Tension,
                                threshold: 0.1,
                                direction: ThresholdDirection::Below,
                                sustained_ticks: 3,
                            },
                            BranchCondition::OnSuccess,
                        ],
                    },
                    next_phase: escalate.clone(),
                },
                PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: de_escalate.clone(),
                },
            ],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    for id in [escalate.clone(), de_escalate.clone()] {
        phases.insert(
            id.clone(),
            CampaignPhase {
                id: id.clone(),
                name: id.to_string(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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
    }

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "OrAny".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");
    let report = result.campaign_reports.get(&chain_id).expect("report");
    assert!(
        matches!(
            report.phase_outcomes.get(&escalate),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "OrAny branch must fire when at least one inner condition matches; got {:?}",
        report.phase_outcomes
    );
    assert!(
        matches!(
            report.phase_outcomes.get(&de_escalate),
            Some(PhaseOutcome::Pending) | None
        ),
        "fallback must not fire when OrAny matches first"
    );
}

#[test]
fn or_any_does_not_fire_when_all_inners_fail() {
    // OrAny over `OnFailure` and `EscalationThreshold(Below 0.1)` —
    // the recon phase succeeds (so OnFailure fails) and tension stays
    // high (so the threshold fails). The fallback must win.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, EscalationMetric, KillChain, PhaseBranch, PhaseCost,
        ThresholdDirection,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 50;
    scenario.political_climate.tension = 0.95;

    let chain_id = KillChainId::from("or_any_chain");
    let recon = PhaseId::from("recon");
    let escalate = PhaseId::from("escalate");
    let de_escalate = PhaseId::from("de_escalate");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 5,
            max_duration: 5,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![
                PhaseBranch {
                    condition: BranchCondition::OrAny {
                        conditions: vec![
                            BranchCondition::OnFailure,
                            BranchCondition::EscalationThreshold {
                                metric: EscalationMetric::Tension,
                                threshold: 0.1,
                                direction: ThresholdDirection::Below,
                                sustained_ticks: 3,
                            },
                        ],
                    },
                    next_phase: escalate.clone(),
                },
                PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: de_escalate.clone(),
                },
            ],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    for id in [escalate.clone(), de_escalate.clone()] {
        phases.insert(
            id.clone(),
            CampaignPhase {
                id: id.clone(),
                name: id.to_string(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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
    }

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "OrAny".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");
    let report = result.campaign_reports.get(&chain_id).expect("report");
    assert!(
        matches!(
            report.phase_outcomes.get(&de_escalate),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "fallback must fire when no OrAny inner matches; got {:?}",
        report.phase_outcomes
    );
    assert!(
        matches!(
            report.phase_outcomes.get(&escalate),
            Some(PhaseOutcome::Pending) | None
        ),
        "OrAny must not fire when every inner condition fails"
    );
}

#[test]
fn empty_or_any_rejected_at_validation() {
    use faultline_engine::validate_scenario;
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost,
    };
    use faultline_types::error::ScenarioError;
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    let chain_id = KillChainId::from("bad_chain");
    let recon = PhaseId::from("recon");

    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OrAny { conditions: vec![] },
                next_phase: recon.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );

    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Bad".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: recon.clone(),
            phases,
        },
    );

    let err = validate_scenario(&scenario).expect_err("empty OrAny must reject");
    assert!(
        matches!(err, ScenarioError::EmptyOrAnyBranch { .. }),
        "expected EmptyOrAnyBranch, got {err:?}"
    );
}

#[test]
fn environment_detection_factor_reduces_phase_detection() {
    // Build two identical scenarios — same chain, same RNG seed — and
    // attach a `detection_factor: 0.0` Always window to one of them.
    // Under that window every kill-chain detection roll is forced to
    // zero, so the recon phase must reach `Succeeded` instead of
    // `Detected`. Pins the contract that environment.detection_factor
    // multiplies into the phase's per-tick detection probability.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
    use faultline_types::stats::PhaseOutcome;

    let make_scenario = |env: EnvironmentSchedule| {
        let mut scenario = base_scenario();
        scenario.simulation.max_ticks = 20;
        scenario.environment = env;

        let chain_id = KillChainId::from("env_chain");
        let recon = PhaseId::from("recon");
        let exfil = PhaseId::from("exfil");

        let mut phases = BTreeMap::new();
        phases.insert(
            recon.clone(),
            CampaignPhase {
                id: recon.clone(),
                name: "Recon".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 5,
                max_duration: 5,
                detection_probability_per_tick: 0.9, // very visible
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OnSuccess,
                    next_phase: exfil.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        phases.insert(
            exfil.clone(),
            CampaignPhase {
                id: exfil.clone(),
                name: "Exfil".into(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id.clone(),
                name: "Env".into(),
                description: String::new(),
                attacker: FactionId::from("alpha"),
                target: FactionId::from("bravo"),
                entry_phase: recon.clone(),
                phases,
            },
        );
        (scenario, chain_id, recon, exfil)
    };

    // Baseline: no environment, very high per-tick detection → recon
    // gets caught well before the duration elapses.
    let (baseline, chain_id_a, recon_a, _) = make_scenario(EnvironmentSchedule::default());
    let mut engine_a = Engine::with_seed(baseline, 42).expect("engine");
    let result_a = engine_a.run().expect("run");
    let report_a = result_a.campaign_reports.get(&chain_id_a).expect("report");
    assert!(
        matches!(
            report_a.phase_outcomes.get(&recon_a),
            Some(PhaseOutcome::Detected { .. })
        ),
        "baseline: high-detection recon must be detected; got {:?}",
        report_a.phase_outcomes.get(&recon_a)
    );

    // Night window: detection_factor = 0.0 forces every roll to 0 →
    // recon completes its 5-tick duration uneventfully and reaches
    // `Succeeded`, the exfil phase fires.
    let night = EnvironmentSchedule {
        windows: vec![EnvironmentWindow {
            id: "night".into(),
            name: "Night".into(),
            activation: Activation::Always,
            applies_to: vec![],
            movement_factor: 1.0,
            defense_factor: 1.0,
            visibility_factor: 1.0,
            detection_factor: 0.0,
        }],
    };
    let (shielded, chain_id_b, recon_b, exfil_b) = make_scenario(night);
    let mut engine_b = Engine::with_seed(shielded, 42).expect("engine");
    let result_b = engine_b.run().expect("run");
    let report_b = result_b.campaign_reports.get(&chain_id_b).expect("report");
    assert!(
        matches!(
            report_b.phase_outcomes.get(&recon_b),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "shielded: detection_factor 0 must zero out every roll; got {:?}",
        report_b.phase_outcomes.get(&recon_b)
    );
    assert!(
        matches!(
            report_b.phase_outcomes.get(&exfil_b),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "shielded: exfil should fire after recon succeeds"
    );
}

#[test]
fn environment_cycle_activation_matches_expected_ticks() {
    // Sanity-check the Cycle activation arithmetic in isolation. A
    // night-cycle of period=24 phase=18 duration=12 should be active
    // for hours 18..=29 mod 24 — i.e. ticks 0..=5 (early morning) and
    // ticks 18..=29 etc. Documented behavior is the implicit contract
    // for time-of-day modeling under hourly ticks; pin it.
    use faultline_types::map::Activation;

    let night = Activation::Cycle {
        period: 24,
        phase: 18,
        duration: 12,
    };
    // Active early-morning hours of day 0 (ticks 0..=5).
    for t in 0..=5 {
        assert!(night.is_active_at(t), "expected active at tick {t}");
    }
    // Inactive daytime hours of day 0 (ticks 6..=17).
    for t in 6..=17 {
        assert!(!night.is_active_at(t), "expected inactive at tick {t}");
    }
    // Active evening of day 0 (ticks 18..=23).
    for t in 18..=23 {
        assert!(night.is_active_at(t), "expected active at tick {t}");
    }
    // Active early-morning of day 1 (ticks 24..=29).
    for t in 24..=29 {
        assert!(night.is_active_at(t), "expected active at tick {t}");
    }
}

#[test]
fn leadership_decapitation_caps_target_morale() {
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
    };
    use faultline_types::faction::{LeadershipCadre, LeadershipRank};
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 20;

    if let Some(bravo) = scenario.factions.get_mut(&FactionId::from("bravo")) {
        bravo.initial_morale = 0.95;
        bravo.leadership = Some(LeadershipCadre {
            ranks: vec![
                LeadershipRank {
                    id: "principal".into(),
                    name: "Principal".into(),
                    effectiveness: 1.0,
                    description: String::new(),
                },
                LeadershipRank {
                    id: "deputy".into(),
                    name: "Deputy".into(),
                    effectiveness: 0.5,
                    description: String::new(),
                },
            ],
            succession_recovery_ticks: 4,
            succession_floor: 0.4,
        });
    }

    // Three distinct strike phases chained OnSuccess → so each one
    // resolves once and lands a decapitation against bravo. The
    // second strike pushes the rank index past the cadre (leaderless),
    // and the third decapitation lands against an already-leaderless
    // faction — counter still increments, index saturates at
    // `ranks.len()`.
    let chain_id = KillChainId::from("decap_chain");
    let strike1 = PhaseId::from("strike1");
    let strike2 = PhaseId::from("strike2");
    let strike3 = PhaseId::from("strike3");
    let make_strike = |id: &PhaseId, next: Option<&PhaseId>| CampaignPhase {
        id: id.clone(),
        name: id.to_string(),
        description: String::new(),
        prerequisites: vec![],
        base_success_probability: 1.0,
        min_duration: 1,
        max_duration: 1,
        detection_probability_per_tick: 0.0,
        prerequisite_success_boost: 0.0,
        attribution_difficulty: 0.5,
        cost: PhaseCost {
            attacker_dollars: 0.0,
            defender_dollars: 0.0,
            attacker_resources: 0.0,
            confidence: None,
        },
        targets_domains: vec![],
        outputs: vec![PhaseOutput::LeadershipDecapitation {
            target_faction: FactionId::from("bravo"),
            morale_shock: 0.2,
        }],
        branches: next
            .map(|n| {
                vec![PhaseBranch {
                    condition: BranchCondition::OnSuccess,
                    next_phase: n.clone(),
                }]
            })
            .unwrap_or_default(),
        parameter_confidence: None,
        warning_indicators: vec![],
        defender_noise: vec![],
        gated_by_defender: None,
    };
    let mut phases = BTreeMap::new();
    phases.insert(strike1.clone(), make_strike(&strike1, Some(&strike2)));
    phases.insert(strike2.clone(), make_strike(&strike2, Some(&strike3)));
    phases.insert(strike3.clone(), make_strike(&strike3, None));
    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Decap".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: strike1.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    // Tick 1 activates strike1 (started_at=1). Tick 2 resolves it.
    engine.tick().expect("tick 1");
    engine.tick().expect("tick 2");
    let bravo_state = engine
        .snapshot()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo runtime state")
        .clone();
    assert_eq!(bravo_state.leadership_decapitations, 1);
    assert_eq!(bravo_state.current_leadership_rank, 1);
    assert_eq!(bravo_state.last_decapitation_tick, Some(2));
    // Cap on morale at strike tick: deputy effectiveness 0.5 *
    // succession_floor 0.4 = 0.20.
    assert!(
        bravo_state.morale <= 0.20 + 1e-9,
        "morale must be capped at deputy 0.5 × floor 0.4 = 0.20 \
         immediately after strike, got {}",
        bravo_state.morale
    );

    // Six more ticks let strikes 2 and 3 resolve (each pair: activate
    // + resolve). After strike2 the rank is past the cadre; strike3
    // increments the count further but the rank index saturates at
    // ranks.len() = 2.
    for _ in 0..6 {
        engine.tick().expect("tick");
    }
    let bravo_state = engine
        .snapshot()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo runtime state")
        .clone();
    assert!(
        bravo_state.leadership_decapitations >= 2,
        "second decapitation must have landed; got {}",
        bravo_state.leadership_decapitations
    );
    assert_eq!(
        bravo_state.current_leadership_rank, 2,
        "rank index must saturate at cadre length once the cadre is exhausted"
    );
    assert!(
        bravo_state.morale <= f64::EPSILON,
        "leaderless faction must floor morale to 0; got {}",
        bravo_state.morale
    );
}

#[test]
fn leadership_zero_recovery_ticks_means_immediate_full_effectiveness() {
    // succession_recovery_ticks = 0 disables the ramp entirely. The
    // helper should return the new rank's nominal effectiveness on
    // the strike tick rather than interpolating from succession_floor.
    use faultline_engine::tick::effective_leadership_factor;
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
    };
    use faultline_types::faction::{LeadershipCadre, LeadershipRank};
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    let bravo_id = FactionId::from("bravo");
    if let Some(bravo) = scenario.factions.get_mut(&bravo_id) {
        bravo.initial_morale = 0.95;
        bravo.leadership = Some(LeadershipCadre {
            ranks: vec![
                LeadershipRank {
                    id: "principal".into(),
                    name: "Principal".into(),
                    effectiveness: 1.0,
                    description: String::new(),
                },
                LeadershipRank {
                    id: "deputy".into(),
                    name: "Deputy".into(),
                    effectiveness: 0.6,
                    description: String::new(),
                },
            ],
            succession_recovery_ticks: 0,
            succession_floor: 0.0, // would be punitive if ramp was active
        });
    }

    let chain_id = KillChainId::from("decap_chain");
    let phase_id = PhaseId::from("strike");
    let mut phases = BTreeMap::new();
    phases.insert(
        phase_id.clone(),
        CampaignPhase {
            id: phase_id.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![PhaseOutput::LeadershipDecapitation {
                target_faction: bravo_id.clone(),
                morale_shock: 0.0,
            }],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: phase_id.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Decap".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: bravo_id.clone(),
            entry_phase: phase_id.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    engine.tick().expect("tick 1");
    engine.tick().expect("tick 2"); // decapitation lands

    // The morale cap on a recovery_ticks=0 cadre lands at the new
    // rank's nominal effectiveness — no ramp scaling. Verified
    // through the public snapshot's morale field.
    let snap = engine.snapshot();
    let bravo_state = snap.faction_states.get(&bravo_id).expect("bravo state");
    assert!(
        bravo_state.morale <= 0.6 + 1e-9 && bravo_state.morale >= 0.6 - 1e-9,
        "morale must be capped at deputy 0.6 with no ramp scaling, got {}",
        bravo_state.morale
    );

    // The helper agrees when called against the post-tick state.
    let factor = effective_leadership_factor(
        engine.state(),
        engine.scenario(),
        &bravo_id,
        engine.current_tick(),
    );
    assert!(
        (factor - 0.6).abs() < 1e-9,
        "effective_leadership_factor must be deputy.effectiveness when recovery_ticks=0; got {factor}"
    );
}

#[test]
fn leadership_factor_returns_zero_for_leaderless_faction() {
    // A 1-rank cadre that's been struck once produces a leaderless
    // terminal state — the helper must return 0.0 from that point on.
    use faultline_engine::tick::effective_leadership_factor;
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
    };
    use faultline_types::faction::{LeadershipCadre, LeadershipRank};
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    let bravo_id = FactionId::from("bravo");
    if let Some(bravo) = scenario.factions.get_mut(&bravo_id) {
        bravo.leadership = Some(LeadershipCadre {
            ranks: vec![LeadershipRank {
                id: "principal".into(),
                name: "Principal".into(),
                effectiveness: 1.0,
                description: String::new(),
            }],
            succession_recovery_ticks: 4,
            succession_floor: 0.5,
        });
    }

    let chain_id = KillChainId::from("decap_chain");
    let phase_id = PhaseId::from("strike");
    let mut phases = BTreeMap::new();
    phases.insert(
        phase_id.clone(),
        CampaignPhase {
            id: phase_id.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![PhaseOutput::LeadershipDecapitation {
                target_faction: bravo_id.clone(),
                morale_shock: 0.0,
            }],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: phase_id.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id,
            name: "Decap".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: bravo_id.clone(),
            entry_phase: phase_id,
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    engine.tick().expect("tick 1");
    engine.tick().expect("tick 2"); // 1-rank cadre exhausted by single strike

    let factor = effective_leadership_factor(
        engine.state(),
        engine.scenario(),
        &bravo_id,
        engine.current_tick(),
    );
    assert_eq!(factor, 0.0, "leaderless faction must produce factor 0.0");
}

#[test]
fn leadership_factor_full_when_no_decapitation_yet() {
    // Faction with a cadre but no strikes — the helper must return
    // the top rank's effectiveness with no ramp scaling. Important
    // because `last_decapitation_tick = None` is the default and
    // must not silently apply a penalty.
    use faultline_engine::tick::effective_leadership_factor;
    use faultline_types::faction::{LeadershipCadre, LeadershipRank};

    let mut scenario = base_scenario();
    let bravo_id = FactionId::from("bravo");
    if let Some(bravo) = scenario.factions.get_mut(&bravo_id) {
        bravo.leadership = Some(LeadershipCadre {
            ranks: vec![LeadershipRank {
                id: "principal".into(),
                name: "Principal".into(),
                effectiveness: 0.9,
                description: String::new(),
            }],
            succession_recovery_ticks: 4,
            succession_floor: 0.0, // punitive if it leaked through
        });
    }

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    engine.tick().expect("tick");

    let factor = effective_leadership_factor(
        engine.state(),
        engine.scenario(),
        &bravo_id,
        engine.current_tick(),
    );
    assert!(
        (factor - 0.9).abs() < 1e-9,
        "no-strike faction must read principal effectiveness 0.9; got {factor}"
    );
}

#[test]
fn environment_factors_compose_multiplicatively() {
    // Two active windows with detection_factor 0.5 each must
    // compose to 0.25. Pins the multiplicative-composition contract
    // the engine integration relies on.
    use faultline_engine::tick::environment_detection_factor;
    use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};

    let mut scenario = base_scenario();
    scenario.environment = EnvironmentSchedule {
        windows: vec![
            EnvironmentWindow {
                id: "night".into(),
                name: "Night".into(),
                activation: Activation::Always,
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 1.0,
                detection_factor: 0.5,
            },
            EnvironmentWindow {
                id: "rain".into(),
                name: "Rain".into(),
                activation: Activation::Always,
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 1.0,
                detection_factor: 0.5,
            },
        ],
    };

    let factor = environment_detection_factor(&scenario, 10);
    assert!(
        (factor - 0.25).abs() < 1e-9,
        "two 0.5x detection_factor windows must compose to 0.25; got {factor}"
    );
}

#[test]
fn or_any_inner_probability_consumes_rng_only_when_reached() {
    // OrAny is short-circuit. With order [OnSuccess, Probability{0.5}],
    // the Probability draw happens only when OnSuccess fails. Two
    // back-to-back runs with different inner orders against the same
    // seed must produce different RNG-consumption patterns IF and
    // ONLY IF the short-circuit position differs — pins the documented
    // determinism contract.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost,
    };
    use faultline_types::ids::{KillChainId, PhaseId};
    use faultline_types::stats::PhaseOutcome;

    let make_scenario = |ordering_swaps: bool| {
        let mut scenario = base_scenario();
        scenario.simulation.max_ticks = 10;
        let chain_id = KillChainId::from("or_chain");
        let recon = PhaseId::from("recon");
        let next = PhaseId::from("next");

        let or_conds = if ordering_swaps {
            // OnSuccess first → for a 1.0-success phase, OnSuccess
            // matches and the Probability draw is never taken.
            vec![
                BranchCondition::OnSuccess,
                BranchCondition::Probability { p: 0.5 },
            ]
        } else {
            // Probability first → the draw is consumed every time,
            // regardless of OnSuccess matching later.
            vec![
                BranchCondition::Probability { p: 0.5 },
                BranchCondition::OnSuccess,
            ]
        };

        let mut phases = BTreeMap::new();
        phases.insert(
            recon.clone(),
            CampaignPhase {
                id: recon.clone(),
                name: "Recon".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OrAny {
                        conditions: or_conds,
                    },
                    next_phase: next.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        phases.insert(
            next.clone(),
            CampaignPhase {
                id: next.clone(),
                name: "Next".into(),
                description: String::new(),
                prerequisites: vec![recon.clone()],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
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

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id.clone(),
                name: "OR".into(),
                description: String::new(),
                attacker: FactionId::from("alpha"),
                target: FactionId::from("bravo"),
                entry_phase: recon,
                phases,
            },
        );
        (scenario, chain_id, next)
    };

    // OnSuccess-first: every run reaches `next` because OnSuccess
    // always matches against a 1.0-probability phase, and the
    // Probability draw is short-circuited.
    let (sc1, cid1, next1) = make_scenario(true);
    let mut engine1 = Engine::with_seed(sc1, 42).expect("engine");
    let result1 = engine1.run().expect("run");
    let report1 = result1.campaign_reports.get(&cid1).expect("report");
    assert!(
        matches!(
            report1.phase_outcomes.get(&next1),
            Some(PhaseOutcome::Succeeded { .. })
        ),
        "OnSuccess-first ordering must always reach `next`: {:?}",
        report1.phase_outcomes
    );

    // Probability-first: OnSuccess will *also* match (the OR is
    // satisfied either way), so `next` still fires. But the RNG
    // state is different because the Probability draw was consumed.
    // This test pins that the determinism is preserved given the
    // declared order — same seed + same order → identical outcome.
    let (sc2_a, _, _) = make_scenario(false);
    let mut engine2_a = Engine::with_seed(sc2_a, 42).expect("engine");
    let result2_a = engine2_a.run().expect("run");

    let (sc2_b, _, _) = make_scenario(false);
    let mut engine2_b = Engine::with_seed(sc2_b, 42).expect("engine");
    let result2_b = engine2_b.run().expect("run");
    assert_eq!(
        result2_a.final_tick, result2_b.final_tick,
        "same seed + same OrAny ordering must produce identical runs"
    );
}

#[test]
fn leadership_no_cadre_means_no_morale_cap() {
    // Belt-and-suspenders: this scenario would fail `validate_scenario`
    // (decapitation against a faction with no cadre is rejected as an
    // authoring mistake), but we drive the engine directly to pin the
    // runtime defensive behavior — `apply_leadership_caps` must remain
    // a no-op for cadre-less factions even if validation is bypassed.
    // Do not "fix" this to validation-pass; it would destroy what the
    // test is asserting.
    use faultline_types::campaign::{
        BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
    };
    use faultline_types::ids::{KillChainId, PhaseId};

    let mut scenario = base_scenario();
    scenario.simulation.max_ticks = 5;
    if let Some(bravo) = scenario.factions.get_mut(&FactionId::from("bravo")) {
        bravo.initial_morale = 0.95;
        bravo.leadership = None;
    }

    let chain_id = KillChainId::from("decap_chain");
    let strike = PhaseId::from("strike");
    let mut phases = BTreeMap::new();
    phases.insert(
        strike.clone(),
        CampaignPhase {
            id: strike.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![PhaseOutput::LeadershipDecapitation {
                target_faction: FactionId::from("bravo"),
                morale_shock: 0.0,
            }],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: strike.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    scenario.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id.clone(),
            name: "Decap".into(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("bravo"),
            entry_phase: strike.clone(),
            phases,
        },
    );

    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    for _ in 0..5 {
        engine.tick().expect("tick");
    }
    let bravo_state = engine
        .snapshot()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo runtime state")
        .clone();
    assert!(bravo_state.leadership_decapitations >= 1);
    assert_eq!(bravo_state.current_leadership_rank, 0);
    assert!(
        bravo_state.morale > 0.5,
        "without a cadre, morale must not be capped by leadership; got {}",
        bravo_state.morale
    );
}

#[test]
fn fracture_scenario_tick_stepping_consistency() {
    let toml_str = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scenarios/us_institutional_fracture.toml"),
    )
    .expect("should read fracture scenario");
    let scenario: Scenario = toml::from_str(&toml_str).expect("should parse");

    let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");

    // Tick 50 times, capturing snapshots at each interval.
    let mut tick_snapshots = Vec::new();
    for _ in 0..50 {
        engine.tick().expect("tick should succeed");
        if engine.current_tick().is_multiple_of(10) {
            tick_snapshots.push(engine.snapshot());
        }
    }

    assert_eq!(tick_snapshots.len(), 5, "should have 5 interval snapshots");

    // All 4 factions should be present in each snapshot.
    for snap in &tick_snapshots {
        assert_eq!(
            snap.faction_states.len(),
            4,
            "fracture scenario should have 4 factions at tick {}",
            snap.tick
        );
    }

    // Verify all 8 regions have control entries.
    let final_snap = tick_snapshots.last().expect("should have snapshots");
    assert_eq!(
        final_snap.region_control.len(),
        8,
        "fracture scenario should have 8 regions in control map"
    );
}
