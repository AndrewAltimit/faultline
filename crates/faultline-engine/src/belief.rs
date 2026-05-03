//! Belief asymmetry phase (Epic M round-one).
//!
//! Implements the per-faction persistent belief state, observation-
//! driven updates, decay, deception event handling, and the
//! belief-accuracy bookkeeping the post-run report consumes. This
//! module is the engine-side companion to
//! [`faultline_types::belief`], which carries the schema.
//!
//! ## When does the belief phase run?
//!
//! The belief phase runs end-of-tick, after combat and political
//! phases have settled the world state, so beliefs reflect what was
//! observable *after* the tick's events resolved. The order matters:
//! a deception event injected mid-tick must override a same-tick
//! direct observation only if the direct observation didn't actually
//! happen (i.e. the believer wasn't watching when the deception
//! landed). Round-one keeps it simple: deception events apply during
//! `apply_event_effects` (early in the tick), and the
//! `belief_phase` runs after combat — so a deception planted at
//! tick start gets *overwritten* by the believer's direct
//! end-of-tick observation if visibility allowed it. This matches
//! the real-world semantic "if you saw the truth with your own
//! eyes, the deception didn't take."
//!
//! ## Determinism
//!
//! Every helper is a pure function of `(state, scenario, map)`. No
//! RNG, no `HashMap`, `BTreeMap`-ordered iteration. The belief
//! phase is idempotent across same-seed runs and produces no
//! observable change beyond the persistent belief state, the
//! per-faction counters, and the optional snapshot stream.
//!
//! ## Backward compatibility
//!
//! Scenarios that omit `simulation.belief_model` (or set
//! `enabled = false`) get the legacy fast path: the belief phase
//! short-circuits in O(1), `belief_states` and `belief_counters`
//! stay empty, and the AI consumes ground truth as it has since
//! pre-Epic-M.

use std::collections::BTreeMap;

use faultline_geo::GameMap;
use faultline_types::belief::{
    BeliefForce, BeliefModelConfig, BeliefRegion, BeliefScalar, BeliefSource, DeceptionPayload,
    FactionBelief, IntelligencePayload,
};
use faultline_types::ids::{FactionId, ForceId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::BeliefSnapshot;
use faultline_types::strategy::{DetectedForce, FactionWorldView, PoliticalClimateView};

use crate::ai::compute_visible_regions;
use crate::state::{BeliefRunCounters, SimulationState};

/// True iff the scenario opts into the belief model with
/// `enabled = true`.
///
/// Pure scalar predicate. Centralized so call sites everywhere read
/// from the same source of truth — adding alternative enables (e.g.
/// "force enabled when any DeceptionOp is authored") in a future
/// round becomes a one-line change here.
pub fn belief_enabled(scenario: &Scenario) -> bool {
    scenario
        .simulation
        .belief_model
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false)
}

/// Return the active [`BeliefModelConfig`] or a sensible default.
///
/// Callers should guard the call with [`belief_enabled`] when they
/// only care about the active path; this helper is for paths that
/// always need a config (e.g. validation).
pub fn belief_config(scenario: &Scenario) -> BeliefModelConfig {
    scenario.simulation.belief_model.clone().unwrap_or_default()
}

/// Initialize per-faction belief state at engine startup.
///
/// Called once from `engine::initialize_state` when belief mode is
/// enabled. Builds an initial `FactionBelief` per faction populated
/// with the believer's full ground-truth view of *its own* faction
/// (own forces, own controlled regions, own morale / resources at
/// confidence 1.0) plus whatever is visible to the believer at
/// tick 0 from the visibility computation. No-op (returns an empty
/// map) when belief mode is off.
pub fn initialize_belief_states(
    state: &SimulationState,
    scenario: &Scenario,
    map: &GameMap,
) -> BTreeMap<FactionId, FactionBelief> {
    if !belief_enabled(scenario) {
        return BTreeMap::new();
    }
    let mut out = BTreeMap::new();
    for fid in scenario.factions.keys() {
        let mut belief = FactionBelief {
            faction: fid.clone(),
            ..Default::default()
        };
        observe_into_belief(&mut belief, fid, state, scenario, map, 0);
        out.insert(fid.clone(), belief);
    }
    out
}

/// Initialize the per-faction running counters when belief mode is on.
pub fn initialize_belief_counters(scenario: &Scenario) -> BTreeMap<FactionId, BeliefRunCounters> {
    if !belief_enabled(scenario) {
        return BTreeMap::new();
    }
    scenario
        .factions
        .keys()
        .map(|fid| (fid.clone(), BeliefRunCounters::default()))
        .collect()
}

/// End-of-tick belief-update phase. Consumed by `tick.rs` after the
/// combat and political phases.
///
/// Three sub-steps in order:
/// 1. Decay every belief entry's confidence by the per-tick rate from
///    `BeliefModelConfig`. Decay applies to *every* entry,
///    including ones that will be refreshed in step 2 — but step 2
///    overwrites those with `confidence = 1.0`, so the net effect is
///    "fresh observations stay fresh, unobserved entries age".
/// 2. For each faction, run the visibility computation against the
///    current ground-truth state and refresh every visible entry.
///    Direct observations clear the `Deceived` source tag and reset
///    confidence to 1.0.
/// 3. Prune entries whose confidence has fallen strictly below the
///    `prune_threshold`.
///
/// Updates `belief_counters` in lock-step: each tick where the
/// believer holds at least one force belief, the running force-error
/// sum is incremented by the per-tick mean error against ground
/// truth.
pub fn belief_phase(state: &mut SimulationState, scenario: &Scenario, map: &GameMap) {
    if !belief_enabled(scenario) {
        return;
    }
    let cfg = belief_config(scenario);
    let tick = state.tick;

    // Build the per-faction observation snapshot up front so the
    // mutable belief loop doesn't need to re-borrow `state`.
    let faction_ids: Vec<FactionId> = scenario.factions.keys().cloned().collect();
    let mut new_snapshots: Vec<(FactionId, BeliefSnapshot)> = Vec::new();
    let snapshot_interval = cfg.snapshot_interval;
    let take_snapshot_this_tick = snapshot_interval > 0 && tick.is_multiple_of(snapshot_interval);

    // Pre-compute the per-tick error contributions while only
    // immutably borrowing state. Then apply mutations.
    let mut counter_updates: BTreeMap<FactionId, (u32, f64, u32, f64)> = BTreeMap::new();

    // First pass: decay + refresh for each faction.
    for fid in &faction_ids {
        // Take ownership of the belief temporarily so we can mutate
        // it while holding an immutable borrow on `state`.
        let mut belief = state
            .belief_states
            .remove(fid)
            .unwrap_or_else(|| FactionBelief {
                faction: fid.clone(),
                ..Default::default()
            });

        decay_belief(&mut belief, &cfg);
        observe_into_belief(&mut belief, fid, state, scenario, map, tick);
        prune_belief(&mut belief, cfg.prune_threshold);
        belief.last_updated_tick = tick;

        // Compute per-tick error contributions for this faction.
        let (force_err_n, force_err_sum, region_n, region_sum) =
            compute_accuracy_contribution(&belief, fid, state);
        if force_err_n > 0 || region_n > 0 {
            counter_updates.insert(
                fid.clone(),
                (force_err_n, force_err_sum, region_n, region_sum),
            );
        }

        if take_snapshot_this_tick {
            new_snapshots.push((fid.clone(), summarize_belief(&belief, tick)));
        }

        state.belief_states.insert(fid.clone(), belief);
    }

    // Apply counter updates after all beliefs have been refreshed.
    for (fid, (force_n, force_sum, region_n, region_sum)) in counter_updates {
        let counter = state.belief_counters.entry(fid).or_default();
        if force_n > 0 {
            counter.force_belief_ticks += 1;
            counter.force_strength_error_sum += force_sum / f64::from(force_n);
        }
        if region_n > 0 {
            counter.region_belief_ticks += 1;
            // `region_sum` is already the per-tick fraction
            // (correct/region_n) returned by compute_accuracy_contribution.
            // The downstream consumer divides by `region_belief_ticks`
            // to get the cross-tick mean, so we must not re-normalize.
            counter.region_accuracy_sum += region_sum;
        }
    }

    // Append the optional belief-snapshot stream.
    for (fid, snap) in new_snapshots {
        state.belief_snapshots.entry(fid).or_default().push(snap);
    }
}

/// Compute the per-tick accuracy contribution for a faction's
/// belief. Returns `(force_count, force_sum_abs_error, region_count,
/// region_sum_correct_fraction)` so the caller can update the run
/// counters in one batch.
///
/// Force-strength error excludes own-force entries (trivially zero)
/// and entries whose `force` no longer exists in ground truth (the
/// believer thinks a destroyed force still exists; counted as
/// max-error = the believed strength itself).
///
/// Region accuracy is the fraction of believed regions whose
/// `controller` matches current ground truth.
fn compute_accuracy_contribution(
    belief: &FactionBelief,
    self_id: &FactionId,
    state: &SimulationState,
) -> (u32, f64, u32, f64) {
    // Force strength error.
    let mut force_n: u32 = 0;
    let mut force_err_sum: f64 = 0.0;
    for bf in belief.forces.values() {
        if &bf.owner == self_id {
            continue;
        }
        let actual = lookup_force(state, &bf.force).map(|f| f.strength);
        let err = match actual {
            Some(a) => (bf.estimated_strength - a).abs(),
            None => bf.estimated_strength.abs(),
        };
        force_err_sum += err;
        force_n = force_n.saturating_add(1);
    }
    // Region accuracy.
    let mut region_n: u32 = 0;
    let mut region_correct: u32 = 0;
    for (rid, br) in &belief.regions {
        let truth = state.region_control.get(rid).cloned().unwrap_or(None);
        if br.controller == truth {
            region_correct = region_correct.saturating_add(1);
        }
        region_n = region_n.saturating_add(1);
    }
    let region_fraction = if region_n > 0 {
        f64::from(region_correct) / f64::from(region_n)
    } else {
        0.0
    };
    (force_n, force_err_sum, region_n, region_fraction)
}

/// Look up a force unit by id across all factions in ground truth.
fn lookup_force<'a>(
    state: &'a SimulationState,
    force_id: &ForceId,
) -> Option<&'a faultline_types::faction::ForceUnit> {
    state
        .faction_states
        .values()
        .find_map(|fs| fs.forces.get(force_id))
}

/// Apply per-tick decay to every belief entry's confidence.
fn decay_belief(belief: &mut FactionBelief, cfg: &BeliefModelConfig) {
    let force_factor = (1.0 - cfg.force_decay_per_tick).max(0.0);
    let region_factor = (1.0 - cfg.region_decay_per_tick).max(0.0);
    let scalar_factor = (1.0 - cfg.scalar_decay_per_tick).max(0.0);
    for f in belief.forces.values_mut() {
        f.confidence *= force_factor;
        f.source = stale_if_not_deceived(f.source);
    }
    for r in belief.regions.values_mut() {
        r.confidence *= region_factor;
        r.source = stale_if_not_deceived(r.source);
    }
    for s in belief.faction_morale.values_mut() {
        s.confidence *= scalar_factor;
        s.source = stale_if_not_deceived(s.source);
    }
    for s in belief.faction_resources.values_mut() {
        s.confidence *= scalar_factor;
        s.source = stale_if_not_deceived(s.source);
    }
}

/// Mark a non-deceived belief as stale after decay. Deception entries
/// keep their `Deceived` tag through aging so the cross-run analytics
/// can distinguish "stale-because-unrefreshed" from "stale-but-still-
/// believed-because-it-was-planted".
fn stale_if_not_deceived(source: BeliefSource) -> BeliefSource {
    match source {
        BeliefSource::DirectObservation | BeliefSource::Stale => BeliefSource::Stale,
        BeliefSource::Deceived => BeliefSource::Deceived,
        BeliefSource::Inferred => BeliefSource::Inferred,
    }
}

/// Drop entries whose confidence is strictly below `prune_threshold`.
/// Setting `prune_threshold = 0.0` disables pruning.
fn prune_belief(belief: &mut FactionBelief, prune_threshold: f64) {
    if prune_threshold <= 0.0 {
        return;
    }
    belief.forces.retain(|_, f| f.confidence >= prune_threshold);
    belief
        .regions
        .retain(|_, r| r.confidence >= prune_threshold);
    belief
        .faction_morale
        .retain(|_, s| s.confidence >= prune_threshold);
    belief
        .faction_resources
        .retain(|_, s| s.confidence >= prune_threshold);
}

/// Refresh every entry visible to `faction_id` from current ground
/// truth. Direct observation overwrites the entry with
/// `confidence = 1.0` and `source = DirectObservation`, clearing any
/// prior `Deceived` tag.
///
/// Visibility uses the same logic as the legacy `build_world_view`:
/// own controlled regions, regions with own forces, regions adjacent
/// to own forces, and Recon-extended visibility hops.
fn observe_into_belief(
    belief: &mut FactionBelief,
    faction_id: &FactionId,
    state: &SimulationState,
    // TODO(round-two): consume scenario for intelligence-stat-driven
    // estimation noise — pre-wired so the call sites don't need to
    // change when round-two lands.
    _scenario: &Scenario,
    map: &GameMap,
    tick: u32,
) {
    let Some(self_state) = state.faction_states.get(faction_id) else {
        return;
    };
    let visible = compute_visible_regions(self_state, map);

    // Refresh own faction's morale + resources (always observable to
    // self; truth at confidence 1.0).
    belief.faction_morale.insert(
        faction_id.clone(),
        BeliefScalar::fresh(self_state.morale, tick),
    );
    belief.faction_resources.insert(
        faction_id.clone(),
        BeliefScalar::fresh(self_state.resources, tick),
    );

    // Refresh visible regions (control attribution).
    for rid in &visible {
        let controller = state.region_control.get(rid).cloned().unwrap_or(None);
        belief.regions.insert(
            rid.clone(),
            BeliefRegion {
                controller,
                confidence: 1.0,
                last_observed_tick: tick,
                source: BeliefSource::DirectObservation,
            },
        );
    }

    // Refresh visible foreign forces. Visibility-filtered: a force in a
    // non-visible region keeps any prior belief entry (with decayed
    // confidence), it does not get refreshed. Force estimation
    // confidence is full 1.0 — round-one models direct observation as
    // perfectly accurate; round-two will introduce an intelligence-
    // dependent estimation noise.
    for (fid, fs) in &state.faction_states {
        for (force_id, force) in &fs.forces {
            let is_self = fid == faction_id;
            let visible_now = visible.contains(&force.region);
            if !visible_now && !is_self {
                continue;
            }
            belief.forces.insert(
                force_id.clone(),
                BeliefForce {
                    force: force_id.clone(),
                    owner: fid.clone(),
                    region: force.region.clone(),
                    estimated_strength: force.strength,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::DirectObservation,
                },
            );
        }
    }
}

/// Apply a `DeceptionOp` event-effect to the target faction's belief.
///
/// Inserts (or overwrites) a single entry tagged
/// [`BeliefSource::Deceived`] at `confidence = 1.0`. The believer
/// cannot tell from inside the simulation that the entry is false —
/// AI consumption sees a normal belief at full confidence. The
/// `Deceived` tag is retained through subsequent decay (see
/// `stale_if_not_deceived`) but is *cleared* if the believer ever
/// directly observes the truth in a later tick (the believer's eyes
/// trump the planted intel). The cross-run analytics in
/// `faultline_stats::belief` count "deceptions still active at run
/// end" by inspecting `BeliefSource::Deceived` on the terminal
/// belief state.
///
/// No-op when belief mode is disabled — the deception event was
/// authored but the model isn't running, so the planted belief has
/// nowhere to land. Validation rejects unknown faction / force /
/// region references at scenario load.
pub fn apply_deception_op(
    state: &mut SimulationState,
    scenario: &Scenario,
    target_faction: &FactionId,
    payload: &DeceptionPayload,
) {
    if !belief_enabled(scenario) {
        return;
    }
    let tick = state.tick;
    let belief = state
        .belief_states
        .entry(target_faction.clone())
        .or_insert_with(|| FactionBelief {
            faction: target_faction.clone(),
            ..Default::default()
        });
    match payload {
        DeceptionPayload::FalseForceStrength {
            force,
            owner,
            region,
            false_strength,
        } => {
            belief.forces.insert(
                force.clone(),
                BeliefForce {
                    force: force.clone(),
                    owner: owner.clone(),
                    region: region.clone(),
                    estimated_strength: *false_strength,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::Deceived,
                },
            );
        },
        DeceptionPayload::FalseRegionControl {
            region,
            false_controller,
        } => {
            belief.regions.insert(
                region.clone(),
                BeliefRegion {
                    controller: false_controller.clone(),
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::Deceived,
                },
            );
        },
        DeceptionPayload::FalseFactionMorale {
            faction,
            false_morale,
        } => {
            belief.faction_morale.insert(
                faction.clone(),
                BeliefScalar {
                    value: *false_morale,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::Deceived,
                },
            );
        },
        DeceptionPayload::FalseFactionResources {
            faction,
            false_resources,
        } => {
            belief.faction_resources.insert(
                faction.clone(),
                BeliefScalar {
                    value: *false_resources,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::Deceived,
                },
            );
        },
    }
    belief.deception_events_received = belief.deception_events_received.saturating_add(1);
    let counter = state
        .belief_counters
        .entry(target_faction.clone())
        .or_default();
    counter.deception_events_received = counter.deception_events_received.saturating_add(1);
}

/// Apply an `IntelligenceShare` event-effect — overwrite the target's
/// belief about the referenced entity with the *current ground truth*
/// at full confidence. Different from `DeceptionOp` only in that the
/// resulting source tag is `DirectObservation`, so subsequent
/// observation refreshes the entry as if the believer had seen it
/// directly.
pub fn apply_intelligence_share(
    state: &mut SimulationState,
    scenario: &Scenario,
    target_faction: &FactionId,
    payload: &IntelligencePayload,
) {
    if !belief_enabled(scenario) {
        return;
    }
    let tick = state.tick;
    // First, derive the ground-truth value(s) before mutably borrowing
    // belief state.
    let ground_truth =
        match payload {
            IntelligencePayload::ForceObservation { force } => {
                let force_id = force.clone();
                lookup_force(state, &force_id).map(|f| {
                    let owner = state
                        .faction_states
                        .iter()
                        .find_map(|(fid, fs)| {
                            if fs.forces.contains_key(&force_id) {
                                Some(fid.clone())
                            } else {
                                None
                            }
                        })
                        .expect("force found by lookup_force must have an owner");
                    IntelGroundTruth::Force {
                        force: force_id,
                        owner,
                        region: f.region.clone(),
                        strength: f.strength,
                    }
                })
            },
            IntelligencePayload::RegionControl { region } => Some(IntelGroundTruth::Region {
                region: region.clone(),
                controller: state.region_control.get(region).cloned().unwrap_or(None),
            }),
            IntelligencePayload::FactionMorale { faction } => state
                .faction_states
                .get(faction)
                .map(|fs| IntelGroundTruth::Morale {
                    faction: faction.clone(),
                    value: fs.morale,
                }),
            IntelligencePayload::FactionResources { faction } => state
                .faction_states
                .get(faction)
                .map(|fs| IntelGroundTruth::Resources {
                    faction: faction.clone(),
                    value: fs.resources,
                }),
        };
    let Some(truth) = ground_truth else {
        return;
    };

    let belief = state
        .belief_states
        .entry(target_faction.clone())
        .or_insert_with(|| FactionBelief {
            faction: target_faction.clone(),
            ..Default::default()
        });
    match truth {
        IntelGroundTruth::Force {
            force,
            owner,
            region,
            strength,
        } => {
            belief.forces.insert(
                force.clone(),
                BeliefForce {
                    force,
                    owner,
                    region,
                    estimated_strength: strength,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::DirectObservation,
                },
            );
        },
        IntelGroundTruth::Region { region, controller } => {
            belief.regions.insert(
                region,
                BeliefRegion {
                    controller,
                    confidence: 1.0,
                    last_observed_tick: tick,
                    source: BeliefSource::DirectObservation,
                },
            );
        },
        IntelGroundTruth::Morale { faction, value } => {
            belief
                .faction_morale
                .insert(faction, BeliefScalar::fresh(value, tick));
        },
        IntelGroundTruth::Resources { faction, value } => {
            belief
                .faction_resources
                .insert(faction, BeliefScalar::fresh(value, tick));
        },
    }
    let counter = state
        .belief_counters
        .entry(target_faction.clone())
        .or_default();
    counter.intel_shares_received = counter.intel_shares_received.saturating_add(1);
}

/// Construct a [`FactionWorldView`] from a persistent
/// [`FactionBelief`] so the AI's existing fog-of-war evaluator can
/// consume beliefs as if they were direct observations.
///
/// This is the integration point that turns belief asymmetry into
/// behavioral effect: the AI's [`crate::ai::evaluate_actions_fog`]
/// reads opponent strength and region control from the world view,
/// and the world view we hand it now reflects belief — including
/// any deception entries that have not been refreshed by direct
/// observation.
///
/// Round-one mapping is direct: each [`BeliefForce`] becomes a
/// [`DetectedForce`] with the believed strength and the belief's
/// own confidence; each [`BeliefRegion`] populates `known_regions`
/// with the believed controller. Self-faction morale / resources
/// come from the believer's own belief entries (which always equal
/// ground truth at confidence 1.0 since the believer always
/// observes themselves directly).
pub fn world_view_from_belief(
    belief: &FactionBelief,
    state: &SimulationState,
    tick: u32,
) -> FactionWorldView {
    let known_regions: BTreeMap<RegionId, Option<FactionId>> = belief
        .regions
        .iter()
        .map(|(rid, br)| (rid.clone(), br.controller.clone()))
        .collect();

    let detected_forces: Vec<DetectedForce> = belief
        .forces
        .iter()
        .filter(|(_, bf)| bf.owner != belief.faction)
        .map(|(_, bf)| DetectedForce {
            force_id: bf.force.clone(),
            faction: bf.owner.clone(),
            region: bf.region.clone(),
            estimated_strength: bf.estimated_strength,
            confidence: bf.confidence,
        })
        .collect();

    // Self-knowledge: own morale / resources read from the believer's
    // entry for itself, which `observe_into_belief` keeps refreshed
    // at confidence 1.0 every tick. Fall back to ground truth if the
    // entry is missing (round-one defensive only — the belief phase
    // always populates it after tick 0).
    let self_morale = belief
        .faction_morale
        .get(&belief.faction)
        .map(|s| s.value)
        .or_else(|| {
            state
                .faction_states
                .get(&belief.faction)
                .map(|fs| fs.morale)
        })
        .unwrap_or(0.5);
    let self_resources = belief
        .faction_resources
        .get(&belief.faction)
        .map(|s| s.value)
        .or_else(|| {
            state
                .faction_states
                .get(&belief.faction)
                .map(|fs| fs.resources)
        })
        .unwrap_or(0.0);

    FactionWorldView {
        faction: belief.faction.clone(),
        known_regions,
        detected_forces,
        infra_states: BTreeMap::new(),
        political_climate: PoliticalClimateView {
            tension: state.political_climate.tension,
            institutional_trust: state.political_climate.institutional_trust,
            civilian_sentiment: 0.0,
        },
        diplomacy: BTreeMap::new(),
        morale: self_morale,
        resources: self_resources,
        tick,
    }
}

/// Internal helper enum for `apply_intelligence_share` so the truth
/// lookup and the belief mutation can be split into two passes
/// without conflicting borrows.
enum IntelGroundTruth {
    Force {
        force: ForceId,
        owner: FactionId,
        region: RegionId,
        strength: f64,
    },
    Region {
        region: RegionId,
        controller: Option<FactionId>,
    },
    Morale {
        faction: FactionId,
        value: f64,
    },
    Resources {
        faction: FactionId,
        value: f64,
    },
}

/// Build a [`BeliefSnapshot`] from the live belief state.
fn summarize_belief(belief: &FactionBelief, tick: u32) -> BeliefSnapshot {
    let force_count = u32::try_from(belief.forces.len()).unwrap_or(u32::MAX);
    let region_count = u32::try_from(belief.regions.len()).unwrap_or(u32::MAX);
    let mean_force_conf = if belief.forces.is_empty() {
        0.0
    } else {
        belief.forces.values().map(|f| f.confidence).sum::<f64>() / belief.forces.len() as f64
    };
    let mean_region_conf = if belief.regions.is_empty() {
        0.0
    } else {
        belief.regions.values().map(|r| r.confidence).sum::<f64>() / belief.regions.len() as f64
    };
    let deceived_force_count = u32::try_from(
        belief
            .forces
            .values()
            .filter(|f| matches!(f.source, BeliefSource::Deceived))
            .count(),
    )
    .unwrap_or(u32::MAX);
    BeliefSnapshot {
        tick,
        force_belief_count: force_count,
        region_belief_count: region_count,
        mean_force_confidence: mean_force_conf,
        mean_region_confidence: mean_region_conf,
        deceived_force_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::belief::BeliefModelConfig;

    #[test]
    fn decay_clamps_at_zero() {
        let cfg = BeliefModelConfig {
            force_decay_per_tick: 1.5, // would go negative if unclamped
            ..Default::default()
        };
        let mut belief = FactionBelief::default();
        belief.forces.insert(
            ForceId::from("f1"),
            BeliefForce {
                force: ForceId::from("f1"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 100.0,
                confidence: 0.5,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );
        decay_belief(&mut belief, &cfg);
        let f = belief.forces.get(&ForceId::from("f1")).expect("entry");
        assert!(f.confidence >= 0.0, "confidence floored at 0");
    }

    #[test]
    fn deception_is_sticky_through_decay() {
        let cfg = BeliefModelConfig::default();
        let mut belief = FactionBelief::default();
        belief.forces.insert(
            ForceId::from("f1"),
            BeliefForce {
                force: ForceId::from("f1"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 100.0,
                confidence: 1.0,
                last_observed_tick: 0,
                source: BeliefSource::Deceived,
            },
        );
        decay_belief(&mut belief, &cfg);
        let f = belief.forces.get(&ForceId::from("f1")).expect("entry");
        assert_eq!(f.source, BeliefSource::Deceived);
    }

    #[test]
    fn direct_observation_decays_to_stale_tag() {
        let cfg = BeliefModelConfig::default();
        let mut belief = FactionBelief::default();
        belief.regions.insert(
            RegionId::from("r1"),
            BeliefRegion {
                controller: None,
                confidence: 1.0,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );
        decay_belief(&mut belief, &cfg);
        let r = belief.regions.get(&RegionId::from("r1")).expect("entry");
        assert_eq!(r.source, BeliefSource::Stale);
    }

    #[test]
    fn prune_drops_below_threshold() {
        let mut belief = FactionBelief::default();
        belief.forces.insert(
            ForceId::from("f_low"),
            BeliefForce {
                force: ForceId::from("f_low"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 50.0,
                confidence: 0.01,
                last_observed_tick: 0,
                source: BeliefSource::Stale,
            },
        );
        belief.forces.insert(
            ForceId::from("f_hi"),
            BeliefForce {
                force: ForceId::from("f_hi"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 50.0,
                confidence: 0.5,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );
        prune_belief(&mut belief, 0.05);
        assert!(!belief.forces.contains_key(&ForceId::from("f_low")));
        assert!(belief.forces.contains_key(&ForceId::from("f_hi")));
    }

    #[test]
    fn prune_zero_threshold_keeps_everything() {
        let mut belief = FactionBelief::default();
        belief.forces.insert(
            ForceId::from("f"),
            BeliefForce {
                force: ForceId::from("f"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 0.1,
                confidence: 0.0,
                last_observed_tick: 0,
                source: BeliefSource::Stale,
            },
        );
        prune_belief(&mut belief, 0.0);
        assert!(belief.forces.contains_key(&ForceId::from("f")));
    }

    #[test]
    fn summarize_empty() {
        let belief = FactionBelief::default();
        let snap = summarize_belief(&belief, 5);
        assert_eq!(snap.tick, 5);
        assert_eq!(snap.force_belief_count, 0);
        assert_eq!(snap.region_belief_count, 0);
        assert_eq!(snap.mean_force_confidence, 0.0);
    }

    /// Pin the contract that `compute_accuracy_contribution` returns
    /// the per-tick region-accuracy as an already-normalized fraction
    /// (`correct / region_n`). Pairs with the matching invariant in
    /// `belief_phase` that adds the value directly to
    /// `region_accuracy_sum` without re-dividing.
    #[test]
    fn region_accuracy_sum_not_double_divided() {
        use crate::state::RuntimeFactionState;
        let red = FactionId::from("red");
        let blue = FactionId::from("blue");
        let r1 = RegionId::from("r1");
        let r2 = RegionId::from("r2");

        let mut belief = FactionBelief::default();
        belief.regions.insert(
            r1.clone(),
            BeliefRegion {
                controller: Some(red.clone()),
                confidence: 1.0,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );
        belief.regions.insert(
            r2.clone(),
            BeliefRegion {
                controller: Some(blue.clone()),
                confidence: 1.0,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );

        let mut faction_states = BTreeMap::new();
        for fid in [red.clone(), blue.clone()] {
            faction_states.insert(
                fid.clone(),
                RuntimeFactionState {
                    faction_id: fid.clone(),
                    total_strength: 0.0,
                    morale: 0.5,
                    resources: 0.0,
                    resource_rate: 0.0,
                    logistics_capacity: 0.0,
                    controlled_regions: vec![],
                    forces: BTreeMap::new(),
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
        }
        let mut region_control = BTreeMap::new();
        region_control.insert(r1, Some(red.clone()));
        region_control.insert(r2, Some(blue.clone()));

        let state = SimulationState {
            tick: 0,
            faction_states,
            region_control,
            infra_status: BTreeMap::new(),
            institution_loyalty: BTreeMap::new(),
            political_climate: faultline_types::politics::PoliticalClimate::default(),
            events_fired: Default::default(),
            events_fired_this_tick: vec![],
            snapshots: vec![],
            non_kinetic: Default::default(),
            metric_history: vec![],
            defender_queues: BTreeMap::new(),
            network_states: BTreeMap::new(),
            defender_over_budget_tick: None,
            diplomacy_overrides: BTreeMap::new(),
            fired_fractures: Default::default(),
            initial_faction_strengths: BTreeMap::new(),
            fracture_events: vec![],
            civilian_activations: vec![],
            narratives: BTreeMap::new(),
            narrative_events: vec![],
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement: BTreeMap::new(),
            utility_decisions: BTreeMap::new(),
            belief_states: BTreeMap::new(),
            belief_counters: BTreeMap::new(),
            belief_snapshots: BTreeMap::new(),
        };

        let observer = FactionId::from("observer");
        let (_, _, region_n, region_sum) =
            compute_accuracy_contribution(&belief, &observer, &state);
        assert_eq!(region_n, 2);
        // 2 regions, 2 correct → fraction is 1.0 (not 0.5, which would
        // indicate a double-division regression).
        assert!(
            (region_sum - 1.0).abs() < 1e-9,
            "region_sum={region_sum} expected 1.0"
        );
    }

    #[test]
    fn summarize_with_entries() {
        let mut belief = FactionBelief::default();
        belief.forces.insert(
            ForceId::from("f1"),
            BeliefForce {
                force: ForceId::from("f1"),
                owner: FactionId::from("red"),
                region: RegionId::from("r1"),
                estimated_strength: 100.0,
                confidence: 0.8,
                last_observed_tick: 0,
                source: BeliefSource::Deceived,
            },
        );
        belief.forces.insert(
            ForceId::from("f2"),
            BeliefForce {
                force: ForceId::from("f2"),
                owner: FactionId::from("red"),
                region: RegionId::from("r2"),
                estimated_strength: 50.0,
                confidence: 1.0,
                last_observed_tick: 0,
                source: BeliefSource::DirectObservation,
            },
        );
        let snap = summarize_belief(&belief, 3);
        assert_eq!(snap.force_belief_count, 2);
        assert!((snap.mean_force_confidence - 0.9).abs() < 1e-9);
        assert_eq!(snap.deceived_force_count, 1);
    }
}
