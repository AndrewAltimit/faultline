//! Per-tick phase implementations for the simulation loop.

use std::collections::BTreeMap;

use rand::Rng;

use faultline_events::{self, EventEvaluator, SimState};
use faultline_geo::{GameMap, adjacent_regions};
use faultline_politics::{self, TensionDelta};
use faultline_types::events::EventEffect;
use faultline_types::faction::ForceUnit;
use faultline_types::ids::{FactionId, ForceId, RegionId, TechCardId};
use faultline_types::map::{EnvironmentSchedule, EnvironmentWindow, TerrainType};
use faultline_types::scenario::Scenario;
use faultline_types::stats::Outcome;
use faultline_types::strategy::FactionAction;
use faultline_types::tech::TechEffect;
use faultline_types::victory::VictoryType;

use crate::ai;
use crate::combat::{self, CombatParams};
use crate::state::{RuntimeFactionState, SimulationState};

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
                // Wired by the coalition-fracture phase. We mutate the
                // runtime override map rather than the
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
            EventEffect::MediaEvent {
                narrative,
                credibility,
                reach,
                favors,
            } => {
                // Epic D round-three item 4 — info-op narrative
                // competition. Register or reinforce a persistent
                // narrative entry; the narrative phase later this tick
                // decays the store, scores dominance, and applies
                // sympathy / tension nudges. Empty narrative key is a
                // silent no-op at runtime because validation rejects it
                // at scenario load. Unknown `favors` faction is also a
                // load-time rejection. The `was_new` flag drives the
                // per-event log so the cross-run aggregator can count
                // distinct firings vs. reinforcements without re-deriving
                // it from the narrative timeline.
                if narrative.is_empty() || !credibility.is_finite() || !reach.is_finite() {
                    continue;
                }
                let credibility = credibility.clamp(0.0, 1.0);
                let reach = reach.clamp(0.0, 1.0);
                let fragmentation = state
                    .political_climate
                    .media_landscape
                    .fragmentation
                    .clamp(0.0, 1.0);
                let amount = credibility * reach * (1.0 + 0.5 * fragmentation);
                let tick = state.tick;
                let entry = state
                    .narratives
                    .entry(narrative.clone())
                    .or_insert_with(|| crate::state::NarrativeRuntimeState {
                        favors: favors.clone(),
                        credibility,
                        reach,
                        strength: 0.0,
                        first_seen_tick: tick,
                        last_reinforced_tick: tick,
                        firings: 0,
                        peak_strength: 0.0,
                    });
                let was_new = entry.firings == 0;
                // Re-tag credibility / reach on reinforcement: a later
                // event with higher reach should pull the live narrative
                // toward the new value (max-of-history) rather than
                // average. Same for credibility. Favors stays sticky to
                // the first-firing's choice so a malicious "switch sides"
                // reinforcement can't silently flip the dominance
                // attribution.
                entry.credibility = entry.credibility.max(credibility);
                entry.reach = entry.reach.max(reach);
                entry.strength = (entry.strength + amount).clamp(0.0, 1.0);
                entry.last_reinforced_tick = tick;
                entry.firings += 1;
                if entry.strength > entry.peak_strength {
                    entry.peak_strength = entry.strength;
                }
                let strength_after = entry.strength;
                let event_favors = entry.favors.clone();
                state
                    .narrative_events
                    .push(faultline_types::stats::NarrativeEvent {
                        tick,
                        narrative: narrative.clone(),
                        favors: event_favors,
                        credibility,
                        reach,
                        strength_after,
                        was_new,
                    });
            },
            EventEffect::Displacement { region, magnitude } => {
                // Epic D round-three item 4 — refugee / displacement
                // flows. Author-driven injection of displaced fraction
                // into a region. Magnitude is interpreted as a
                // delta-fraction-of-region-population in `[0, 1]`;
                // out-of-range / non-finite values are rejected at
                // scenario load. Unknown regions are also a load-time
                // rejection. Runtime is defensive: bad values are
                // skipped silently rather than poisoning state, and the
                // resulting `current_displaced` is clamped to `[0, 1]`.
                if !state.region_control.contains_key(region) {
                    continue;
                }
                let mag = if magnitude.is_finite() && *magnitude > 0.0 {
                    magnitude.clamp(0.0, 1.0)
                } else {
                    continue;
                };
                let entry = state.displacement.entry(region.clone()).or_default();
                let new_displaced = (entry.current_displaced + mag).clamp(0.0, 1.0);
                let actual_added = new_displaced - entry.current_displaced;
                entry.current_displaced = new_displaced;
                entry.total_inflow += actual_added;
                if entry.current_displaced > entry.peak_displaced {
                    entry.peak_displaced = entry.current_displaced;
                }
            },
            // Effects that require more complex handling are logged
            // but not fully resolved in this skeleton.
            EventEffect::InstitutionDefection { .. }
            | EventEffect::TechAccess { .. }
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
///
/// `campaigns` is the live in-flight kill-chain state (read-only).
/// Threaded through so the utility-driven adaptive AI scaffold (Epic
/// J round-one) can score the `AdaptiveCondition::AttributionAgainstSelf`
/// trigger without re-deriving attribution from the event log.
/// Empty `campaigns` (legacy scenarios with no kill chains) is the
/// fast path — utility evaluation reads it as zero attribution.
pub fn decision_phase(
    state: &mut SimulationState,
    scenario: &Scenario,
    map: &GameMap,
    campaigns: &BTreeMap<faultline_types::ids::KillChainId, crate::campaign::CampaignState>,
    rng: &mut impl Rng,
) -> BTreeMap<FactionId, Vec<FactionAction>> {
    let faction_ids: Vec<FactionId> = state.faction_states.keys().cloned().collect();
    let fog_of_war = scenario.simulation.fog_of_war;

    let mut all_actions = BTreeMap::new();

    for fid in &faction_ids {
        let evaluation = if fog_of_war {
            let world_view = ai::build_world_view(fid, state, scenario, map);
            ai::evaluate_actions_fog(fid, state, scenario, &world_view, map, campaigns, rng)
        } else {
            ai::evaluate_actions(fid, state, scenario, map, campaigns, rng)
        };

        // Capture per-faction utility decomposition for the post-run
        // report (Epic J round-one). Sums per-term contributions
        // across the *top-3 selected* actions only — the actions the
        // engine actually executes — so the report describes what
        // drove the visible behavior, not the whole candidate set.
        // No-op for factions with no `[utility]` profile (every
        // ScoredAction's `utility` is `None`).
        let mut term_sums: BTreeMap<faultline_types::faction::UtilityTerm, f64> = BTreeMap::new();
        let mut decision_count = 0u32;
        let mut top3 = Vec::with_capacity(3);
        for sa in evaluation.actions.into_iter().take(3) {
            if let Some(u) = &sa.utility {
                decision_count += 1;
                for (term, contribution) in &u.contributions {
                    *term_sums.entry(*term).or_insert(0.0) += contribution;
                }
            }
            top3.push(sa.action);
        }
        if decision_count > 0 {
            let entry = state.utility_decisions.entry(fid.clone()).or_default();
            entry.tick_count += 1;
            entry.decision_count += u64::from(decision_count);
            for (term, sum) in term_sums {
                *entry.term_sums.entry(term).or_insert(0.0) += sum;
            }
            // `fired_triggers` was populated by `ai::evaluate_actions`
            // / `evaluate_actions_fog` from the same `EffectiveWeights`
            // used to score the candidate actions, so reading it here
            // avoids a second `crate::utility::effective_weights` call
            // per faction per tick.
            for trigger_id in evaluation.fired_triggers {
                *entry.trigger_fires.entry(trigger_id).or_insert(0) += 1;
            }
        }

        all_actions.insert(fid.clone(), top3);
    }

    all_actions
}

// -----------------------------------------------------------------------
// Phase 3: Movement
// -----------------------------------------------------------------------

/// Resolve queued movement actions. Units move to adjacent regions
/// if the move is valid and their movement accumulator has reached
/// the unit threshold. Wires `ForceUnit.mobility`,
/// `TerrainModifier.movement_modifier`, and active environment
/// windows' `movement_factor` into a single per-tick rate gate.
pub fn movement_phase(
    state: &mut SimulationState,
    scenario: &Scenario,
    map: &GameMap,
    queued_actions: &BTreeMap<FactionId, Vec<FactionAction>>,
) {
    let tick = state.tick;
    for (faction_id, actions) in queued_actions {
        for action in actions {
            if let FactionAction::MoveUnit { force, destination } = action {
                move_unit(state, scenario, faction_id, force, destination, map, tick);
            }
        }
    }
}

fn move_unit(
    state: &mut SimulationState,
    scenario: &Scenario,
    faction_id: &FactionId,
    force_id: &ForceId,
    destination: &RegionId,
    map: &GameMap,
    tick: u32,
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

    // Compute effective mobility: unit mobility × terrain modifier
    // (source region) × environment movement_factor. NaN/negative
    // values are clamped to 0.0 via `.max(0.0)` (IEEE-754 fmax
    // semantics turn NaN into the other operand) so a malicious or
    // buggy override can't drive the accumulator negative. Mirrors
    // the graceful-degradation pattern in `find_contested_regions`
    // for `morale_modifier`.
    let source_terrain = scenario
        .map
        .terrain
        .iter()
        .find(|t| t.region == force.region);
    let terrain_modifier = source_terrain.map_or(1.0, |t| t.movement_modifier);
    let terrain_type = source_terrain.map_or(TerrainType::Rural, |t| t.terrain_type.clone());
    let env_factor = environment_movement_factor(scenario, &terrain_type, tick);
    let effective_mobility = (force.mobility * terrain_modifier * env_factor).max(0.0);

    // Move accumulator. Capped at 1.0 so the field can never
    // accumulate "saved up" moves between attempts — keeps the
    // gate's per-call semantics local. Default field value is 0.0
    // (post-deserialize), so a unit with `mobility = 1.0` and
    // identity terrain/env multipliers reaches the 1.0 threshold on
    // its first attempt and moves every subsequent tick — exactly
    // the legacy behavior.
    if let Some(force) = fs.forces.get_mut(force_id) {
        force.move_progress = (force.move_progress + effective_mobility).min(1.0);
        if force.move_progress < 1.0 {
            return;
        }
        force.move_progress -= 1.0;
        force.region = destination.clone();
    }
}

// -----------------------------------------------------------------------
// Phase 4: Combat
// -----------------------------------------------------------------------

/// Resolve combat in regions where opposing factions have forces.
pub fn combat_phase(state: &mut SimulationState, scenario: &Scenario, rng: &mut impl Rng) -> u32 {
    // Reset per-tick tech coverage counters before we resolve any
    // combat. The counters are scoped to the combat phase: each
    // contribution to a (region, opponent) pair bumps the counter
    // for the techs that were applied; once a card hits its
    // `coverage_limit`, further applications in the same tick are
    // skipped. Pure read-then-clear — no allocations beyond resetting
    // the BTreeMaps that already exist on each faction.
    for fs in state.faction_states.values_mut() {
        fs.tech_coverage_used.clear();
    }

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

        // Apply environmental defense modifier (weather / time-of-day).
        // Multiplies the base terrain defense; resolves to 1.0 when no
        // windows are declared or none are active.
        let env_defense_factor = environment_defense_factor(scenario, &terrain_type, state.tick);
        let terrain_defense = base_terrain_defense * env_defense_factor;

        // Pairwise combat: all faction pairs engage each other,
        // except mutually-Allied pairs (diplomacy behavioral
        // coupling). Cooperative pairs still fight if their forces
        // collide; only `Diplomacy::Allied` (in both directions)
        // blocks combat.
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

                // Combat reads `morale * command_effectiveness`
                // (via `effective_combat_morale`) rather than raw
                // morale so a leadership decapitation degrades
                // combat performance without rewriting morale —
                // keeping the political-phase / alliance-fracture
                // morale signal clean. See R3-4 in the improvement
                // plan and `update_command_effectiveness`.
                let morale_a = state
                    .faction_states
                    .get(fid_a)
                    .map_or(0.5, effective_combat_morale);
                let morale_b = state
                    .faction_states
                    .get(fid_b)
                    .map_or(0.5, effective_combat_morale);

                let guerrilla_a = state
                    .faction_states
                    .get(fid_a)
                    .is_some_and(|fs| fs.has_guerrilla_units());
                let guerrilla_b = state
                    .faction_states
                    .get(fid_b)
                    .is_some_and(|fs| fs.has_guerrilla_units());

                let (tech_modifier_a, applied_a) =
                    compute_tech_combat_modifier(fid_a, fid_b, state, scenario, &terrain_type);
                let (tech_modifier_b, applied_b) =
                    compute_tech_combat_modifier(fid_b, fid_a, state, scenario, &terrain_type);
                // Bump per-tick coverage counters for techs that were
                // actually applied (the helper already enforced the
                // cap when deciding what to apply). Done in a separate
                // mutation pass so the helper itself can take a `&`
                // reference to `state` — combats touch many factions
                // per pair, and threading `&mut` through the helper
                // would conflict with the surrounding loop's reads.
                if let Some(fs) = state.faction_states.get_mut(fid_a) {
                    for tid in applied_a {
                        *fs.tech_coverage_used.entry(tid).or_insert(0) += 1;
                    }
                }
                if let Some(fs) = state.faction_states.get_mut(fid_b) {
                    for tid in applied_b {
                        *fs.tech_coverage_used.entry(tid).or_insert(0) += 1;
                    }
                }

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
///
/// Returns `(modifier, applied_techs)`. The caller bumps each applied
/// card's per-tick coverage counter — kept out of this function so the
/// helper can stay `&` over `state`. Coverage gating itself happens
/// here by reading the current counter value: a card whose
/// `coverage_limit` has already been reached this tick is skipped
/// (contributes nothing to `modifier`, omitted from `applied_techs`).
/// Cards without a `coverage_limit` (the legacy default) bypass the
/// gate entirely.
fn compute_tech_combat_modifier(
    faction_id: &FactionId,
    opponent_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    terrain: &TerrainType,
) -> (f64, Vec<TechCardId>) {
    let faction_state = match state.faction_states.get(faction_id) {
        Some(fs) => fs,
        None => return (1.0, Vec::new()),
    };
    let tech_deployed = &faction_state.tech_deployed;

    let empty_techs = Vec::new();
    let opponent_techs = state
        .faction_states
        .get(opponent_id)
        .map_or(&empty_techs, |fs| &fs.tech_deployed);

    let mut modifier = 1.0;
    let mut applied: Vec<TechCardId> = Vec::new();

    for tech_id in tech_deployed {
        let card = match scenario.technology.get(tech_id) {
            Some(c) => c,
            None => continue,
        };

        if faultline_tech::is_countered(card, opponent_techs) {
            continue;
        }

        // Coverage gate. Only enforced for cards with an authored
        // `coverage_limit`; uncapped cards bypass
        // the gate entirely (and stay out of the per-tick counter
        // map, so legacy scenarios pay zero bookkeeping overhead).
        // The counter is updated by the caller after this function
        // returns, so reading it here gives the count from prior
        // (region, opponent) pairs in this tick.
        let has_limit = card.coverage_limit.is_some();
        if let Some(limit) = card.coverage_limit {
            let used = faction_state
                .tech_coverage_used
                .get(tech_id)
                .copied()
                .unwrap_or(0);
            if used >= limit {
                continue;
            }
        }

        let resolved = faultline_tech::apply_tech_effects(card, terrain);
        for re in &resolved {
            if let TechEffect::CombatModifier { factor } = &re.effect {
                modifier *= factor * re.effectiveness;
            }
        }
        if has_limit {
            applied.push(tech_id.clone());
        }
    }

    (modifier.clamp(0.25, 3.0), applied)
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
        // Supply pressure. Computed before reading `resource_rate` so
        // income is attenuated by
        // the latest network state. Pure function — no RNG, no
        // allocation, returns (1.0, false) for any faction without
        // an owned non-degenerate supply network so legacy scenarios
        // are untouched. Captured onto `current_supply_pressure` and
        // rolled into per-faction sum / min / pressured-tick counters
        // for the post-run report. We sample only when at least one
        // non-degenerate owned supply network actually contributed to
        // the product — a faction whose only supply networks have
        // zero baseline capacity never carried supply, and emitting
        // `(mean=1.0, min=1.0)` samples for it would falsely
        // advertise "supply intact" in the report.
        let (pressure, sampled) = crate::supply::supply_pressure_for_faction(scenario, state, fid);
        if let Some(fs) = state.faction_states.get_mut(fid) {
            fs.current_supply_pressure = pressure;
            if sampled {
                fs.supply_pressure_sum += pressure;
                fs.supply_pressure_samples = fs.supply_pressure_samples.saturating_add(1);
                if pressure < fs.supply_pressure_min {
                    fs.supply_pressure_min = pressure;
                }
                if pressure < crate::supply::PRESSURE_REPORTING_THRESHOLD {
                    fs.supply_pressure_pressured_ticks =
                        fs.supply_pressure_pressured_ticks.saturating_add(1);
                }
            }
        }

        let tick = state.tick;
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
            // Income, attenuated by supply pressure. `pressure` is in
            // `[0, 1]` so a fully-cut supply line zeroes income but
            // never inverts it; an intact supply line (pressure = 1.0,
            // the legacy default) leaves income unchanged. Upkeep is
            // *not* attenuated — units still consume regardless of
            // whether resupply is reaching them, which is exactly why
            // cut supply lines hurt in the first place. We use the
            // local `pressure` here rather than re-reading
            // `fs.current_supply_pressure` so a future refactor that
            // splits or reorders the two `get_mut` blocks can't
            // silently apply the *previous* tick's pressure.
            fs.resources += resource_rate * pressure;

            // Upkeep.
            fs.resources = (fs.resources - upkeep).max(0.0);

            // Tech maintenance. Walk `tech_deployed` in declaration
            // order; for each
            // card, deduct `cost_per_tick` if affordable, otherwise
            // decommission the card (remove from `tech_deployed` and
            // record the loss). Decommissioning is final — the card
            // does not re-deploy if resources later recover, mirroring
            // the real-world "you can't conjure a deployed sensor mast
            // back into existence with a wire transfer" intuition.
            // Income, upkeep, and tech maintenance are all charged
            // *before* recruitment so the new-unit upkeep the next
            // tick is properly funded before we even consider
            // spawning replacements.
            //
            // Cards referenced in `tech_deployed` but absent from
            // `scenario.technology` are kept (consistent with init —
            // missing tech is a silent no-op, not a runtime error).
            // A separate audit could promote that to a load-time
            // error.
            let mut decommissioned_now: Vec<TechCardId> = Vec::new();
            for tech_id in &fs.tech_deployed {
                let cost = scenario
                    .technology
                    .get(tech_id)
                    .map_or(0.0, |c| c.cost_per_tick);
                if cost > fs.resources {
                    decommissioned_now.push(tech_id.clone());
                } else {
                    fs.resources -= cost;
                    fs.tech_maintenance_spend += cost;
                }
            }
            if !decommissioned_now.is_empty() {
                let removed: std::collections::BTreeSet<TechCardId> =
                    decommissioned_now.iter().cloned().collect();
                fs.tech_deployed.retain(|id| !removed.contains(id));
                for tech_id in decommissioned_now {
                    fs.tech_decommissioned.push((tick, tech_id));
                }
            }

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
                    move_progress: 0.0,
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
        // Capture the activation onto the per-run log before processing
        // so the post-run report records the firing even if the
        // processor itself short-circuits on a malformed action set.
        // `action_kinds` mirrors the order of `actions` so the report
        // can describe "what fired in what order"; the pretty
        // discriminant strings come from the dedicated helper rather
        // than `Debug` so the manifest schema stays stable when new
        // `CivilianAction` variants are added with extra payload.
        state
            .civilian_activations
            .push(faultline_types::stats::CivilianActivationEvent {
                tick: state.tick,
                segment: activation.segment_id.clone(),
                favored_faction: activation.favored_faction.clone(),
                action_kinds: activation
                    .actions
                    .iter()
                    .map(civilian_action_kind)
                    .map(str::to_owned)
                    .collect(),
            });
        process_civilian_activation(state, scenario, activation, rng);
    }
}

/// Stable discriminant name for a [`CivilianAction`] — used by the
/// per-run civilian-activation log to record what an activation will
/// fire without dragging the typed payload into the report schema.
///
/// Keeping the mapping local (rather than deriving it from `Debug`)
/// ensures adding a new `CivilianAction` variant fails compilation
/// here, forcing a deliberate decision about how to surface it in
/// the cross-run summary.
fn civilian_action_kind(action: &faultline_types::politics::CivilianAction) -> &'static str {
    use faultline_types::politics::CivilianAction;
    match action {
        CivilianAction::NonCooperation { .. } => "NonCooperation",
        CivilianAction::Protest { .. } => "Protest",
        CivilianAction::Intelligence { .. } => "Intelligence",
        CivilianAction::MaterialSupport { .. } => "MaterialSupport",
        CivilianAction::ArmedResistance { .. } => "ArmedResistance",
        CivilianAction::Flee { .. } => "Flee",
        CivilianAction::Sabotage { .. } => "Sabotage",
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
                            move_progress: 0.0,
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
                // Reduce segment fraction (already activated, so find it)
                // and push the fled fraction into the displacement
                // store for each region the segment was concentrated
                // in. Splits the rate evenly across concentrated regions
                // so a flee that depopulates a 0.10 fraction across two
                // regions adds 0.05 displaced to each. Epic D round-three
                // item 4 — the previous behavior was "the population
                // disappears", which is fine for political bookkeeping
                // but loses signal for the displacement-flow mechanic.
                for seg in &mut state.political_climate.population_segments {
                    if seg.id == activation.segment_id {
                        seg.fraction = (seg.fraction - rate).max(0.0);
                    }
                }
                if !activation.concentrated_in.is_empty() && *rate > 0.0 {
                    let per_region = rate / activation.concentrated_in.len() as f64;
                    for region in &activation.concentrated_in {
                        if !state.region_control.contains_key(region) {
                            continue;
                        }
                        let entry = state.displacement.entry(region.clone()).or_default();
                        let new_displaced = (entry.current_displaced + per_region).clamp(0.0, 1.0);
                        let actual_added = new_displaced - entry.current_displaced;
                        entry.current_displaced = new_displaced;
                        entry.total_inflow += actual_added;
                        if entry.current_displaced > entry.peak_displaced {
                            entry.peak_displaced = entry.current_displaced;
                        }
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

/// Process information warfare effects.
///
/// Reads four media-landscape fields:
/// - `disinformation_susceptibility` (existing): high values raise
///   tension above the 0.5 baseline.
/// - `state_control` (existing): high values dampen tension above the
///   0.7 baseline.
/// - `fragmentation` (newly read): amplifies the disinfo→tension
///   effect — a fragmented audience is more vulnerable to bubble-
///   targeted narratives than a unified one.
/// - `social_media_penetration × internet_availability` (newly read):
///   the penetration field is gated by internet availability so a
///   "lights out" scenario neutralizes both. The composed value
///   amplifies the disinfo→tension effect — high reach amplifies
///   whichever narrative is dominant in the moment.
pub fn information_phase(state: &mut SimulationState, _scenario: &Scenario) {
    // Information warfare affects civilian sympathy and tension.
    let media = &state.political_climate.media_landscape;
    let disinfo_factor = media.disinformation_susceptibility;
    let fragmentation = media.fragmentation.clamp(0.0, 1.0);
    let social_media = media.social_media_penetration.clamp(0.0, 1.0);
    let internet = media.internet_availability.clamp(0.0, 1.0);
    let effective_social_media = social_media * internet;

    // High disinformation susceptibility increases tension. Both
    // multipliers are >= 1.0 (the no-amp default at fragmentation = 0
    // and effective_social_media = 0 reproduces the legacy delta of
    // `(disinfo_factor - 0.5) * 0.01`). The bound at max-amp is ~2×
    // (fragmentation = 1.0 contributes 0.5, effective_social_media =
    // 1.0 contributes 0.5).
    if disinfo_factor > 0.5 {
        let amp = 1.0 + 0.5 * fragmentation + 0.5 * effective_social_media;
        let delta = (disinfo_factor - 0.5) * 0.01 * amp;
        state.political_climate.tension = (state.political_climate.tension + delta).min(1.0);
    }

    // High state media control dampens tension.
    if media.state_control > 0.7 {
        let dampening = (media.state_control - 0.7) * 0.005;
        state.political_climate.tension = (state.political_climate.tension - dampening).max(0.0);
    }
}

// -----------------------------------------------------------------------
// Phase 7b: Narrative competition (Epic D round-three item 4)
// -----------------------------------------------------------------------

/// Per-tick narrative-strength decay multiplier baseline. The
/// canonical decay rate used in `narrative_phase` is
/// `BASE_NARRATIVE_DECAY × (1 − 0.5 × reach)` — a high-reach narrative
/// (saturated in the media landscape) decays at roughly half the rate
/// of a low-reach one. Tunable via constants here so authors don't
/// see the values in the schema.
const BASE_NARRATIVE_DECAY: f64 = 0.08;

/// Strength below which a narrative is dropped from the live store.
/// Saves the per-tick scoring loop from carrying epsilon-strength
/// entries indefinitely. Rounded so a narrative that reinforced
/// briefly and then went silent fades out within ~30–40 ticks.
const NARRATIVE_DROP_EPSILON: f64 = 0.005;

/// Base sympathy nudge per tick from a dominant narrative on segments
/// receptive to its disinformation slope. Multiplies through
/// `disinformation_susceptibility × strength × credibility` so a
/// low-credibility narrative pushes sympathy more slowly than a
/// high-credibility one.
const NARRATIVE_SYMPATHY_NUDGE: f64 = 0.02;

/// Tension contribution per unit of `(strength × credibility)` summed
/// across the narrative store. Capped at +0.02 / tick after summation
/// — the narrative phase is meant to be a slow-burn pressure source,
/// not a runaway tension generator.
const NARRATIVE_TENSION_RATE: f64 = 0.005;
const NARRATIVE_MAX_TENSION_DELTA: f64 = 0.02;

/// Process narrative competition end-of-tick (Epic D round-three item 4).
///
/// Order of operations:
/// 1. Decay every active narrative's strength by a reach-discounted
///    base rate. Drop entries that fell below `NARRATIVE_DROP_EPSILON`.
/// 2. Score per-faction information dominance: sum `strength ×
///    credibility` over narratives that favor each faction. The
///    leading faction (max sum, ties broken `BTreeMap`-lexicographically)
///    accrues a dominance tick on `narrative_dominance_ticks`.
/// 3. Apply sympathy nudges: for each population segment, the dominant
///    narrative's `favors` faction (if any) gets a sympathy bump
///    scaled by the segment's `disinformation_susceptibility` analogue
///    — actually, since population segments don't carry a per-segment
///    susceptibility, we use the global media-landscape value. The
///    nudge is a one-sided pull (no symmetric zero-sum redistribution)
///    so total sympathy mass can drift; that's fine — sympathy is
///    already clamped per-faction.
/// 4. Add a tension delta proportional to total narrative pressure
///    (sum of `strength × credibility` across the entire store), capped
///    at `NARRATIVE_MAX_TENSION_DELTA`.
/// 5. Update `non_kinetic.information_dominance` to the leading faction's
///    score (max over all factions); zero when no narrative is active.
///
/// Pure function of `(state, scenario)` — no RNG. Determinism preserved.
pub fn narrative_phase(state: &mut SimulationState, scenario: &Scenario) {
    if state.narratives.is_empty() {
        // Reset information_dominance even when the store is empty: a
        // narrative that decayed to nothing this tick should stop
        // contributing. Cheap and unconditional so the metric snapshot
        // doesn't drift.
        state.non_kinetic.information_dominance = 0.0;
        return;
    }

    // Step 1: decay + drop. `BTreeMap::retain` avoids the prior
    // `Vec<String>` key-clone allocation; iteration order is still
    // ascending, which is what the rest of the phase assumes.
    state.narratives.retain(|_, entry| {
        let decay = BASE_NARRATIVE_DECAY * (1.0 - 0.5 * entry.reach.clamp(0.0, 1.0));
        entry.strength = (entry.strength - decay).max(0.0);
        entry.strength >= NARRATIVE_DROP_EPSILON
    });

    if state.narratives.is_empty() {
        state.non_kinetic.information_dominance = 0.0;
        return;
    }

    // Step 2: dominance score per faction. Iteration order is
    // deterministic via `BTreeMap` over both narratives and the
    // `faction_scores` accumulator.
    let mut faction_scores: BTreeMap<FactionId, f64> = BTreeMap::new();
    let mut total_pressure: f64 = 0.0;
    for entry in state.narratives.values() {
        let pressure = entry.strength * entry.credibility;
        total_pressure += pressure;
        if let Some(fav) = &entry.favors {
            *faction_scores.entry(fav.clone()).or_insert(0.0) += pressure;
        }
    }

    // Leading faction = max score; on score tie, the lexicographically
    // *largest* `FactionId` wins (the `then_with(|| b.0.cmp(a.0))`
    // inversion). The same direction is used by
    // `narrative_dynamics::compute_narrative_dynamics`, so cross-report
    // attribution stays coherent.
    let leading: Option<(FactionId, f64)> = faction_scores
        .iter()
        .max_by(|a, b| {
            a.1.partial_cmp(b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(a.0))
        })
        .map(|(fid, score)| (fid.clone(), *score));

    let leading_score = leading.as_ref().map(|(_, s)| *s).unwrap_or(0.0);
    state.non_kinetic.information_dominance = leading_score.clamp(0.0, 1.0);

    if let Some((fid, score)) = &leading
        && *score > 0.0
    {
        *state
            .narrative_dominance_ticks
            .entry(fid.clone())
            .or_insert(0) += 1;
        let peak = state
            .narrative_peak_dominance
            .entry(fid.clone())
            .or_insert(0.0);
        if *score > *peak {
            *peak = score.clamp(0.0, 1.0);
        }
    }

    // Step 3: sympathy nudges. Pull each segment's sympathy toward the
    // leading faction by a small amount scaled by media
    // `disinformation_susceptibility` — segments in fragile
    // information environments shift faster. The nudge is *additive*,
    // not zero-sum redistribution, so total sympathy mass can drift
    // up; the per-faction clamp at `[-1, 1]` and the segment-level
    // sympathy clamp prevent runaway.
    let susceptibility = scenario
        .political_climate
        .media_landscape
        .disinformation_susceptibility
        .clamp(0.0, 1.0);
    if let Some((leader_fid, leader_score)) = &leading
        && *leader_score > 0.0
    {
        // Scale by the leader's contribution density: leader_score is
        // already in `[0, 1]` since it's a sum of `strength × credibility`
        // for narratives with strength + credibility each in `[0, 1]`,
        // and saturation just clips the multiplier at 1.0.
        let nudge_factor = NARRATIVE_SYMPATHY_NUDGE * susceptibility * leader_score.min(1.0);
        if nudge_factor > 0.0 {
            for seg in &mut state.political_climate.population_segments {
                for sym in &mut seg.sympathies {
                    if &sym.faction == leader_fid {
                        sym.sympathy = (sym.sympathy + nudge_factor).clamp(-1.0, 1.0);
                    }
                }
            }
        }
    }

    // Step 4: tension contribution. Total pressure across the whole
    // store nudges global tension upward; cap the per-tick delta so
    // the narrative phase stays a slow-burn source.
    if total_pressure > 0.0 {
        let delta = (total_pressure * NARRATIVE_TENSION_RATE).min(NARRATIVE_MAX_TENSION_DELTA);
        state.political_climate.tension = (state.political_climate.tension + delta).clamp(0.0, 1.0);
    }
}

// -----------------------------------------------------------------------
// Phase 7c: Displacement flow (Epic D round-three item 4)
// -----------------------------------------------------------------------

/// Per-tick fraction of a region's displaced population that propagates
/// to adjacent regions. The total outflow is split evenly across
/// adjacent regions; the rest stays put for the next tick's iteration.
const DISPLACEMENT_PROPAGATION_RATE: f64 = 0.10;

/// Per-tick fraction of a region's displaced population that absorbs
/// back into the resident population (assimilation / settlement). A
/// region with zero adjacencies — a hand-crafted edge case that no
/// bundled scenario authors — would still steadily decay via
/// absorption alone, so the analytical signal isn't lost.
const DISPLACEMENT_ABSORPTION_RATE: f64 = 0.05;

/// Tension contribution per unit of average displaced fraction across
/// regions. Capped at +0.005 / tick after summation: displacement
/// stress should accumulate slowly relative to direct combat /
/// disinformation tension.
const DISPLACEMENT_TENSION_RATE: f64 = 0.01;
const DISPLACEMENT_MAX_TENSION_DELTA: f64 = 0.005;

/// Process displacement-flow propagation end-of-tick (Epic D
/// round-three item 4). Pure function of `(state, scenario)` — no
/// RNG.
///
/// Each region's `current_displaced` is split into three buckets:
///
/// 1. `outflow = current × DISPLACEMENT_PROPAGATION_RATE`, distributed
///    evenly across adjacent regions. The receiving regions' counters
///    are bumped *after* every source has computed its outflow, so
///    "ricochet" effects (a region propagates, receives, then
///    propagates again the same tick) are deferred to the next tick.
///    This matches the existing `network` and `supply` phase
///    conventions: per-tick state mutation is single-pass, not
///    iterative-to-fixedpoint.
/// 2. `absorbed = current × DISPLACEMENT_ABSORPTION_RATE`, removed
///    from the live count. Tracks the fraction that stops being "in
///    motion" and merges back into the resident population.
/// 3. The remainder stays put for next tick.
///
/// After propagation, the total displaced fraction across regions
/// contributes a small tension delta capped at
/// `DISPLACEMENT_MAX_TENSION_DELTA`.
pub fn displacement_phase(state: &mut SimulationState, scenario: &Scenario) {
    if state.displacement.is_empty() {
        return;
    }

    // Snapshot the current displaced values per region so propagation
    // is single-pass — every region's outflow reads the *pre-tick*
    // displaced value, not values that other regions have already
    // written.
    let snapshot: BTreeMap<RegionId, f64> = state
        .displacement
        .iter()
        .map(|(rid, st)| (rid.clone(), st.current_displaced))
        .collect();

    let mut inflows: BTreeMap<RegionId, f64> = BTreeMap::new();

    for (rid, displaced) in &snapshot {
        if *displaced <= 0.0 {
            continue;
        }
        let region = match scenario.map.regions.get(rid) {
            Some(r) => r,
            None => continue,
        };
        // Propagation: split outflow across known adjacent regions.
        // Borders that don't resolve (typo, race-condition with map
        // mutation) are skipped; the residual stays put. The border
        // list itself is `Vec<RegionId>` — duplicates would be a
        // load-time validation error, so we treat the iteration as a
        // set. Two passes over `region.borders` (count + distribute)
        // avoid a per-region `Vec<RegionId>` heap allocation on the
        // hot path; deterministic because both passes apply the same
        // filter in the same order.
        let outflow_total = displaced * DISPLACEMENT_PROPAGATION_RATE;
        let absorbed = displaced * DISPLACEMENT_ABSORPTION_RATE;
        let valid_neighbor_count = region
            .borders
            .iter()
            .filter(|nid| scenario.map.regions.contains_key(nid))
            .count();
        if valid_neighbor_count > 0 && outflow_total > 0.0 {
            let per_neighbor = outflow_total / valid_neighbor_count as f64;
            for nid in region
                .borders
                .iter()
                .filter(|nid| scenario.map.regions.contains_key(nid))
            {
                *inflows.entry(nid.clone()).or_insert(0.0) += per_neighbor;
            }
        }
        let actual_outflow = if valid_neighbor_count == 0 {
            0.0
        } else {
            outflow_total
        };
        if let Some(entry) = state.displacement.get_mut(rid) {
            entry.total_outflow += actual_outflow;
            entry.total_absorbed += absorbed;
            entry.current_displaced =
                (entry.current_displaced - actual_outflow - absorbed).max(0.0);
        }
    }

    // Apply inflows. Regions receiving propagation may not already
    // have a displacement entry, so insert default on demand.
    for (rid, inflow) in inflows {
        if !scenario.map.regions.contains_key(&rid) {
            continue;
        }
        let entry = state.displacement.entry(rid).or_default();
        let new_displaced = (entry.current_displaced + inflow).clamp(0.0, 1.0);
        let actual_added = new_displaced - entry.current_displaced;
        entry.current_displaced = new_displaced;
        entry.total_inflow += actual_added;
        if entry.current_displaced > entry.peak_displaced {
            entry.peak_displaced = entry.current_displaced;
        }
    }

    // Stress-tick counter and tension delta. Region count is bounded
    // by scenario.map.regions, so summation is O(R). Peak is already
    // updated everywhere current_displaced grows (event effects, flee
    // sources, propagation inflows); regions only lose mass in this
    // phase, so no peak update is needed here.
    //
    // Convention: `stressed_ticks` reads post-outflow/absorption, so a
    // region that started the tick with displacement but drained to
    // zero by end-of-phase does not accrue a stressed tick for that
    // tick. Reads as "ticks the region ended with residual displaced
    // mass" rather than "ticks the region carried any displaced mass
    // at any point". The under-count is intentional and matches the
    // single-pass propagation convention used elsewhere in this phase.
    let mut sum_displaced: f64 = 0.0;
    let mut nonzero_regions: u32 = 0;
    for st in state.displacement.values_mut() {
        if st.current_displaced > 0.0 {
            st.stressed_ticks += 1;
            sum_displaced += st.current_displaced;
            nonzero_regions += 1;
        }
    }

    if nonzero_regions > 0 {
        let avg_displaced = sum_displaced / f64::from(nonzero_regions);
        let delta = (avg_displaced * DISPLACEMENT_TENSION_RATE).min(DISPLACEMENT_MAX_TENSION_DELTA);
        state.political_climate.tension = (state.political_climate.tension + delta).clamp(0.0, 1.0);
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
// Environment helpers (weather, time-of-day)
// -----------------------------------------------------------------------

/// Whether `window` applies to a region of the given terrain.
fn window_covers(window: &EnvironmentWindow, terrain: &TerrainType) -> bool {
    window.applies_to.is_empty() || window.applies_to.contains(terrain)
}

/// Multiplicative product of every active window's `movement_factor`
/// that covers `terrain` at `tick`. Empty schedule resolves to 1.0,
/// so legacy scenarios are unchanged. Read by the movement phase
/// when computing a unit's effective mobility for the accumulator
/// gate.
pub fn environment_movement_factor(scenario: &Scenario, terrain: &TerrainType, tick: u32) -> f64 {
    multiplicative_factor(&scenario.environment, tick, |w| {
        if window_covers(w, terrain) {
            Some(w.movement_factor)
        } else {
            None
        }
    })
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
// Leadership caps (decapitation + succession)
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

/// Recompute each faction's `command_effectiveness` from its current
/// `effective_leadership_factor`.
///
/// Replaces the Epic D round-one morale-clamping behavior. The
/// previous implementation pushed leadership degradation into
/// `morale` directly; combat read raw morale, so the cap surfaced
/// the decapitation. That conflated two distinct axes — rank-and-
/// file *will to fight* and chain-of-command *capacity to direct
/// that will* — and made the morale-floor alliance-fracture
/// condition incidentally fire on a leadership strike.
///
/// Now: morale stays untouched and `command_effectiveness` is
/// written to the leadership factor. Combat and AI threat-scoring
/// read `morale * command_effectiveness`, so a decapitation still
/// degrades both, but the morale signal stays clean for the
/// political phase, alliance-fracture evaluation, and any future
/// consumer that wants the raw will-to-fight axis. Future
/// command-degrading effects (logistics-targeted strikes, command-
/// jamming, supply-pressure tier escalation) can compose
/// multiplicatively into `command_effectiveness` without colliding
/// with morale's other consumers.
///
/// Composition contract: this writer resets every faction's
/// `command_effectiveness` to `1.0` then multiplies the leadership
/// factor in. Future command-degrading sources (logistics-targeted
/// strikes, command-jamming, supply-pressure tier escalation) should
/// run *after* this writer and multiply their own factor into the
/// field; resetting first ensures repeated ticks don't compound the
/// leadership factor with itself, and using `*=` here keeps the
/// pattern uniform so the next writer added doesn't have to special-
/// case the first multiplication.
///
/// Bit-identical fast path: when no faction declares a `leadership`
/// cadre the function returns immediately and every faction's
/// `command_effectiveness` stays at its `1.0` default. The
/// reset+multiply pattern is mathematically equivalent to a direct
/// overwrite while there is only one writer (`1.0 × factor =
/// factor`), so switching from overwrite to reset+multiply alone did
/// not change any cadre-bearing scenario's `output_hash`. Note that
/// the broader R3-4 morale/command split *does* shift cadre-bearing
/// scenario hashes (raw morale is no longer clamped by the
/// leadership factor, so combat outcomes diverge); see the PR
/// description and `CLAUDE.md`'s R3-4 section for the full hash
/// movement.
pub fn update_command_effectiveness(state: &mut SimulationState, scenario: &Scenario) {
    // Legacy scenarios with no cadres pay only this scan; the
    // command_effectiveness field stays at its 1.0 default.
    if !scenario.factions.values().any(|f| f.leadership.is_some()) {
        return;
    }
    let tick = state.tick;
    // Compute factors first so we don't hold an immutable borrow
    // while writing. Faction order is BTreeMap-deterministic.
    let factors: Vec<(FactionId, f64)> = state
        .faction_states
        .keys()
        .map(|fid| {
            let factor = effective_leadership_factor(state, scenario, fid, tick);
            (fid.clone(), factor)
        })
        .collect();
    for (fid, factor) in factors {
        if let Some(fs) = state.faction_states.get_mut(&fid) {
            // Reset to baseline then multiply in. Equivalent to direct
            // overwrite while there is only one writer, but composes
            // correctly the moment a second command-degrading source is
            // added (and prevents repeated calls within a tick from
            // compounding the leadership factor with itself).
            fs.command_effectiveness = 1.0;
            fs.command_effectiveness *= factor.clamp(0.0, 1.0);
        }
    }
}

/// Effective combat/AI morale for a faction: raw morale modulated
/// by the chain-of-command capacity to translate that morale into
/// directed action.
///
/// Combat reads this through [`effective_combat_morale`] rather
/// than `fs.morale` directly so a leadership decapitation surfaces
/// in casualty outcomes without polluting the political-phase /
/// alliance-fracture morale signal.
///
/// `command_effectiveness` is written end-of-tick by
/// [`update_command_effectiveness`]; the field defaults to `1.0`
/// for legacy factions and the first tick before the writer has
/// run, so the legacy fast path (no cadre declared anywhere)
/// produces identical numerical output to the pre-refactor
/// morale-only read.
pub fn effective_combat_morale(fs: &RuntimeFactionState) -> f64 {
    (fs.morale * fs.command_effectiveness).clamp(0.0, 1.0)
}
