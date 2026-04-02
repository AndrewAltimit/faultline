//! Per-tick phase implementations for the simulation loop.

use std::collections::BTreeMap;

use rand::Rng;

use faultline_events::{self, EventEvaluator, SimState};
use faultline_geo::{GameMap, adjacent_regions};
use faultline_politics::{self, TensionDelta};
use faultline_types::events::EventEffect;
use faultline_types::faction::ForceUnit;
use faultline_types::ids::{FactionId, ForceId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::Outcome;
use faultline_types::strategy::FactionAction;
use faultline_types::victory::VictoryType;

use crate::ai;
use crate::combat::{self, CombatParams};
use crate::state::SimulationState;

/// Result of a single tick execution.
#[derive(Clone, Debug)]
pub struct TickResult {
    pub tick: u32,
    pub events_fired: Vec<String>,
    pub combats_resolved: u32,
    pub outcome: Option<Outcome>,
}

// -----------------------------------------------------------------------
// Phase 1: Events
// -----------------------------------------------------------------------

/// Evaluate event conditions and fire eligible events, applying their
/// effects to the simulation state.
pub fn event_phase(
    state: &mut SimulationState,
    evaluator: &EventEvaluator,
    rng: &mut impl Rng,
) -> Vec<String> {
    let sim_state = build_sim_state(state);
    let mut fired = Vec::new();

    // Collect events to evaluate (avoid borrow conflict).
    let candidates: Vec<_> = evaluator
        .events
        .iter()
        .filter(|(eid, _)| !state.events_fired.contains(*eid))
        .map(|(eid, def)| (eid.clone(), def.clone()))
        .collect();

    for (eid, def) in &candidates {
        if !faultline_events::evaluate_conditions(def, &sim_state) {
            continue;
        }

        if let Some(effects) = faultline_events::fire_event(def, rng) {
            tracing::info!(event = %eid, "event fired");
            apply_event_effects(state, &effects);
            fired.push(def.name.clone());

            if !def.repeatable {
                state.events_fired.insert(eid.clone());
            }
        }
    }

    fired
}

/// Build a `SimState` snapshot for the event evaluator.
fn build_sim_state(state: &SimulationState) -> SimState {
    let mut faction_strengths = BTreeMap::new();
    let mut faction_morale = BTreeMap::new();

    for (fid, fs) in &state.faction_states {
        faction_strengths.insert(fid.clone(), fs.total_strength);
        faction_morale.insert(fid.clone(), fs.morale);
    }

    let fired_events = state
        .events_fired
        .iter()
        .map(|eid| (eid.clone(), true))
        .collect();

    SimState {
        tick: state.tick,
        tension: state.political_climate.tension,
        faction_strengths,
        faction_morale,
        region_control: state.region_control.clone(),
        fired_events,
    }
}

/// Apply a list of event effects to the simulation state.
fn apply_event_effects(state: &mut SimulationState, effects: &[EventEffect]) {
    for effect in effects {
        match effect {
            EventEffect::TensionShift { delta } => {
                state.political_climate.tension =
                    (state.political_climate.tension + delta).clamp(0.0, 1.0);
            },
            EventEffect::MoraleShift { faction, delta } => {
                if let Some(fs) = state.faction_states.get_mut(faction) {
                    fs.morale = (fs.morale + delta).clamp(0.0, 1.0);
                }
            },
            EventEffect::LoyaltyShift { institution, delta } => {
                if let Some(loyalty) = state.institution_loyalty.get_mut(institution) {
                    *loyalty = (*loyalty + delta).clamp(0.0, 1.0);
                }
            },
            EventEffect::DamageInfra { infra, damage } => {
                if let Some(status) = state.infra_status.get_mut(infra) {
                    *status = (*status - damage).max(0.0);
                }
            },
            EventEffect::ResourceChange { faction, delta } => {
                if let Some(fs) = state.faction_states.get_mut(faction) {
                    fs.resources = (fs.resources + delta).max(0.0);
                }
            },
            EventEffect::SpawnUnits { faction, units } => {
                if let Some(fs) = state.faction_states.get_mut(faction) {
                    for unit in units {
                        fs.forces.insert(unit.id.clone(), unit.clone());
                    }
                    fs.recompute_strength();
                }
            },
            EventEffect::DestroyUnits {
                faction,
                region,
                damage,
            } => {
                if let Some(fs) = state.faction_states.get_mut(faction) {
                    for force in fs.forces.values_mut() {
                        if force.region == *region {
                            force.strength = (force.strength - damage).max(0.0);
                        }
                    }
                    fs.recompute_strength();
                }
            },
            EventEffect::SympathyShift {
                segment,
                faction,
                delta,
            } => {
                for seg in &mut state.political_climate.population_segments {
                    if seg.id == *segment {
                        for sym in &mut seg.sympathies {
                            if sym.faction == *faction {
                                sym.sympathy = (sym.sympathy + delta).clamp(-1.0, 1.0);
                            }
                        }
                    }
                }
            },
            // Effects that require more complex handling are logged
            // but not fully resolved in this skeleton.
            EventEffect::InstitutionDefection { .. }
            | EventEffect::DiplomacyChange { .. }
            | EventEffect::TechAccess { .. }
            | EventEffect::MediaEvent { .. }
            | EventEffect::Narrative { .. } => {
                tracing::debug!(?effect, "unhandled event effect");
            },
        }
    }
}

// -----------------------------------------------------------------------
// Phase 2: Decision (AI)
// -----------------------------------------------------------------------

/// Each faction evaluates its situation and queues actions.
pub fn decision_phase(
    state: &mut SimulationState,
    _scenario: &Scenario,
    map: &GameMap,
    rng: &mut impl Rng,
) -> BTreeMap<FactionId, Vec<FactionAction>> {
    let faction_ids: Vec<FactionId> = state.faction_states.keys().cloned().collect();

    let mut all_actions = BTreeMap::new();

    for fid in &faction_ids {
        let scored = ai::evaluate_actions(fid, state, map, rng);
        // Take top 3 actions per faction per tick.
        let top_actions: Vec<FactionAction> =
            scored.into_iter().take(3).map(|sa| sa.action).collect();
        all_actions.insert(fid.clone(), top_actions);
    }

    all_actions
}

// -----------------------------------------------------------------------
// Phase 3: Movement
// -----------------------------------------------------------------------

/// Resolve queued movement actions. Units move to adjacent regions
/// if the move is valid.
pub fn movement_phase(
    state: &mut SimulationState,
    map: &GameMap,
    queued_actions: &BTreeMap<FactionId, Vec<FactionAction>>,
) {
    for (faction_id, actions) in queued_actions {
        for action in actions {
            if let FactionAction::MoveUnit { force, destination } = action {
                move_unit(state, faction_id, force, destination, map);
            }
        }
    }
}

fn move_unit(
    state: &mut SimulationState,
    faction_id: &FactionId,
    force_id: &ForceId,
    destination: &RegionId,
    map: &GameMap,
) {
    let fs = match state.faction_states.get_mut(faction_id) {
        Some(fs) => fs,
        None => return,
    };

    let force = match fs.forces.get(force_id) {
        Some(f) => f,
        None => return,
    };

    // Validate adjacency.
    let neighbors = adjacent_regions(&force.region, map);
    if !neighbors.contains(destination) {
        return;
    }

    // Move the unit.
    if let Some(force) = fs.forces.get_mut(force_id) {
        force.region = destination.clone();
    }
}

// -----------------------------------------------------------------------
// Phase 4: Combat
// -----------------------------------------------------------------------

/// Resolve combat in regions where opposing factions have forces.
pub fn combat_phase(state: &mut SimulationState, scenario: &Scenario, rng: &mut impl Rng) -> u32 {
    // Find regions with forces from multiple factions.
    let contested = find_contested_regions(state);
    let mut combats = 0;

    for (region, faction_forces) in &contested {
        if faction_forces.len() < 2 {
            continue;
        }

        // Get terrain defense modifier for this region.
        let terrain_defense = scenario
            .map
            .terrain
            .iter()
            .find(|t| t.region == *region)
            .map_or(1.0, |t| t.defense_modifier);

        // Simple pairwise combat: first faction vs. second faction.
        let factions: Vec<&FactionId> = faction_forces.keys().collect();

        // Process the first pair of opposing factions.
        if factions.len() >= 2 {
            let fid_a = factions[0];
            let fid_b = factions[1];

            let str_a = faction_forces.get(fid_a).copied().unwrap_or(0.0);
            let str_b = faction_forces.get(fid_b).copied().unwrap_or(0.0);

            let morale_a = state.faction_states.get(fid_a).map_or(0.5, |fs| fs.morale);
            let morale_b = state.faction_states.get(fid_b).map_or(0.5, |fs| fs.morale);

            let guerrilla_a = state
                .faction_states
                .get(fid_a)
                .is_some_and(|fs| fs.has_guerrilla_units());
            let guerrilla_b = state
                .faction_states
                .get(fid_b)
                .is_some_and(|fs| fs.has_guerrilla_units());

            let params = CombatParams {
                strength_a: str_a,
                strength_b: str_b,
                morale_a,
                morale_b,
                terrain_defense,
                tech_modifier_a: 1.0,
                tech_modifier_b: 1.0,
                guerrilla_a,
                guerrilla_b,
                attrition_coeff: 0.01,
            };

            let result = combat::resolve_combat(&params, &scenario.simulation.attrition_model, rng);

            // Apply attrition to forces in this region.
            apply_attrition_to_region(state, region, fid_a, result.attrition_a);
            apply_attrition_to_region(state, region, fid_b, result.attrition_b);

            // Morale impact from combat.
            if let Some(fs) = state.faction_states.get_mut(fid_a) {
                if result.rout_a || result.surrender_a {
                    fs.morale = (fs.morale - 0.15).max(0.0);
                } else {
                    fs.morale = (fs.morale - 0.02).max(0.0);
                }
            }
            if let Some(fs) = state.faction_states.get_mut(fid_b) {
                if result.rout_b || result.surrender_b {
                    fs.morale = (fs.morale - 0.15).max(0.0);
                } else {
                    fs.morale = (fs.morale - 0.02).max(0.0);
                }
            }

            combats += 1;
        }
    }

    combats
}

/// Find regions where multiple factions have forces.
fn find_contested_regions(state: &SimulationState) -> BTreeMap<RegionId, BTreeMap<FactionId, f64>> {
    let mut region_forces: BTreeMap<RegionId, BTreeMap<FactionId, f64>> = BTreeMap::new();

    for (fid, fs) in &state.faction_states {
        if fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            *region_forces
                .entry(force.region.clone())
                .or_default()
                .entry(fid.clone())
                .or_insert(0.0) += force.strength;
        }
    }

    // Only keep contested regions (2+ factions).
    region_forces.retain(|_, factions| factions.len() >= 2);
    region_forces
}

/// Distribute attrition across a faction's forces in a region.
fn apply_attrition_to_region(
    state: &mut SimulationState,
    region: &RegionId,
    faction_id: &FactionId,
    total_attrition: f64,
) {
    let fs = match state.faction_states.get_mut(faction_id) {
        Some(fs) => fs,
        None => return,
    };

    // Distribute proportionally across forces in the region.
    let forces_in_region: Vec<ForceId> = fs
        .forces
        .values()
        .filter(|f| f.region == *region)
        .map(|f| f.id.clone())
        .collect();

    let total_str: f64 = forces_in_region
        .iter()
        .filter_map(|fid| fs.forces.get(fid))
        .map(|f| f.strength)
        .sum();

    if total_str <= 0.0 {
        return;
    }

    for fid in &forces_in_region {
        if let Some(force) = fs.forces.get_mut(fid) {
            let share = force.strength / total_str;
            force.strength = (force.strength - total_attrition * share).max(0.0);
        }
    }

    // Remove destroyed forces.
    fs.forces.retain(|_, f| f.strength > 0.1);
    fs.recompute_strength();
}

// -----------------------------------------------------------------------
// Phase 5: Attrition (logistics, resources, recruitment)
// -----------------------------------------------------------------------

/// Resource consumption, recruitment, and infrastructure repair.
pub fn attrition_phase(state: &mut SimulationState, scenario: &Scenario) {
    let faction_ids: Vec<FactionId> = state.faction_states.keys().cloned().collect();

    for fid in &faction_ids {
        let (resource_rate, recruitment_cfg, upkeep) = {
            let faction_def = match scenario.factions.get(fid) {
                Some(f) => f,
                None => continue,
            };
            let fs = match state.faction_states.get(fid) {
                Some(fs) => fs,
                None => continue,
            };
            let upkeep: f64 = fs.forces.values().map(|f| f.upkeep).sum();
            (
                faction_def.resource_rate,
                faction_def.recruitment.clone(),
                upkeep,
            )
        };

        if let Some(fs) = state.faction_states.get_mut(fid) {
            // Income.
            fs.resources += resource_rate;

            // Upkeep.
            fs.resources = (fs.resources - upkeep).max(0.0);

            // Recruitment: spawn new units if affordable.
            if let Some(ref recruit) = recruitment_cfg
                && fs.resources >= recruit.cost
                && let Some(region) = fs.controlled_regions.first().cloned()
            {
                let new_id = ForceId::from(format!("{}-recruit-{}", fid, state.tick));
                let unit = ForceUnit {
                    id: new_id.clone(),
                    name: format!("Recruits T{}", state.tick),
                    unit_type: recruit.unit_type.clone(),
                    region,
                    strength: recruit.base_strength * recruit.rate,
                    mobility: 1.0,
                    force_projection: None,
                    upkeep: recruit.cost * 0.1,
                    morale_modifier: 0.0,
                    capabilities: Vec::new(),
                };
                fs.forces.insert(new_id, unit);
                fs.resources -= recruit.cost;
                fs.recompute_strength();
            }

            // Check elimination.
            if fs.total_strength <= 0.0 && recruitment_cfg.is_none() {
                fs.eliminated = true;
            }
        }
    }

    // Infrastructure natural repair (slow).
    for (infra_id, status) in &mut state.infra_status {
        if *status < 1.0 {
            let repairable = scenario
                .map
                .infrastructure
                .get(infra_id)
                .and_then(|node| node.repairable)
                .unwrap_or(0);
            if repairable > 0 {
                let repair_rate = 1.0 / f64::from(repairable);
                *status = (*status + repair_rate).min(1.0);
            }
        }
    }
}

// -----------------------------------------------------------------------
// Phase 6: Political
// -----------------------------------------------------------------------

/// Update tension, institutional loyalty, and civilian segments.
pub fn political_phase(state: &mut SimulationState, scenario: &Scenario, rng: &mut impl Rng) {
    // Tension naturally drifts based on combat activity.
    let active_factions = state
        .faction_states
        .values()
        .filter(|fs| !fs.eliminated)
        .count();

    let tension_delta = if active_factions > 2 {
        0.01 // Multi-faction conflict increases tension
    } else {
        -0.005 // Cooling off
    };

    faultline_politics::update_tension(
        &mut state.political_climate,
        &[TensionDelta {
            faction: None,
            delta: tension_delta,
        }],
    );

    // Update institution loyalty.
    for (inst_id, loyalty) in &mut state.institution_loyalty {
        // Erosion from high tension.
        let tension = state.political_climate.tension;
        let erosion = tension * 0.005;
        *loyalty = (*loyalty - erosion).clamp(0.0, 1.0);

        // Check for fracture in scenario institutions.
        for faction in scenario.factions.values() {
            if let faultline_types::faction::FactionType::Government { institutions } =
                &faction.faction_type
                && let Some(inst) = institutions.get(inst_id)
                && *loyalty < inst.fracture_threshold.unwrap_or(0.0)
            {
                tracing::warn!(
                    institution = %inst_id,
                    "institution fractured"
                );
            }
        }
    }

    // Update civilian segments.
    faultline_politics::update_civilian_segments(&mut state.political_climate, state.tick, rng);
}

// -----------------------------------------------------------------------
// Phase 7: Information warfare
// -----------------------------------------------------------------------

/// Process information warfare effects (simplified).
pub fn information_phase(state: &mut SimulationState, _scenario: &Scenario) {
    // Information warfare affects civilian sympathy and tension.
    let media = &state.political_climate.media_landscape;
    let disinfo_factor = media.disinformation_susceptibility;

    // High disinformation susceptibility increases tension.
    if disinfo_factor > 0.5 {
        let delta = (disinfo_factor - 0.5) * 0.01;
        state.political_climate.tension = (state.political_climate.tension + delta).min(1.0);
    }

    // High state media control dampens tension.
    if media.state_control > 0.7 {
        let dampening = (media.state_control - 0.7) * 0.005;
        state.political_climate.tension = (state.political_climate.tension - dampening).max(0.0);
    }
}

// -----------------------------------------------------------------------
// Phase 8: Victory check
// -----------------------------------------------------------------------

/// Check all victory conditions and return an outcome if one is met.
pub fn victory_check(state: &SimulationState, scenario: &Scenario) -> Option<Outcome> {
    for vc in scenario.victory_conditions.values() {
        let met = check_condition(state, scenario, vc);
        if met {
            return Some(Outcome {
                victor: Some(vc.faction.clone()),
                victory_condition: Some(vc.name.clone()),
                final_tension: state.political_climate.tension,
            });
        }
    }

    // Check if only one faction remains.
    let alive: Vec<&FactionId> = state
        .faction_states
        .iter()
        .filter(|(_, fs)| !fs.eliminated)
        .map(|(fid, _)| fid)
        .collect();

    if alive.len() == 1 {
        return Some(Outcome {
            victor: Some(alive[0].clone()),
            victory_condition: Some("Last faction standing".to_owned()),
            final_tension: state.political_climate.tension,
        });
    }

    None
}

fn check_condition(
    state: &SimulationState,
    _scenario: &Scenario,
    vc: &faultline_types::victory::VictoryCondition,
) -> bool {
    match &vc.condition {
        VictoryType::StrategicControl { threshold } => {
            let total_regions = state.region_control.len();
            if total_regions == 0 {
                return false;
            }
            let controlled = state
                .region_control
                .values()
                .filter(|ctrl| ctrl.as_ref().is_some_and(|f| *f == vc.faction))
                .count();
            let ratio = controlled as f64 / total_regions as f64;
            ratio >= *threshold
        },
        VictoryType::MilitaryDominance {
            enemy_strength_below,
        } => {
            // All enemy factions must be below the threshold.
            state.faction_states.iter().all(|(fid, fs)| {
                *fid == vc.faction || fs.eliminated || fs.total_strength < *enemy_strength_below
            })
        },
        VictoryType::HoldRegions { regions, duration } => {
            let fs = match state.faction_states.get(&vc.faction) {
                Some(fs) => fs,
                None => return false,
            };
            regions
                .iter()
                .all(|rid| fs.region_hold_ticks.get(rid).copied().unwrap_or(0) >= *duration)
        },
        VictoryType::InstitutionalCollapse { trust_below } => {
            state.political_climate.institutional_trust < *trust_below
        },
        VictoryType::PeaceSettlement => {
            // Peace when tension is very low and no active combat.
            state.political_climate.tension < 0.1
        },
        VictoryType::Custom {
            variable: _,
            threshold: _,
            above: _,
        } => {
            // Custom conditions not evaluated in this version.
            false
        },
    }
}

/// Update region control based on which faction has the most
/// strength in each region. Also update hold ticks.
pub fn update_region_control(state: &mut SimulationState, _scenario: &Scenario) {
    // Compute per-region strength.
    let mut region_strength: BTreeMap<RegionId, BTreeMap<FactionId, f64>> = BTreeMap::new();

    for (fid, fs) in &state.faction_states {
        if fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            *region_strength
                .entry(force.region.clone())
                .or_default()
                .entry(fid.clone())
                .or_insert(0.0) += force.strength;
        }
    }

    // Assign control to strongest faction in each region.
    for (rid, control) in &mut state.region_control {
        let strongest = region_strength.get(rid).and_then(|factions| {
            factions
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(fid, _)| fid.clone())
        });
        *control = strongest;
    }

    // Update controlled_regions and hold ticks for each faction.
    let region_control_snapshot = state.region_control.clone();
    for (fid, fs) in &mut state.faction_states {
        fs.controlled_regions = region_control_snapshot
            .iter()
            .filter(|(_, ctrl)| ctrl.as_ref().is_some_and(|f| f == fid))
            .map(|(rid, _)| rid.clone())
            .collect();

        // Update hold ticks.
        let controlled_set: std::collections::BTreeSet<_> =
            fs.controlled_regions.iter().cloned().collect();
        let hold_keys: Vec<RegionId> = fs.region_hold_ticks.keys().cloned().collect();
        for rid in &hold_keys {
            if !controlled_set.contains(rid) {
                fs.region_hold_ticks.remove(rid);
            }
        }
        for rid in &fs.controlled_regions {
            *fs.region_hold_ticks.entry(rid.clone()).or_insert(0) += 1;
        }
    }
}
