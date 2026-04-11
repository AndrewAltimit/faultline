use std::collections::BTreeMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use faultline_types::faction::{ForceUnit, UnitType};
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
        snapshots: vec![],
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
    let _initial_actions = ai::evaluate_actions(&alpha, &state, &scenario, &map, &mut rng);
    state
        .faction_states
        .get_mut(&bravo)
        .expect("bravo should exist")
        .forces
        .get_mut(&ForceId::from("bravo_threat"))
        .expect("bravo_threat should exist")
        .region = RegionId::from("nw");

    let actions = ai::evaluate_actions(&alpha, &state, &scenario, &map, &mut rng);

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
            },
        );

    // Mark ne as bravo-controlled.
    state
        .region_control
        .insert(RegionId::from("ne"), Some(bravo.clone()));

    let alpha = FactionId::from("alpha");
    let scenario = minimal_scenario();
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    let actions = ai::evaluate_actions(&alpha, &state, &scenario, &map, &mut rng);

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
