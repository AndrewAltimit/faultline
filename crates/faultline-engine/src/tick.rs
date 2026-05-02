//! Per-tick phase implementations for the simulation loop.

use std::collections::BTreeMap;

use rand::Rng;

use faultline_events::{self, EventEvaluator, SimState};
use faultline_geo::{GameMap, adjacent_regions};
use faultline_politics::{self, TensionDelta};
use faultline_types::events::EventEffect;
use faultline_types::faction::ForceUnit;
use faultline_types::ids::{FactionId, ForceId, RegionId};
use faultline_types::map::{EnvironmentSchedule, EnvironmentWindow, TerrainType};
use faultline_types::scenario::Scenario;
use faultline_types::stats::Outcome;
use faultline_types::strategy::FactionAction;
use faultline_types::tech::TechEffect;
use faultline_types::victory::VictoryType;

use crate::ai;
use crate::combat::{self, CombatParams};
use crate::state::SimulationState;

/// Result of a single tick execution.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
            state.events_fired_this_tick.push(eid.clone());

            if !def.repeatable {
                state.events_fired.insert(eid.clone());
            }

            // Follow event chain (depth limit is defense-in-depth for
            // non-repeatable chains; cycles are prevented by DFS in
            // EventEvaluator::new).
            fire_event_chain(state, evaluator, rng, def, &mut fired, 10);
        }
    }

    fired
}

/// Follow an event's chain, firing chained events if their conditions are met.
fn fire_event_chain(
    state: &mut SimulationState,
    evaluator: &EventEvaluator,
    rng: &mut impl Rng,
    parent: &faultline_types::events::EventDefinition,
    fired: &mut Vec<String>,
    max_depth: u32,
) {
    let mut current_chain = parent.chain.clone();
    let mut depth = 0;

    while let Some(ref chain_id) = current_chain {
        if depth >= max_depth {
            tracing::warn!("event chain depth limit reached at {chain_id}");
            break;
        }

        if state.events_fired.contains(chain_id) {
            break;
        }

        let chained_def = match evaluator.events.get(chain_id) {
            Some(def) => def.clone(),
            None => break,
        };

        let sim_state = build_sim_state(state);
        if !faultline_events::evaluate_conditions(&chained_def, &sim_state) {
            break;
        }

        if let Some(effects) = faultline_events::fire_event(&chained_def, rng) {
            tracing::info!(event = %chain_id, "chained event fired");
            apply_event_effects(state, &effects);
            fired.push(chained_def.name.clone());
            state.events_fired_this_tick.push(chain_id.clone());

            if !chained_def.repeatable {
                state.events_fired.insert(chain_id.clone());
            }

            current_chain = chained_def.chain.clone();
        } else {
            break;
        }

        depth += 1;
    }
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
            EventEffect::NetworkEdgeCapacity {
                network,
                edge,
                factor,
            } => {
                if let Some(rt) = state.network_states.get_mut(network) {
                    let new_factor = if factor.is_finite() {
                        // Compose multiplicatively with whatever this
                        // edge already had: a chain of two `0.5`
                        // events drives the edge to 0.25, not 0.5.
                        // Clamp to `[0.0, 4.0]` so a runaway author
                        // chain can't poison the residual-capacity
                        // series with `inf`. Negative factors are
                        // also clamped to 0.0 (they would invert flow
                        // direction, which the metric layer doesn't
                        // model).
                        let prev = rt.edge_factor(edge);
                        (prev * factor).clamp(0.0, 4.0)
                    } else {
                        rt.edge_factor(edge)
                    };
                    rt.edge_factors.insert(edge.clone(), new_factor);
                }
            },
            EventEffect::NetworkNodeDisrupt { network, node } => {
                if let Some(rt) = state.network_states.get_mut(network) {
                    rt.disrupted_nodes.insert(node.clone());
                }
            },
            EventEffect::NetworkInfiltrate {
                network,
                node,
                faction,
            } => {
                if let Some(rt) = state.network_states.get_mut(network) {
                    rt.infiltrated
                        .entry(faction.clone())
                        .or_default()
                        .insert(node.clone());
                }
            },
            EventEffect::DiplomacyChange {
                faction_a,
                faction_b,
                new_stance,
            } => {
                // Wired by Epic D round two (coalition fracture).
                // We mutate the runtime override map rather than the
                // scenario-authored Faction.diplomacy table so the
                // baseline stays inspectable. The override is
                // direction-aware: `(faction_a, faction_b) -> stance`
                // sets faction_a's stance toward faction_b. The
                // event variant currently models a one-directional
                // change; symmetric flips are expressed by emitting
                // two events. Unknown faction ids are silently
                // ignored — same defensive shape used by every
                // event effect with a faction reference.
                if state.faction_states.contains_key(faction_a)
                    && state.faction_states.contains_key(faction_b)
                {
                    state
                        .diplomacy_overrides
                        .entry(faction_a.clone())
                        .or_default()
                        .insert(faction_b.clone(), *new_stance);
                }
            },
            // Effects that require more complex handling are logged
            // but not fully resolved in this skeleton.
            EventEffect::InstitutionDefection { .. }
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
    scenario: &Scenario,
    map: &GameMap,
    rng: &mut impl Rng,
) -> BTreeMap<FactionId, Vec<FactionAction>> {
    let faction_ids: Vec<FactionId> = state.faction_states.keys().cloned().collect();
    let fog_of_war = scenario.simulation.fog_of_war;

    let mut all_actions = BTreeMap::new();

    for fid in &faction_ids {
        let scored = if fog_of_war {
            let world_view = ai::build_world_view(fid, state, scenario, map);
            ai::evaluate_actions_fog(fid, state, scenario, &world_view, map, rng)
        } else {
            ai::evaluate_actions(fid, state, scenario, map, rng)
        };
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

        // Get terrain info for this region.
        let terrain_info = scenario.map.terrain.iter().find(|t| t.region == *region);
        let base_terrain_defense = terrain_info.map_or(1.0, |t| t.defense_modifier);
        let terrain_type = terrain_info.map_or(TerrainType::Rural, |t| t.terrain_type.clone());

        // Apply environmental defense modifier (Epic D — weather /
        // time-of-day). Multiplies the base terrain defense; resolves
        // to 1.0 when no windows are declared or none are active.
        let env_defense_factor = environment_defense_factor(scenario, &terrain_type, state.tick);
        let terrain_defense = base_terrain_defense * env_defense_factor;

        // Pairwise combat: all faction pairs engage each other,
        // except mutually-Allied pairs (Epic D round-three item 1 —
        // diplomacy behavioral coupling). Cooperative pairs still
        // fight if their forces collide; only `Diplomacy::Allied`
        // (in both directions) blocks combat.
        let factions: Vec<&FactionId> = faction_forces.keys().collect();

        for i in 0..factions.len() {
            for j in (i + 1)..factions.len() {
                let fid_a = factions[i];
                let fid_b = factions[j];

                if crate::diplomacy::combat_blocked(state, scenario, fid_a, fid_b) {
                    continue;
                }

                let str_a = faction_forces.get(fid_a).copied().unwrap_or(0.0);
                let str_b = faction_forces.get(fid_b).copied().unwrap_or(0.0);

                if str_a <= 0.0 || str_b <= 0.0 {
                    continue;
                }

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

                let tech_modifier_a =
                    compute_tech_combat_modifier(fid_a, fid_b, state, scenario, &terrain_type);
                let tech_modifier_b =
                    compute_tech_combat_modifier(fid_b, fid_a, state, scenario, &terrain_type);

                let params = CombatParams {
                    strength_a: str_a,
                    strength_b: str_b,
                    morale_a,
                    morale_b,
                    terrain_defense,
                    tech_modifier_a,
                    tech_modifier_b,
                    guerrilla_a,
                    guerrilla_b,
                    attrition_coeff: 0.01,
                };

                let result =
                    combat::resolve_combat(&params, &scenario.simulation.attrition_model, rng);

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
    }

    combats
}

/// Compute a faction's cumulative tech combat modifier for a given terrain.
///
/// Iterates the faction's deployed tech cards, resolves terrain effects,
/// extracts `CombatModifier` effects, and multiplies their effectiveness.
/// Cards countered by the opponent's active techs are skipped.
fn compute_tech_combat_modifier(
    faction_id: &FactionId,
    opponent_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    terrain: &TerrainType,
) -> f64 {
    let tech_deployed = match state.faction_states.get(faction_id) {
        Some(fs) => &fs.tech_deployed,
        None => return 1.0,
    };

    let empty_techs = Vec::new();
    let opponent_techs = state
        .faction_states
        .get(opponent_id)
        .map_or(&empty_techs, |fs| &fs.tech_deployed);

    let mut modifier = 1.0;

    for tech_id in tech_deployed {
        let card = match scenario.technology.get(tech_id) {
            Some(c) => c,
            None => continue,
        };

        if faultline_tech::is_countered(card, opponent_techs) {
            continue;
        }

        let resolved = faultline_tech::apply_tech_effects(card, terrain);
        for re in &resolved {
            if let TechEffect::CombatModifier { factor } = &re.effect {
                modifier *= factor * re.effectiveness;
            }
        }
    }

    modifier.clamp(0.25, 3.0)
}

/// Find regions where multiple factions have forces.
fn find_contested_regions(state: &SimulationState) -> BTreeMap<RegionId, BTreeMap<FactionId, f64>> {
    let mut region_forces: BTreeMap<RegionId, BTreeMap<FactionId, f64>> = BTreeMap::new();

    for (fid, fs) in &state.faction_states {
        if fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            // `morale_modifier` is a per-unit cohesion / training
            // multiplier folded into the unit's effective combat
            // contribution. Default 0.0 yields a 1.0× multiplier
            // (legacy behavior); a value of 0.15 — the high end seen in
            // bundled scenarios — gives an elite unit a 15% strength
            // bonus when it engages. Negative values are permitted
            // (a green or demoralized unit punches below its weight)
            // but the multiplier is floored at 0 so a pathological
            // override below -1.0 cannot produce a negative effective
            // strength that would invert the combat math.
            let multiplier = (1.0 + force.morale_modifier).max(0.0);
            *region_forces
                .entry(force.region.clone())
                .or_default()
                .entry(fid.clone())
                .or_insert(0.0) += force.strength * multiplier;
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

    // Update civilian segments and process activations.
    let activations =
        faultline_politics::update_civilian_segments(&mut state.political_climate, state.tick, rng);

    for activation in &activations {
        tracing::info!(
            segment = %activation.segment_id,
            faction = %activation.favored_faction,
            "civilian segment activated"
        );
        process_civilian_activation(state, scenario, activation, rng);
    }
}

/// Process the effects of a civilian segment activation.
fn process_civilian_activation(
    state: &mut SimulationState,
    scenario: &Scenario,
    activation: &faultline_politics::ActivationResult,
    rng: &mut impl Rng,
) {
    use faultline_types::faction::UnitType;
    use faultline_types::politics::CivilianAction;

    for action in &activation.actions {
        match action {
            CivilianAction::ArmedResistance {
                target_faction,
                unit_strength,
            } => {
                // Spawn militia for the favored faction in concentrated regions.
                if let Some(fs) = state.faction_states.get_mut(target_faction) {
                    for region in &activation.concentrated_in {
                        let force_id = ForceId::from(format!(
                            "militia-{}-{}-{}",
                            activation.segment_id, region, state.tick
                        ));
                        let unit = ForceUnit {
                            id: force_id.clone(),
                            name: format!("{} Militia", activation.segment_id),
                            unit_type: UnitType::Militia,
                            region: region.clone(),
                            strength: *unit_strength,
                            mobility: 0.5,
                            force_projection: None,
                            upkeep: unit_strength * 0.05,
                            morale_modifier: 0.0,
                            capabilities: Vec::new(),
                        };
                        fs.forces.insert(force_id, unit);
                    }
                    fs.recompute_strength();
                }
            },
            CivilianAction::Sabotage {
                target_infra_type,
                probability,
            } => {
                let roll: f64 = rng.r#gen();
                if roll < *probability {
                    // Damage infrastructure of the matching type in concentrated regions.
                    for (infra_id, status) in &mut state.infra_status {
                        let matches = match target_infra_type {
                            Some(target_type) => scenario
                                .map
                                .infrastructure
                                .get(infra_id)
                                .is_some_and(|node| {
                                    node.infra_type == *target_type
                                        && activation.concentrated_in.contains(&node.region)
                                }),
                            None => scenario
                                .map
                                .infrastructure
                                .get(infra_id)
                                .is_some_and(|node| {
                                    activation.concentrated_in.contains(&node.region)
                                }),
                        };
                        if matches {
                            *status = (*status - 0.3).max(0.0);
                            tracing::info!(
                                infra = %infra_id,
                                "infrastructure sabotaged by civilian segment"
                            );
                        }
                    }
                }
            },
            CivilianAction::MaterialSupport {
                target_faction,
                rate,
            } => {
                if let Some(fs) = state.faction_states.get_mut(target_faction) {
                    fs.resources += rate;
                }
            },
            CivilianAction::Protest { intensity } => {
                state.political_climate.tension =
                    (state.political_climate.tension + intensity * 0.05).min(1.0);
            },
            CivilianAction::Flee { rate } => {
                // Reduce segment fraction (already activated, so find it).
                for seg in &mut state.political_climate.population_segments {
                    if seg.id == activation.segment_id {
                        seg.fraction = (seg.fraction - rate).max(0.0);
                    }
                }
            },
            CivilianAction::Intelligence {
                target_faction,
                quality,
            } => {
                // Civilian intelligence degrades the target faction's
                // morale (pressure from surveillance/infiltration) and
                // provides a small resource bonus to the favored faction
                // (actionable intel is an asset).
                if let Some(fs) = state.faction_states.get_mut(target_faction) {
                    fs.morale = (fs.morale - quality * 0.05).max(0.0);
                }
                if let Some(fs) = state.faction_states.get_mut(&activation.favored_faction) {
                    fs.resources += quality * 5.0;
                }
                tracing::info!(
                    target = %target_faction,
                    quality = quality,
                    "civilian intelligence gathered"
                );
            },
            CivilianAction::NonCooperation {
                effectiveness_reduction,
            } => {
                // Non-cooperation reduces resource income for factions
                // controlling the segment's concentrated regions (strikes,
                // refusal to work, bureaucratic obstruction).
                for region in &activation.concentrated_in {
                    let controller = state.region_control.get(region).and_then(|c| c.clone());
                    if let Some(ctrl_fid) = controller
                        && ctrl_fid != activation.favored_faction
                        && let Some(fs) = state.faction_states.get_mut(&ctrl_fid)
                    {
                        let loss = fs.resources * effectiveness_reduction;
                        fs.resources = (fs.resources - loss).max(0.0);
                    }
                }
                tracing::info!(
                    reduction = effectiveness_reduction,
                    "civilian non-cooperation applied"
                );
            },
        }
    }
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
        VictoryType::NonKineticThreshold { metric, threshold } => {
            use faultline_types::victory::NonKineticMetric;
            let value = match metric {
                NonKineticMetric::InformationDominance => state.non_kinetic.information_dominance,
                NonKineticMetric::InstitutionalErosion => state.non_kinetic.institutional_erosion,
                NonKineticMetric::CoercionPressure => state.non_kinetic.coercion_pressure,
                NonKineticMetric::PoliticalCost => state.non_kinetic.political_cost,
            };
            value >= *threshold
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
                .max_by(|a, b| a.1.total_cmp(b.1))
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

// -----------------------------------------------------------------------
// Environment helpers (Epic D — weather, time-of-day)
// -----------------------------------------------------------------------

/// Whether `window` applies to a region of the given terrain.
fn window_covers(window: &EnvironmentWindow, terrain: &TerrainType) -> bool {
    window.applies_to.is_empty() || window.applies_to.contains(terrain)
}

/// Multiplicative product of every active window's `defense_factor`
/// that covers `terrain` at `tick`. Empty schedule resolves to 1.0,
/// so legacy scenarios are unchanged.
pub fn environment_defense_factor(scenario: &Scenario, terrain: &TerrainType, tick: u32) -> f64 {
    multiplicative_factor(&scenario.environment, tick, |w| {
        if window_covers(w, terrain) {
            Some(w.defense_factor)
        } else {
            None
        }
    })
}

/// Multiplicative product of every active window's `detection_factor`,
/// applied globally to kill-chain phase rolls. `applies_to` does not
/// gate this — see [`EnvironmentWindow::detection_factor`] for why.
pub fn environment_detection_factor(scenario: &Scenario, tick: u32) -> f64 {
    multiplicative_factor(&scenario.environment, tick, |w| Some(w.detection_factor))
}

fn multiplicative_factor(
    schedule: &EnvironmentSchedule,
    tick: u32,
    extract: impl Fn(&EnvironmentWindow) -> Option<f64>,
) -> f64 {
    let mut acc = 1.0_f64;
    for window in &schedule.windows {
        if !window.activation.is_active_at(tick) {
            continue;
        }
        if let Some(factor) = extract(window) {
            acc *= factor;
        }
    }
    acc
}

// -----------------------------------------------------------------------
// Leadership caps (Epic D — decapitation + succession)
// -----------------------------------------------------------------------

/// Compute the effective leadership multiplier for `faction_id` at
/// `tick`. Returns `1.0` (no effect) for any faction without a
/// declared cadre — legacy scenarios pay zero per-tick overhead.
///
/// Formula:
/// - `current_rank.effectiveness * recovery_ramp(elapsed)`
/// - where `recovery_ramp` linearly interpolates from
///   `succession_floor` to `1.0` over `succession_recovery_ticks`,
///   measured from the most recent decapitation.
/// - Returns `0.0` when the rank index has advanced past the end of
///   the cadre — the faction is leaderless, morale floors at zero.
pub fn effective_leadership_factor(
    state: &SimulationState,
    scenario: &Scenario,
    faction_id: &FactionId,
    tick: u32,
) -> f64 {
    let Some(faction) = scenario.factions.get(faction_id) else {
        return 1.0;
    };
    let Some(cadre) = faction.leadership.as_ref() else {
        return 1.0;
    };
    let Some(fs) = state.faction_states.get(faction_id) else {
        return 1.0;
    };

    // Past the end of the cadre: leaderless terminal state.
    let idx = fs.current_leadership_rank as usize;
    if idx >= cadre.ranks.len() {
        return 0.0;
    }
    let rank = &cadre.ranks[idx];

    // No decapitation yet? Full effectiveness immediately.
    let Some(strike_tick) = fs.last_decapitation_tick else {
        return rank.effectiveness;
    };
    if cadre.succession_recovery_ticks == 0 {
        return rank.effectiveness;
    }

    let elapsed = tick.saturating_sub(strike_tick);
    if elapsed >= cadre.succession_recovery_ticks {
        return rank.effectiveness;
    }
    let progress = f64::from(elapsed) / f64::from(cadre.succession_recovery_ticks);
    let ramp = cadre.succession_floor + (1.0 - cadre.succession_floor) * progress;
    rank.effectiveness * ramp
}

/// Cap each faction's morale at its current `effective_leadership_factor`.
///
/// Iterates over every faction with a declared cadre and clamps
/// `morale` from above. Faction morale stays at or below the
/// leadership ceiling for the whole recovery window, which is what
/// makes the decapitation observable in combat outcomes
/// (combat reads `morale` directly).
///
/// No-op when no faction declares a `leadership` cadre — the
/// per-faction loop body short-circuits via the `1.0` return.
pub fn apply_leadership_caps(state: &mut SimulationState, scenario: &Scenario) {
    // Legacy scenarios with no cadres pay only this scan instead of
    // cloning every FactionId and computing a no-op factor per tick.
    if !scenario.factions.values().any(|f| f.leadership.is_some()) {
        return;
    }
    // Snapshot tick before borrowing the faction map mutably.
    let tick = state.tick;
    // Collect the cap values first so we don't hold an immutable
    // borrow while writing.
    let caps: Vec<(FactionId, f64)> = state
        .faction_states
        .keys()
        .map(|fid| {
            let cap = effective_leadership_factor(state, scenario, fid, tick);
            (fid.clone(), cap)
        })
        .collect();
    for (fid, cap) in caps {
        if cap >= 1.0 - f64::EPSILON {
            // No effect — common path for legacy factions; skip the
            // write to keep the morale field untouched.
            continue;
        }
        if let Some(fs) = state.faction_states.get_mut(&fid)
            && fs.morale > cap
        {
            fs.morale = cap.clamp(0.0, 1.0);
        }
    }
}
