//! Basic utility-based faction AI for decision making.

use std::collections::BTreeMap;

use rand::Rng;

use std::collections::BTreeSet;

use faultline_geo::{GameMap, adjacent_regions};
use faultline_types::faction::UnitCapability;
use faultline_types::ids::{FactionId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::strategy::{
    DetectedForce, Doctrine, FactionAction, FactionWorldView, PoliticalClimateView,
};

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
    /// Base weights for a given doctrine variant.
    pub fn for_doctrine(doctrine: &Doctrine) -> Self {
        match doctrine {
            Doctrine::Conventional => Self {
                survival_weight: 0.3,
                objective_weight: 0.4,
                opportunity_weight: 0.2,
                risk_aversion: 0.4,
            },
            Doctrine::Guerrilla => Self {
                survival_weight: 0.5,
                objective_weight: 0.2,
                opportunity_weight: 0.2,
                risk_aversion: 0.7,
            },
            Doctrine::Defensive => Self {
                survival_weight: 0.6,
                objective_weight: 0.15,
                opportunity_weight: 0.1,
                risk_aversion: 0.75,
            },
            Doctrine::Disruption => Self {
                survival_weight: 0.15,
                objective_weight: 0.3,
                opportunity_weight: 0.5,
                risk_aversion: 0.4,
            },
            Doctrine::CounterInsurgency => Self {
                survival_weight: 0.35,
                objective_weight: 0.35,
                opportunity_weight: 0.15,
                risk_aversion: 0.3,
            },
            Doctrine::Blitzkrieg => Self {
                survival_weight: 0.15,
                objective_weight: 0.6,
                opportunity_weight: 0.2,
                risk_aversion: 0.15,
            },
            Doctrine::Adaptive => Self {
                survival_weight: 0.3,
                objective_weight: 0.4,
                opportunity_weight: 0.2,
                risk_aversion: 0.4,
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
    scenario: &Scenario,
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

    // Determine which factions are hostile, weighted by diplomatic
    // stance. Allied factions contribute 0× to perceived threat;
    // Cooperative neighbors contribute 0.3×.
    let enemy_presence = compute_enemy_presence(faction_id, state, scenario);

    let weights = determine_weights(faction_id, state, scenario);

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
        scenario,
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

fn determine_weights(
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
) -> AiWeights {
    let faction_state = match state.faction_states.get(faction_id) {
        Some(fs) => fs,
        None => return AiWeights::for_doctrine(&Doctrine::Conventional),
    };

    let doctrine = scenario
        .factions
        .get(faction_id)
        .map_or(&Doctrine::Conventional, |f| &f.doctrine);

    let mut weights = AiWeights::for_doctrine(doctrine);

    // Adaptive doctrine and all doctrines get morale-based adjustments.
    // For Adaptive, morale is the primary driver; for others, it's a
    // smaller secondary adjustment.
    let morale_strength = if *doctrine == Doctrine::Adaptive {
        1.0
    } else {
        0.5
    };

    // Read effective combat morale (raw morale × command_effectiveness)
    // rather than raw morale. R3-4 split the two axes so a faction with
    // intact rank-and-file morale but degraded command — e.g. just had
    // its top leader killed and is in the recovery ramp — correctly
    // shifts toward defensive posture rather than continuing to behave
    // as if its full offensive capability were available.
    let effective_morale = crate::tick::effective_combat_morale(faction_state);
    if effective_morale < 0.3 {
        weights.survival_weight += 0.2 * morale_strength;
        weights.objective_weight -= 0.15 * morale_strength;
        weights.risk_aversion += 0.2 * morale_strength;
    } else if effective_morale > 0.7 {
        weights.objective_weight += 0.1 * morale_strength;
        weights.opportunity_weight += 0.1 * morale_strength;
        weights.risk_aversion -= 0.15 * morale_strength;
    }

    weights
}

/// Compute which regions have enemy forces and their total strength.
///
/// Each contributing faction's strength is weighted by
/// [`crate::diplomacy::ai_threat_multiplier`] from `faction_id`'s
/// perspective: `Allied` factions are excluded entirely (0.0×),
/// `Cooperative` neighbors are de-rated to
/// [`crate::diplomacy::COOPERATIVE_AI_FACTOR`], everyone else
/// contributes at full strength. This is the "AI de-prioritizes
/// Cooperative neighbors" half of the diplomacy coupling — the AI
/// doesn't size up defenses against allies, and only modestly
/// against partners.
fn compute_enemy_presence(
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
) -> BTreeMap<RegionId, f64> {
    let mut presence = BTreeMap::new();
    for (fid, fs) in &state.faction_states {
        if fid == faction_id || fs.eliminated {
            continue;
        }
        let multiplier = crate::diplomacy::ai_threat_multiplier(state, scenario, faction_id, fid);
        if multiplier == 0.0 {
            continue;
        }
        for force in fs.forces.values() {
            *presence.entry(force.region.clone()).or_insert(0.0) += force.strength * multiplier;
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
///
/// The controller's diplomatic stance is consulted via
/// [`crate::diplomacy::ai_threat_multiplier`]: `Allied` controllers
/// are skipped entirely (the AI never targets sworn allies), and
/// `Cooperative` controllers' attack scores are de-rated by
/// [`crate::diplomacy::COOPERATIVE_AI_FACTOR`] — the AI may still
/// queue an attack against a Cooperative neighbor, but it will
/// almost always be outranked by a true-enemy alternative.
#[allow(clippy::too_many_arguments)]
fn evaluate_attack_actions(
    faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    state: &SimulationState,
    scenario: &Scenario,
    map: &GameMap,
    weights: &AiWeights,
    rng: &mut impl Rng,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let neighbors = adjacent_regions(&force.region, map);
        for neighbor in &neighbors {
            // Check if this region is controlled by an enemy.
            let controller = state
                .region_control
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref());
            let enemy_controlled = controller.is_some_and(|ctrl| ctrl != faction_id);

            if !enemy_controlled {
                continue;
            }

            // Draw the noise unconditionally per enemy-controlled
            // neighbor *before* consulting diplomacy. Adding an
            // `Allied` declaration to a pre-existing scenario must
            // not shift the RNG sequence for the remaining neighbors
            // in this loop iteration — that would break
            // bit-identical replay against legacy seeds.
            let noise: f64 = rng.r#gen::<f64>() * 0.1;

            let priority_multiplier = controller
                .map(|ctrl| {
                    crate::diplomacy::ai_threat_multiplier(state, scenario, faction_id, ctrl)
                })
                .unwrap_or(1.0);
            if priority_multiplier == 0.0 {
                continue;
            }

            let strategic_value = map.regions.get(neighbor).map_or(1.0, |r| r.strategic_value);

            let score = (weights.objective_weight * strategic_value * 0.1
                + weights.opportunity_weight * 0.3
                + noise
                - weights.risk_aversion * 0.2)
                * priority_multiplier;

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

// -----------------------------------------------------------------------
// Fog of war
// -----------------------------------------------------------------------

/// Build a `FactionWorldView` from the full simulation state.
///
/// Visible regions are: regions with own forces, own controlled regions,
/// regions adjacent to owned forces, and regions within recon range of
/// units with `Recon` capability.
///
/// Enemy forces in visible regions are detected with strength estimates
/// scaled by the faction's intelligence stat.
pub fn build_world_view(
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    map: &GameMap,
) -> FactionWorldView {
    let faction_state = state
        .faction_states
        .get(faction_id)
        .expect("faction must exist when building world view");

    let intelligence = scenario
        .factions
        .get(faction_id)
        .map_or(0.5, |f| f.intelligence);

    // Compute visible regions.
    let mut visible: BTreeSet<RegionId> = BTreeSet::new();

    // Own controlled regions are always visible.
    for r in &faction_state.controlled_regions {
        visible.insert(r.clone());
    }

    // Regions with own forces + adjacent regions are visible.
    for force in faction_state.forces.values() {
        visible.insert(force.region.clone());
        for neighbor in adjacent_regions(&force.region, map) {
            visible.insert(neighbor);
        }

        // Extended visibility from Recon capability.
        for cap in &force.capabilities {
            if let UnitCapability::Recon { range, .. } = cap {
                // Treat range as number of hops of extended visibility.
                let mut frontier = vec![force.region.clone()];
                let mut seen = BTreeSet::new();
                seen.insert(force.region.clone());
                let hops = (*range as u32).min(3); // cap at 3 hops
                for _ in 0..hops {
                    let mut next_frontier = Vec::new();
                    for r in &frontier {
                        for neighbor in adjacent_regions(r, map) {
                            if seen.insert(neighbor.clone()) {
                                next_frontier.push(neighbor.clone());
                                visible.insert(neighbor);
                            }
                        }
                    }
                    frontier = next_frontier;
                }
            }
        }
    }

    // Build known_regions: only visible ones, with control info.
    let known_regions: BTreeMap<RegionId, Option<FactionId>> = visible
        .iter()
        .filter_map(|rid| {
            state
                .region_control
                .get(rid)
                .map(|ctrl| (rid.clone(), ctrl.clone()))
        })
        .collect();

    // Detect enemy forces in visible regions.
    let mut detected_forces = Vec::new();
    let base_confidence = (intelligence * 0.6 + 0.2).clamp(0.2, 0.9);

    for (fid, fs) in &state.faction_states {
        if fid == faction_id || fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            if visible.contains(&force.region) {
                // Scale estimated strength by confidence (deterministic).
                let confidence = base_confidence;
                let estimated_strength = force.strength * confidence;

                detected_forces.push(DetectedForce {
                    force_id: force.id.clone(),
                    faction: fid.clone(),
                    region: force.region.clone(),
                    estimated_strength,
                    confidence,
                });
            }
        }
    }

    FactionWorldView {
        faction: faction_id.clone(),
        known_regions,
        detected_forces,
        infra_states: BTreeMap::new(), // TODO: populate from visible infra
        political_climate: PoliticalClimateView {
            tension: state.political_climate.tension,
            institutional_trust: state.political_climate.institutional_trust,
            civilian_sentiment: 0.0, // TODO: derive from segment sympathies
        },
        diplomacy: BTreeMap::new(), // TODO: populate from faction diplomacy
        morale: faction_state.morale,
        resources: faction_state.resources,
        tick: state.tick,
    }
}

/// Evaluate actions using fog-of-war partial information.
///
/// Uses the faction's world view instead of full ground truth.
pub fn evaluate_actions_fog(
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    world_view: &FactionWorldView,
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

    // Build enemy presence from detected forces only, weighted by
    // diplomatic stance. A faction's declared diplomatic posture is
    // "self-knowledge" — the AI applies its own stance multiplier
    // even under fog of war.
    let mut enemy_presence = BTreeMap::new();
    for df in &world_view.detected_forces {
        let multiplier =
            crate::diplomacy::ai_threat_multiplier(state, scenario, faction_id, &df.faction);
        if multiplier == 0.0 {
            continue;
        }
        *enemy_presence.entry(df.region.clone()).or_insert(0.0) +=
            df.estimated_strength * multiplier;
    }

    let weights = determine_weights(faction_id, state, scenario);

    let mut actions = Vec::new();

    evaluate_defend_actions(
        faction_id,
        faction_state,
        &enemy_presence,
        &weights,
        &mut actions,
    );

    // Attack using known region control instead of ground truth.
    evaluate_attack_actions_fog(
        faction_id,
        faction_state,
        state,
        scenario,
        world_view,
        map,
        &weights,
        rng,
        &mut actions,
    );

    evaluate_move_actions_fog(
        faction_id,
        faction_state,
        world_view,
        map,
        &weights,
        &mut actions,
    );

    evaluate_recruit_actions(faction_state, &weights, &mut actions);

    actions.sort_by(|a, b| b.score.total_cmp(&a.score));
    actions
}

/// Attack evaluation using fog-of-war region control.
///
/// The diplomacy multiplier is read from ground truth (a faction
/// always knows its own declared stance). When `world_view.diplomacy`
/// is wired up in a future epic, this can shift to consulting the
/// world-view directly.
#[allow(clippy::too_many_arguments)]
fn evaluate_attack_actions_fog(
    faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    state: &SimulationState,
    scenario: &Scenario,
    world_view: &FactionWorldView,
    map: &GameMap,
    weights: &AiWeights,
    rng: &mut impl Rng,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let neighbors = adjacent_regions(&force.region, map);
        for neighbor in &neighbors {
            let controller = world_view
                .known_regions
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref());
            let enemy_controlled = controller.is_some_and(|ctrl| ctrl != faction_id);

            if !enemy_controlled {
                continue;
            }

            // Draw the noise unconditionally per enemy-controlled
            // neighbor *before* consulting diplomacy — see the
            // ground-truth `evaluate_attack_actions` for the
            // determinism rationale.
            let noise: f64 = rng.r#gen::<f64>() * 0.1;

            let priority_multiplier = controller
                .map(|ctrl| {
                    crate::diplomacy::ai_threat_multiplier(state, scenario, faction_id, ctrl)
                })
                .unwrap_or(1.0);
            if priority_multiplier == 0.0 {
                continue;
            }

            let strategic_value = map.regions.get(neighbor).map_or(1.0, |r| r.strategic_value);

            let score = (weights.objective_weight * strategic_value * 0.1
                + weights.opportunity_weight * 0.3
                + noise
                - weights.risk_aversion * 0.2)
                * priority_multiplier;

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

/// Move evaluation using fog-of-war region control.
fn evaluate_move_actions_fog(
    faction_id: &FactionId,
    faction_state: &RuntimeFactionState,
    world_view: &FactionWorldView,
    map: &GameMap,
    weights: &AiWeights,
    actions: &mut Vec<ScoredAction>,
) {
    for force in faction_state.forces.values() {
        let neighbors = adjacent_regions(&force.region, map);
        for neighbor in &neighbors {
            let is_ours = world_view
                .known_regions
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref())
                .is_some_and(|ctrl| ctrl == faction_id);

            if is_ours {
                continue;
            }

            // Unknown regions or unclaimed regions are move targets.
            let is_enemy = world_view
                .known_regions
                .get(neighbor)
                .and_then(|ctrl| ctrl.as_ref())
                .is_some_and(|ctrl| ctrl != faction_id);

            if is_enemy {
                continue; // Attacks handle enemy regions.
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
