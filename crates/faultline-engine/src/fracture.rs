//! Alliance-fracture phase (Epic D round two).
//!
//! Evaluates each faction's declared `[factions.<id>.alliance_fracture]`
//! rules at end-of-tick after the campaign phase, and when a rule's
//! condition is satisfied:
//!
//! 1. Records the firing on `SimulationState.fracture_events`.
//! 2. Mutates `SimulationState.diplomacy_overrides` so subsequent
//!    diplomacy reads see the new stance.
//! 3. Latches the rule into `SimulationState.fired_fractures` so it
//!    won't re-fire later in the run (one-shot semantics).
//!
//! No RNG, no allocations on the hot path for legacy scenarios — the
//! phase short-circuits at the top when no faction declares an
//! `alliance_fracture` block.

use std::collections::BTreeMap;

use faultline_types::faction::{Diplomacy, FractureCondition, FractureRule};
use faultline_types::ids::{FactionId, KillChainId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::FractureEvent;

use crate::campaign::CampaignState;
use crate::state::SimulationState;

/// Run the fracture phase. Idempotent for ticks where no rule fires;
/// no-op for scenarios with no rules at all.
///
/// The order of evaluation is deterministic: factions are visited in
/// `BTreeMap` key order, and within each faction the rules are visited
/// in their authored vector order. A rule that fires earlier in this
/// order can affect a later rule's condition only via
/// `previous_stance` reads on `EventFired` — every other condition
/// reads runtime metrics (morale, tension, attribution, strength) that
/// the fracture phase does not mutate.
pub fn fracture_phase(
    state: &mut SimulationState,
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
) {
    // Cheap precheck: skip the whole phase when no faction has rules.
    // Iterating an empty BTreeMap is fast but the inner short-circuit
    // makes the legacy hot path entirely free.
    if !scenario
        .factions
        .values()
        .any(|f| f.alliance_fracture.is_some())
    {
        return;
    }

    // Walk a snapshot of (faction_id, rules) so we can mutate `state`
    // freely inside the loop without conflicting borrows. Cloning the
    // rule vector is acceptable — fracture rules are author-bounded,
    // typically <10 per faction.
    let faction_rules: Vec<(FactionId, Vec<FractureRule>)> = scenario
        .factions
        .iter()
        .filter_map(|(fid, f)| {
            f.alliance_fracture
                .as_ref()
                .map(|af| (fid.clone(), af.rules.clone()))
        })
        .collect();

    let current_tick = state.tick;
    for (faction_id, rules) in faction_rules {
        for rule in rules {
            let key = (faction_id.clone(), rule.id.clone());
            if state.fired_fractures.contains(&key) {
                continue;
            }
            if !condition_satisfied(state, scenario, campaigns, &faction_id, &rule.condition) {
                continue;
            }

            // Capture the live previous stance (overrides take
            // precedence over the scenario baseline) so the report
            // records each fracture's actual transition.
            let previous_stance = current_stance(state, scenario, &faction_id, &rule.counterparty);

            state
                .diplomacy_overrides
                .entry(faction_id.clone())
                .or_default()
                .insert(rule.counterparty.clone(), rule.new_stance);
            state.fired_fractures.insert(key);
            state.fracture_events.push(FractureEvent {
                tick: current_tick,
                faction: faction_id.clone(),
                counterparty: rule.counterparty.clone(),
                rule_id: rule.id.clone(),
                previous_stance,
                new_stance: rule.new_stance,
            });
        }
    }
}

/// Resolve `(source -> target)` stance, preferring runtime overrides
/// over the scenario-authored `Faction.diplomacy` table. Falls back to
/// `Diplomacy::Neutral` when no relationship is declared — same default
/// the scenario schema implies for any unlisted pair.
pub fn current_stance(
    state: &SimulationState,
    scenario: &Scenario,
    source: &FactionId,
    target: &FactionId,
) -> Diplomacy {
    if let Some(overrides) = state.diplomacy_overrides.get(source)
        && let Some(stance) = overrides.get(target)
    {
        return *stance;
    }
    baseline_stance(scenario, source, target)
}

/// Resolve `(source -> target)` stance from the scenario's authored
/// `Faction.diplomacy` table, ignoring runtime overrides. Useful for
/// post-run analytics that have no `SimulationState` in scope (e.g. the
/// cross-run rollup in `faultline_stats::alliance_dynamics`). Same
/// fallback semantics as `current_stance`: unlisted pairs read as
/// `Diplomacy::Neutral`.
pub fn baseline_stance(scenario: &Scenario, source: &FactionId, target: &FactionId) -> Diplomacy {
    if let Some(faction) = scenario.factions.get(source) {
        for entry in &faction.diplomacy {
            if entry.target_faction == *target {
                return entry.stance;
            }
        }
    }
    Diplomacy::Neutral
}

fn condition_satisfied(
    state: &SimulationState,
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
    faction_id: &FactionId,
    cond: &FractureCondition,
) -> bool {
    match cond {
        FractureCondition::AttributionThreshold {
            attacker,
            threshold,
        } => mean_attribution(scenario, campaigns, attacker) >= *threshold,
        FractureCondition::MoraleFloor { floor } => state
            .faction_states
            .get(faction_id)
            .is_some_and(|fs| fs.morale <= *floor),
        FractureCondition::TensionThreshold { threshold } => {
            state.political_climate.tension >= *threshold
        },
        FractureCondition::EventFired { event } => state.events_fired.contains(event),
        FractureCondition::StrengthLossFraction { delta_fraction } => {
            // Read the captured initial strength so we don't need a
            // running history. A faction that started at zero
            // strength has an undefined loss ratio — treat as
            // "never satisfied" rather than dividing by zero.
            let Some(initial) = state.initial_faction_strengths.get(faction_id) else {
                return false;
            };
            if *initial <= 0.0 {
                return false;
            }
            let Some(fs) = state.faction_states.get(faction_id) else {
                return false;
            };
            let lost = (*initial - fs.total_strength).max(0.0);
            (lost / *initial) >= *delta_fraction
        },
    }
}

/// Mean per-chain attribution confidence over chains owned by
/// `attacker`. Returns 0.0 when no such chain is in flight (no
/// signal yet, can't fire the rule). Iteration is `BTreeMap`-ordered
/// for determinism even though the result is order-independent.
fn mean_attribution(
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
    attacker: &FactionId,
) -> f64 {
    let mut sum = 0.0f64;
    let mut count = 0u32;
    for (cid, chain) in &scenario.kill_chains {
        if chain.attacker != *attacker {
            continue;
        }
        if let Some(cstate) = campaigns.get(cid) {
            sum += cstate.attribution_confidence;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / f64::from(count)
    }
}
