use std::collections::BTreeMap;

use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use faultline_types::faction::{Diplomacy, DiplomaticStance, ForceUnit, UnitType};
use faultline_types::ids::{FactionId, ForceId, RegionId};
use faultline_types::strategy::{Doctrine, FactionAction};

use crate::ai::{self, AiWeights};
use crate::state::{RuntimeFactionState, SimulationState};
use crate::tests::minimal_scenario;

/// Build a minimal `SimulationState` with two factions and four regions.
/// Alpha controls nw, bravo controls se.
fn make_ai_test_state() -> SimulationState {
    let alpha = FactionId::from("alpha");
    let bravo = FactionId::from("bravo");
    let nw = RegionId::from("nw");
    let ne = RegionId::from("ne");
    let sw = RegionId::from("sw");
    let se = RegionId::from("se");

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
            move_progress: 0.0,
        },
    );

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
            move_progress: 0.0,
        },
    );

    let mut faction_states = BTreeMap::new();
    faction_states.insert(
        alpha.clone(),
        RuntimeFactionState {
            faction_id: alpha.clone(),
            total_strength: 100.0,
            morale: 0.8,
            resources: 200.0,
            resource_rate: 10.0,
            logistics_capacity: 50.0,
            controlled_regions: vec![nw.clone()],
            forces: alpha_forces,
            tech_deployed: vec![],
            region_hold_ticks: BTreeMap::new(),
            eliminated: false,
            current_leadership_rank: 0,
            last_decapitation_tick: None,
            leadership_decapitations: 0,
            command_effectiveness: 1.0,
            current_supply_pressure: 1.0,
            supply_pressure_sum: 0.0,
            supply_pressure_samples: 0,
            supply_pressure_min: 1.0,
            supply_pressure_pressured_ticks: 0,
            tech_denied_at_deployment: Vec::new(),
            tech_decommissioned: Vec::new(),
            tech_deployment_spend: 0.0,
            tech_maintenance_spend: 0.0,
            tech_coverage_used: BTreeMap::new(),
        },
    );
    faction_states.insert(
        bravo.clone(),
        RuntimeFactionState {
            faction_id: bravo.clone(),
            total_strength: 100.0,
            morale: 0.8,
            resources: 200.0,
            resource_rate: 10.0,
            logistics_capacity: 50.0,
            controlled_regions: vec![se.clone()],
            forces: bravo_forces,
            tech_deployed: vec![],
            region_hold_ticks: BTreeMap::new(),
            eliminated: false,
            current_leadership_rank: 0,
            last_decapitation_tick: None,
            leadership_decapitations: 0,
            command_effectiveness: 1.0,
            current_supply_pressure: 1.0,
            supply_pressure_sum: 0.0,
            supply_pressure_samples: 0,
            supply_pressure_min: 1.0,
            supply_pressure_pressured_ticks: 0,
            tech_denied_at_deployment: Vec::new(),
            tech_decommissioned: Vec::new(),
            tech_deployment_spend: 0.0,
            tech_maintenance_spend: 0.0,
            tech_coverage_used: BTreeMap::new(),
        },
    );

    let mut region_control = BTreeMap::new();
    region_control.insert(nw, Some(alpha));
    region_control.insert(ne, None);
    region_control.insert(sw, None);
    region_control.insert(se, Some(bravo));

    SimulationState {
        tick: 1,
        faction_states,
        region_control,
        infra_status: BTreeMap::new(),
        institution_loyalty: BTreeMap::new(),
        political_climate: faultline_types::politics::PoliticalClimate {
            tension: 0.3,
            institutional_trust: 0.7,
            media_landscape: faultline_types::politics::MediaLandscape {
                fragmentation: 0.3,
                disinformation_susceptibility: 0.2,
                state_control: 0.1,
                social_media_penetration: 0.5,
                internet_availability: 0.9,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events_fired: std::collections::BTreeSet::new(),
        events_fired_this_tick: vec![],
        snapshots: vec![],
        non_kinetic: Default::default(),
        metric_history: vec![],
        defender_queues: BTreeMap::new(),
        network_states: std::collections::BTreeMap::new(),
        defender_over_budget_tick: None,
        diplomacy_overrides: BTreeMap::new(),
        fired_fractures: std::collections::BTreeSet::new(),
        initial_faction_strengths: BTreeMap::new(),
        fracture_events: vec![],
        civilian_activations: vec![],
        narratives: BTreeMap::new(),
        narrative_events: vec![],
        narrative_dominance_ticks: BTreeMap::new(),
        narrative_peak_dominance: BTreeMap::new(),
        displacement: BTreeMap::new(),
        utility_decisions: BTreeMap::new(),
    }
}

/// Build a minimal GameMap matching the test state (4 regions, square).
fn make_ai_test_map() -> faultline_geo::GameMap {
    let nw = RegionId::from("nw");
    let ne = RegionId::from("ne");
    let sw = RegionId::from("sw");
    let se = RegionId::from("se");

    let mut regions = BTreeMap::new();
    for (rid, name, sv) in [
        (nw.clone(), "North-West", 1.0),
        (ne.clone(), "North-East", 1.0),
        (sw.clone(), "South-West", 1.0),
        (se.clone(), "South-East", 1.0),
    ] {
        regions.insert(
            rid.clone(),
            faultline_geo::RegionInfo {
                id: rid,
                name: name.into(),
                population: 500_000,
                urbanization: 0.5,
                strategic_value: sv,
            },
        );
    }

    let mut adjacency = BTreeMap::new();
    adjacency.insert(nw.clone(), vec![ne.clone(), sw.clone()]);
    adjacency.insert(ne.clone(), vec![nw.clone(), se.clone()]);
    adjacency.insert(sw.clone(), vec![nw.clone(), se.clone()]);
    adjacency.insert(se.clone(), vec![ne.clone(), sw.clone()]);

    faultline_geo::GameMap {
        regions,
        adjacency,
        movement_costs: BTreeMap::new(),
    }
}

#[test]
fn ai_weights_conventional_doctrine() {
    let weights = AiWeights::for_doctrine(&Doctrine::Conventional);
    assert!(
        weights.objective_weight >= 0.4,
        "conventional should have high objective_weight: {}",
        weights.objective_weight
    );
    assert!(
        weights.objective_weight > weights.survival_weight,
        "conventional objective_weight ({}) should exceed \
         survival_weight ({})",
        weights.objective_weight,
        weights.survival_weight,
    );
}

#[test]
fn ai_weights_guerrilla_doctrine() {
    let weights = AiWeights::for_doctrine(&Doctrine::Guerrilla);
    assert!(
        weights.survival_weight >= 0.4,
        "guerrilla should have high survival_weight: {}",
        weights.survival_weight
    );
    assert!(
        weights.survival_weight > weights.objective_weight,
        "guerrilla survival_weight ({}) should exceed \
         objective_weight ({})",
        weights.survival_weight,
        weights.objective_weight,
    );
}

#[test]
fn ai_weights_blitzkrieg_doctrine() {
    let weights = AiWeights::for_doctrine(&Doctrine::Blitzkrieg);
    assert!(
        weights.objective_weight >= 0.5,
        "blitzkrieg should have very high objective_weight: {}",
        weights.objective_weight
    );
    assert!(
        weights.risk_aversion < 0.2,
        "blitzkrieg should have low risk_aversion: {}",
        weights.risk_aversion
    );
}

#[test]
fn ai_weights_defensive_doctrine() {
    let weights = AiWeights::for_doctrine(&Doctrine::Defensive);
    assert!(
        weights.survival_weight >= 0.5,
        "defensive should have very high survival_weight: {}",
        weights.survival_weight
    );
    assert!(
        weights.risk_aversion >= 0.7,
        "defensive should have high risk_aversion: {}",
        weights.risk_aversion
    );
}

#[test]
fn ai_evaluates_defend_for_threatened_region() {
    let mut state = make_ai_test_state();
    let map = make_ai_test_map();

    // Place bravo force in ne (adjacent to alpha in nw) to threaten.
    let bravo = FactionId::from("bravo");
    state
        .faction_states
        .get_mut(&bravo)
        .expect("bravo should exist")
        .forces
        .insert(
            ForceId::from("bravo_threat"),
            ForceUnit {
                id: ForceId::from("bravo_threat"),
                name: "Bravo Threat".into(),
                unit_type: UnitType::Infantry,
                region: RegionId::from("ne"),
                strength: 80.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 2.0,
                morale_modifier: 0.0,
                capabilities: vec![],
                move_progress: 0.0,
            },
        );

    // Also mark ne as bravo-controlled so the AI sees an enemy region.
    state
        .region_control
        .insert(RegionId::from("ne"), Some(bravo.clone()));

    let alpha = FactionId::from("alpha");
    let scenario = minimal_scenario();
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    // The defend logic checks for enemy presence in the same region
    // as our force, so place bravo directly in nw to trigger defense.
    let _initial_actions =
        ai::evaluate_actions(&alpha, &state, &scenario, &map, &BTreeMap::new(), &mut rng);
    state
        .faction_states
        .get_mut(&bravo)
        .expect("bravo should exist")
        .forces
        .get_mut(&ForceId::from("bravo_threat"))
        .expect("bravo_threat should exist")
        .region = RegionId::from("nw");

    let actions =
        ai::evaluate_actions(&alpha, &state, &scenario, &map, &BTreeMap::new(), &mut rng).actions;

    let has_defend = actions.iter().any(|sa| {
        matches!(&sa.action, FactionAction::Defend { force, region }
            if *force == ForceId::from("alpha_inf")
            && *region == RegionId::from("nw"))
    });
    assert!(
        has_defend,
        "alpha should generate a defend action when enemy is in its \
         region, got: {:?}",
        actions.iter().map(|a| &a.action).collect::<Vec<_>>(),
    );
}

#[test]
fn ai_evaluates_attack_for_weak_enemy() {
    let mut state = make_ai_test_state();
    let map = make_ai_test_map();

    // Place a weak bravo force in ne (adjacent to alpha's nw).
    let bravo = FactionId::from("bravo");
    state
        .faction_states
        .get_mut(&bravo)
        .expect("bravo should exist")
        .forces
        .insert(
            ForceId::from("bravo_weak"),
            ForceUnit {
                id: ForceId::from("bravo_weak"),
                name: "Bravo Weak".into(),
                unit_type: UnitType::Infantry,
                region: RegionId::from("ne"),
                strength: 10.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
                move_progress: 0.0,
            },
        );

    // Mark ne as bravo-controlled.
    state
        .region_control
        .insert(RegionId::from("ne"), Some(bravo.clone()));

    let alpha = FactionId::from("alpha");
    let scenario = minimal_scenario();
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    let actions =
        ai::evaluate_actions(&alpha, &state, &scenario, &map, &BTreeMap::new(), &mut rng).actions;

    let has_attack = actions.iter().any(|sa| {
        matches!(&sa.action, FactionAction::Attack { target_region, .. }
            if *target_region == RegionId::from("ne"))
    });
    assert!(
        has_attack,
        "alpha should generate an attack action toward weak enemy in \
         adjacent region, got: {:?}",
        actions.iter().map(|a| &a.action).collect::<Vec<_>>(),
    );
}

/// Regression: an `Allied` declaration toward one neighbor must not
/// shift the RNG sequence consumed by `evaluate_attack_actions` for
/// the remaining neighbors in the same loop iteration. The early
/// `continue` on `priority_multiplier == 0.0` previously fired
/// *before* the per-neighbor noise draw, desyncing replay for any
/// scenario that gained an `Allied` declaration. The fix moves the
/// draw above the multiplier check; this test pins it.
#[test]
fn allied_neighbor_preserves_downstream_rng_state() {
    use faultline_types::scenario::Scenario;
    let alpha = FactionId::from("alpha");
    let bravo = FactionId::from("bravo");
    let charlie = FactionId::from("charlie");
    let sw = RegionId::from("sw");

    // Two scenarios that differ only in alpha's diplomacy toward
    // bravo. Both have alpha facing two enemy-controlled neighbors
    // (ne controlled by bravo, sw controlled by charlie). Under the
    // fix, the RNG draw for the bravo neighbor still happens before
    // the multiplier-zero early-continue, so the next f64 sampled
    // off the RNG matches across arms.
    let mut neutral_scenario = minimal_scenario();
    neutral_scenario.factions.insert(
        alpha.clone(),
        faultline_types::faction::Faction {
            id: alpha.clone(),
            name: "Alpha".into(),
            faction_type: faultline_types::faction::FactionType::Military {
                branch: faultline_types::faction::MilitaryBranch::Army,
            },
            description: String::new(),
            color: "#000000".into(),
            forces: BTreeMap::new(),
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 50.0,
            initial_resources: 1_000.0,
            resource_rate: 10.0,
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
        },
    );
    let alpha_def = neutral_scenario
        .factions
        .get(&alpha)
        .cloned()
        .expect("alpha was just inserted");
    let mut bravo_def = alpha_def.clone();
    bravo_def.id = bravo.clone();
    bravo_def.name = "Bravo".into();
    let mut charlie_def = alpha_def.clone();
    charlie_def.id = charlie.clone();
    charlie_def.name = "Charlie".into();
    neutral_scenario.factions.insert(bravo.clone(), bravo_def);
    neutral_scenario
        .factions
        .insert(charlie.clone(), charlie_def);

    let mut allied_scenario: Scenario = neutral_scenario.clone();
    allied_scenario
        .factions
        .get_mut(&alpha)
        .expect("alpha")
        .diplomacy = vec![DiplomaticStance {
        target_faction: bravo.clone(),
        stance: Diplomacy::Allied,
    }];

    // State: alpha controls nw, bravo controls ne (alpha's neighbor),
    // charlie controls sw (alpha's other neighbor). Two
    // enemy-controlled neighbors per loop iteration.
    let mut state = make_ai_test_state();
    state.faction_states.insert(charlie.clone(), {
        let mut fs = state
            .faction_states
            .get(&bravo)
            .cloned()
            .expect("bravo state exists");
        fs.faction_id = charlie.clone();
        fs.controlled_regions = vec![sw.clone()];
        fs.forces = BTreeMap::new();
        fs.forces.insert(
            ForceId::from("charlie_inf"),
            ForceUnit {
                id: ForceId::from("charlie_inf"),
                name: "Charlie Infantry".into(),
                unit_type: UnitType::Infantry,
                region: sw.clone(),
                strength: 50.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
                move_progress: 0.0,
            },
        );
        fs
    });
    state
        .region_control
        .insert(RegionId::from("ne"), Some(bravo));
    state.region_control.insert(sw.clone(), Some(charlie));

    let map = make_ai_test_map();

    // Same seed, same loop body, two scenario arms. Drain the AI
    // call, then sample one more f64 — under the fix the post-call
    // RNG state is identical across arms.
    let mut rng_neutral = ChaCha8Rng::seed_from_u64(7);
    let _ = ai::evaluate_actions(
        &alpha,
        &state,
        &neutral_scenario,
        &map,
        &BTreeMap::new(),
        &mut rng_neutral,
    );
    let next_neutral: f64 = rng_neutral.r#gen();

    let mut rng_allied = ChaCha8Rng::seed_from_u64(7);
    let _ = ai::evaluate_actions(
        &alpha,
        &state,
        &allied_scenario,
        &map,
        &BTreeMap::new(),
        &mut rng_allied,
    );
    let next_allied: f64 = rng_allied.r#gen();

    assert_eq!(
        next_neutral, next_allied,
        "Allied declaration toward one neighbor must not shift the RNG \
         sequence for un-affected neighbors. If this test fails, the \
         RNG draw in evaluate_attack_actions has drifted back below \
         the priority_multiplier == 0.0 early-continue."
    );
}
