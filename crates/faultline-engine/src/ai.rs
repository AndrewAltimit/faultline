//! Basic utility-based faction AI for decision making.

use std::collections::BTreeMap;

use rand::Rng;

use faultline_geo::{GameMap, adjacent_regions};
use faultline_types::faction::FactionType;
use faultline_types::ids::{FactionId, RegionId};
use faultline_types::strategy::FactionAction;

use crate::state::{RuntimeFactionState, SimulationState};

/// Weights used to score candidate actions.
#[derive(Clone, Debug)]
pub struct AiWeights {
    pub survival_weight: f64,
    pub objective_weight: f64,
    pub opportunity_weight: f64,
    pub risk_aversion: f64,
}

impl AiWeights {
    /// Default weights for a given faction type.
    pub fn for_faction_type(faction_type: &FactionType) -> Self {
        match faction_type {
            FactionType::Insurgent => Self {
                survival_weight: 0.4,
                objective_weight: 0.3,
                opportunity_weight: 0.25,
                risk_aversion: 0.6,
            },
            FactionType::Military { .. } => Self {
                survival_weight: 0.2,
                objective_weight: 0.5,
                opportunity_weight: 0.2,
                risk_aversion: 0.3,
            },
            FactionType::Government { .. } => Self {
                survival_weight: 0.3,
                objective_weight: 0.4,
                opportunity_weight: 0.15,
                risk_aversion: 0.5,
            },
            FactionType::PrivateMilitary => Self {
                survival_weight: 0.35,
                objective_weight: 0.35,
                opportunity_weight: 0.25,
                risk_aversion: 0.4,
            },
            FactionType::Civilian | FactionType::Foreign { .. } => Self {
                survival_weight: 0.5,
                objective_weight: 0.2,
                opportunity_weight: 0.1,
                risk_aversion: 0.8,
            },
        }
    }
}

/// A scored candidate action.
#[derive(Clone, Debug)]
pub struct ScoredAction {
    pub action: FactionAction,
    pub score: f64,
}

/// Evaluate and return a prioritized list of actions for a faction.
pub fn evaluate_actions(
    faction_id: &FactionId,
    state: &SimulationState,
    map: &GameMap,
    rng: &mut impl Rng,
) -> Vec<ScoredAction> {
    let faction_state = match state.faction_states.get(faction_id) {
        Some(fs) => fs,
        None => return Vec::new(),
    };

    if faction_state.eliminated {
        return Vec::new();
    }

    // Determine which factions are hostile (simplification: any
    // faction that controls a region we want or vice versa).
    let enemy_presence = compute_enemy_presence(faction_id, state);

    let weights = determine_weights(faction_id, state);

    let mut actions = Vec::new();

    // Evaluate defend actions for regions under threat.
    evaluate_defend_actions(
        faction_id,
        faction_state,
        &enemy_presence,
        &weights,
        &mut actions,
    );

    // Evaluate attack actions against adjacent enemy regions.
    evaluate_attack_actions(
        faction_id,
        faction_state,
        state,
        map,
        &weights,
        rng,
        &mut actions,
    );

    // Evaluate move actions toward strategic objectives.
    evaluate_move_actions(
        faction_id,
        faction_state,
        state,
        map,
        &weights,
        &mut actions,
    );

    // Evaluate recruit action if resources allow.
    evaluate_recruit_actions(faction_state, &weights, &mut actions);

    // Sort by score descending.
    actions.sort_by(|a, b| b.score.total_cmp(&a.score));

    actions
}

// -----------------------------------------------------------------------
// Internal helpers
// -----------------------------------------------------------------------

fn determine_weights(faction_id: &FactionId, state: &SimulationState) -> AiWeights {
    let faction_state = match state.faction_states.get(faction_id) {
        Some(fs) => fs,
        None => return AiWeights::for_faction_type(&FactionType::Civilian),
    };

    // Use morale-adjusted weights: low morale -> more survival focused.
    let mut weights = AiWeights {
        survival_weight: 0.3,
        objective_weight: 0.4,
        opportunity_weight: 0.2,
        risk_aversion: 0.4,
    };

    if faction_state.morale < 0.3 {
        weights.survival_weight = 0.6;
        weights.objective_weight = 0.2;
        weights.risk_aversion = 0.7;
    } else if faction_state.morale > 0.7 {
        weights.objective_weight = 0.5;
        weights.opportunity_weight = 0.3;
        weights.risk_aversion = 0.2;
    }

    weights
}

/// Compute which regions have enemy forces and their total strength.
fn compute_enemy_presence(
    faction_id: &FactionId,
    state: &SimulationState,
) -> BTreeMap<RegionId, f64> {
    let mut presence = BTreeMap::new();
    for (fid, fs) in &state.faction_states {
        if fid == faction_id || fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            *presence.entry(force.region.clone()).or_insert(0.0) += force.strength;
        }
    }
    presence
}

/// Score defend actions for regions where we have forces and enemies
/// are nearby.
fn evaluate_defend_actions(
    _faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    enemy_presence: &BTreeMap<RegionId, f64>,
    weights: &AiWeights,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let threat = enemy_presence.get(&force.region).copied().unwrap_or(0.0);

        if threat > 0.0 {
            let threat_ratio = threat / force.strength.max(1.0);
            let score = weights.survival_weight * threat_ratio + weights.risk_aversion * 0.5;

            actions.push(ScoredAction {
                action: FactionAction::Defend {
                    force: force.id.clone(),
                    region: force.region.clone(),
                },
                score,
            });
        }
    }
}

/// Score attack actions toward adjacent regions with enemy forces
/// or uncontrolled strategic regions.
fn evaluate_attack_actions(
    faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    state: &SimulationState,
    map: &GameMap,
    weights: &AiWeights,
    rng: &mut impl Rng,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let neighbors = adjacent_regions(&force.region, map);
        for neighbor in &neighbors {
            // Check if this region is controlled by an enemy.
            let enemy_controlled = state
                .region_control
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref())
                .is_some_and(|ctrl| ctrl != faction_id);

            if !enemy_controlled {
                continue;
            }

            let strategic_value = map.regions.get(neighbor).map_or(1.0, |r| r.strategic_value);

            // Small random factor for variety.
            let noise: f64 = rng.r#gen::<f64>() * 0.1;

            let score = weights.objective_weight * strategic_value * 0.1
                + weights.opportunity_weight * 0.3
                + noise
                - weights.risk_aversion * 0.2;

            if score > 0.0 {
                actions.push(ScoredAction {
                    action: FactionAction::Attack {
                        force: force.id.clone(),
                        target_region: neighbor.clone(),
                    },
                    score,
                });
            }
        }
    }
}

/// Score move actions toward high-value unoccupied regions.
fn evaluate_move_actions(
    faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    state: &SimulationState,
    map: &GameMap,
    weights: &AiWeights,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let neighbors = adjacent_regions(&force.region, map);
        for neighbor in &neighbors {
            let is_ours = state
                .region_control
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref())
                .is_some_and(|ctrl| ctrl == faction_id);

            if is_ours {
                continue;
            }

            // Only move to unclaimed regions (attacks handle enemy).
            let is_unclaimed = state
                .region_control
                .get(neighbor)
                .is_none_or(|ctrl| ctrl.is_none());

            if !is_unclaimed {
                continue;
            }

            let strategic_value = map.regions.get(neighbor).map_or(1.0, |r| r.strategic_value);

            let score = weights.objective_weight * strategic_value * 0.05
                + weights.opportunity_weight * 0.2;

            if score > 0.0 {
                actions.push(ScoredAction {
                    action: FactionAction::MoveUnit {
                        force: force.id.clone(),
                        destination: neighbor.clone(),
                    },
                    score,
                });
            }
        }
    }
}

/// Score recruitment if the faction has resources and a controlled
/// region.
fn evaluate_recruit_actions(
    faction_state: &RuntimeFactionState,
    weights: &AiWeights,
    actions: &mut Vec<ScoredAction>,
) {
    // Only recruit if resources are above a threshold.
    if faction_state.resources < 50.0 {
        return;
    }

    if let Some(region) = faction_state.controlled_regions.first() {
        let score = weights.survival_weight * 0.4 + weights.objective_weight * 0.1;

        actions.push(ScoredAction {
            action: FactionAction::Recruit {
                region: region.clone(),
            },
            score,
        });
    }
}
