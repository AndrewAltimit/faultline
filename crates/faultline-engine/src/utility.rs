//! Multi-term utility evaluator for adaptive AI scoring (Epic J round-one).
//!
//! Pure post-processor on top of the existing doctrine-based AI scoring
//! in [`crate::ai`]. Faction declares a `[utility]` block; the evaluator
//! computes a per-action expected utility delta and adds it to the
//! doctrine score before action selection. Composition is additive so
//! the two signals coexist — a faction without `[utility]` is unaffected.
//!
//! Determinism: every public function is a pure function of
//! `(state, scenario, campaigns, action)` — no RNG, no `HashMap`,
//! `BTreeMap`-ordered iteration. Adding a `[utility]` block to a
//! scenario *will* change the affected scenario's combat schedule and
//! downstream observable outputs (the score re-ranking shifts which
//! action wins), but determinism for any fixed seed holds.
//!
//! Per-action mapping (the round-one heuristic):
//!
//! | Action | Control | CasualtiesSelf | CasualtiesInflicted | AttributionRisk | TimeToObjective | ResourceCost | ForceConcentration |
//! |---|---|---|---|---|---|---|---|
//! | `Attack` | + region.strategic_value × p_capture | − own_strength × p_loss | + opp_strength × damage_factor | 0 (overt) | + (progress) | 0 | 0 |
//! | `Defend` | + retain_value | − projected_loss × shield | 0 | 0 | 0 | 0 | 0 |
//! | `MoveUnit` | + region.strategic_value × 0.5 (unclaimed) | 0 | 0 | 0 | + 0.5 (progress) | 0 | + if friendly forces present |
//! | `Recruit` | 0 | + base_strength (negative-weight = avoiding casualties means we *want* fresh units) | 0 | 0 | 0 | − recruit_cost | 0 |
//!
//! The numbers in each cell are intentionally bounded in `[0, 1]` so
//! term weights sized in `[0, 5]` cover the natural authoring range.
//! See the bundled `scenarios/adaptive_utility_demo.toml` for an
//! end-to-end walkthrough.

use std::collections::BTreeMap;

use faultline_geo::{GameMap, adjacent_regions};
use faultline_types::faction::{AdaptiveCondition, AdaptiveTrigger, FactionUtility, UtilityTerm};
use faultline_types::ids::{FactionId, KillChainId};
use faultline_types::scenario::Scenario;
use faultline_types::strategy::FactionAction;

use crate::campaign::CampaignState;
use crate::state::SimulationState;

/// Per-action utility score with a contribution breakdown.
///
/// `total` is what the AI adds to the doctrine score. `contributions`
/// is the per-term decomposition — used by the report's per-faction
/// utility decomposition section to surface which axis drove which
/// decisions across the run. Empty `contributions` is a legitimate
/// signal: the action either touched no terms the faction cared
/// about, or every touched term had zero effective weight.
#[derive(Clone, Debug, Default)]
pub struct UtilityScore {
    pub total: f64,
    pub contributions: BTreeMap<UtilityTerm, f64>,
}

/// Snapshot of effective utility weights for one faction at one tick.
///
/// Built once per (faction, tick) by [`effective_weights`] from the
/// declared base weights and any matched adaptive triggers. Cached
/// across one decision-phase pass so the AI doesn't re-evaluate
/// triggers per candidate action.
#[derive(Clone, Debug, Default)]
pub struct EffectiveWeights {
    pub weights: BTreeMap<UtilityTerm, f64>,
    /// IDs of triggers that fired this phase, in declaration order.
    /// Empty when no trigger matched.
    pub fired_triggers: Vec<String>,
}

/// Compute the effective utility weights for a faction this phase.
///
/// Each declared trigger is evaluated against current state. Matched
/// triggers compose multiplicatively in declaration order: a trigger
/// that doubles `CasualtiesSelf` followed by another that halves it
/// lands at the original weight. Pure function of state + scenario.
pub fn effective_weights(
    profile: &FactionUtility,
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
) -> EffectiveWeights {
    // Start from the declared base weights. Missing terms default to 0
    // — no contribution from that axis.
    let mut weights: BTreeMap<UtilityTerm, f64> = profile.terms.clone();
    let mut fired_triggers = Vec::new();
    for trigger in &profile.triggers {
        if !condition_holds(
            &trigger.condition,
            faction_id,
            state,
            scenario,
            campaigns,
            profile,
        ) {
            continue;
        }
        fired_triggers.push(trigger.id.clone());
        for (term, multiplier) in &trigger.adjustments {
            // Multiply against current weight (which may already have
            // been adjusted by a prior trigger in this phase). A term
            // not in `weights` defaults to 0; the product of 0 and
            // anything is 0, which is the right semantics for
            // "trigger fires but base weight is zero, so the
            // adjustment can't reach in".
            let entry = weights.entry(*term).or_insert(0.0);
            *entry *= multiplier;
        }
    }
    EffectiveWeights {
        weights,
        fired_triggers,
    }
}

/// Whether `cond` holds for `faction_id` at the current tick.
///
/// Pure function — no RNG, no allocation in the hot path.
fn condition_holds(
    cond: &AdaptiveCondition,
    faction_id: &FactionId,
    state: &SimulationState,
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
    profile: &FactionUtility,
) -> bool {
    let Some(fs) = state.faction_states.get(faction_id) else {
        // Faction not in state — degenerate. Triggers can't fire.
        return false;
    };
    match cond {
        AdaptiveCondition::MoraleBelow { threshold } => {
            crate::tick::effective_combat_morale(fs) <= *threshold
        },
        AdaptiveCondition::MoraleAbove { threshold } => {
            crate::tick::effective_combat_morale(fs) >= *threshold
        },
        AdaptiveCondition::TensionAbove { threshold } => {
            state.political_climate.tension >= *threshold
        },
        AdaptiveCondition::TickFraction { fraction } => {
            // Time horizon: faction-overridden when set, else the
            // scenario's max_ticks. The `max_ticks` floor of `1` guards
            // against the degenerate scenario with no ticks at all.
            let horizon = profile
                .time_horizon_ticks
                .unwrap_or(scenario.simulation.max_ticks)
                .max(1);
            let frac = f64::from(state.tick) / f64::from(horizon);
            frac >= *fraction
        },
        AdaptiveCondition::ResourcesBelow { threshold } => fs.resources <= *threshold,
        AdaptiveCondition::StrengthLossFraction { fraction } => {
            let initial = state
                .initial_faction_strengths
                .get(faction_id)
                .copied()
                .unwrap_or(0.0);
            if initial <= 0.0 {
                return false;
            }
            let lost = (initial - fs.total_strength).max(0.0);
            (lost / initial) >= *fraction
        },
        AdaptiveCondition::AttributionAgainstSelf { threshold } => {
            mean_attribution_against(scenario, campaigns, faction_id) >= *threshold
        },
    }
}

/// Mean per-chain attribution confidence over kill chains where
/// `faction` is the attacker. `0.0` when no such chain is in flight
/// (no signal yet) — the trigger can't fire on a faction without a
/// kill chain in the first place. Mirrors the helper in
/// `crate::fracture` so the two phases agree on the attribution
/// definition.
pub(crate) fn mean_attribution_against(
    scenario: &Scenario,
    campaigns: &BTreeMap<KillChainId, CampaignState>,
    faction: &FactionId,
) -> f64 {
    let mut sum = 0.0f64;
    let mut count = 0u32;
    for (cid, chain) in &scenario.kill_chains {
        if chain.attacker != *faction {
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

/// Score a single candidate action against the effective weights.
///
/// Implements the per-action term mapping documented at the top of
/// the module. Contributions are bounded in `[-w, w]` per term so
/// the total stays interpretable when the analyst sees it in the
/// report.
pub fn evaluate_action_utility(
    weights: &EffectiveWeights,
    faction_id: &FactionId,
    action: &FactionAction,
    state: &SimulationState,
    _scenario: &Scenario,
    map: &GameMap,
) -> UtilityScore {
    let Some(fs) = state.faction_states.get(faction_id) else {
        return UtilityScore::default();
    };
    let mut score = UtilityScore::default();

    let read_w = |t: UtilityTerm| weights.weights.get(&t).copied().unwrap_or(0.0);

    match action {
        FactionAction::Attack {
            force,
            target_region,
        } => {
            let strategic_value = map
                .regions
                .get(target_region)
                .map_or(0.0, |r| r.strategic_value);
            // Estimate p_capture from own-vs-enemy strength ratio at the
            // target region. A pure heuristic — combat resolution has
            // far more inputs (terrain, doctrine, leadership, supply,
            // tech), but the AI doesn't simulate combat to score; it
            // approximates. Sized so a faction with 2× attacker
            // strength scores ~0.67 capture probability.
            let own_strength = fs.forces.get(force).map_or(1.0, |f| f.strength);
            let opp_strength = enemy_strength_in_region(state, faction_id, target_region);
            let p_capture = if own_strength + opp_strength <= f64::EPSILON {
                0.5
            } else {
                own_strength / (own_strength + opp_strength)
            };
            let control_delta = strategic_value * p_capture;
            add_contribution(
                &mut score,
                UtilityTerm::Control,
                control_delta * read_w(UtilityTerm::Control),
            );

            // Casualties_self: positive weight = avoid losses, but
            // this is a *cost* on attack (we project losing some
            // strength). Sign convention: contribution is negative
            // when the action loses us strength; the analyst writes
            // a *positive* weight on `CasualtiesSelf` to express
            // "I want to minimize self-casualties", which then
            // multiplies the negative contribution into a negative
            // score, biasing away from attack.
            let projected_self_loss = own_strength * (1.0 - p_capture) * 0.3;
            let cs_contribution = -projected_self_loss * 0.01;
            add_contribution(
                &mut score,
                UtilityTerm::CasualtiesSelf,
                cs_contribution * read_w(UtilityTerm::CasualtiesSelf),
            );

            let projected_inflicted = opp_strength * p_capture * 0.3;
            let ci_contribution = projected_inflicted * 0.01;
            add_contribution(
                &mut score,
                UtilityTerm::CasualtiesInflicted,
                ci_contribution * read_w(UtilityTerm::CasualtiesInflicted),
            );

            // TimeToObjective: attacking high-strategic-value regions
            // makes progress. Sign convention mirrors CasualtiesSelf —
            // a positive weight expresses urgency.
            add_contribution(
                &mut score,
                UtilityTerm::TimeToObjective,
                strategic_value * read_w(UtilityTerm::TimeToObjective),
            );
        },
        FactionAction::Defend { force, region } => {
            let strategic_value = map.regions.get(region).map_or(0.0, |r| r.strategic_value);
            // Defending a region we control retains its value. The
            // contribution is smaller than Attack's because Defend
            // doesn't *gain* control, just preserves it; but if no
            // enemy threatens the region the contribution should
            // compress to zero (defending an unthreatened region is
            // wasted attention).
            let threat = enemy_strength_in_adjacent(state, faction_id, region, map);
            let own_strength = fs.forces.get(force).map_or(1.0, |f| f.strength);
            let normalized_threat = if threat <= f64::EPSILON {
                0.0
            } else {
                (threat / (own_strength + threat)).min(1.0)
            };
            // Control retention: scales with how much the strategic
            // value is *at risk*.
            add_contribution(
                &mut score,
                UtilityTerm::Control,
                strategic_value * normalized_threat * read_w(UtilityTerm::Control),
            );
            // Defend reduces projected self-loss — positive
            // contribution to CasualtiesSelf when the threat is real.
            let avoided_loss = own_strength * normalized_threat * 0.2;
            add_contribution(
                &mut score,
                UtilityTerm::CasualtiesSelf,
                avoided_loss * 0.01 * read_w(UtilityTerm::CasualtiesSelf),
            );
        },
        FactionAction::MoveUnit { force, destination } => {
            let strategic_value = map
                .regions
                .get(destination)
                .map_or(0.0, |r| r.strategic_value);
            // Move into unclaimed territory captures half the strategic
            // value (vs. Attack's full value × p_capture). Move into
            // friendly territory contributes nothing — that's
            // re-positioning, not progress.
            let controller = state
                .region_control
                .get(destination)
                .and_then(|c| c.as_ref());
            let unclaimed = controller.is_none();
            let is_ours = controller == Some(faction_id);
            let control_factor = if unclaimed {
                strategic_value * 0.5
            } else if is_ours {
                0.0
            } else {
                // Moving into enemy territory makes no control gain
                // by Move (Attack handles that path); the action is
                // effectively a no-op at the engine level.
                0.0
            };
            add_contribution(
                &mut score,
                UtilityTerm::Control,
                control_factor * read_w(UtilityTerm::Control),
            );
            add_contribution(
                &mut score,
                UtilityTerm::TimeToObjective,
                strategic_value * 0.5 * read_w(UtilityTerm::TimeToObjective),
            );
            // ForceConcentration: moving into a region where this
            // faction already has forces is consolidation; into empty
            // territory it's dispersion. The friendly-region case
            // contributes positively scaled by how many *other*
            // friendly forces are already in the destination — so a
            // unit joining a single sibling adds 1, joining a stack
            // of three adds 3.
            let friendly_count = friendly_forces_in_region(state, faction_id, destination, force);
            let concentration_delta = (friendly_count as f64) * 0.25;
            add_contribution(
                &mut score,
                UtilityTerm::ForceConcentration,
                concentration_delta * read_w(UtilityTerm::ForceConcentration),
            );
        },
        FactionAction::Recruit { region: _ } => {
            // Recruit costs resources but adds future strength. The
            // CasualtiesSelf term reads "fresh units" as a positive
            // contribution — a faction that wants to minimize its
            // own losses *should* prefer fresh recruits to taking
            // damage on the field.
            let fresh_units_delta = 0.5;
            add_contribution(
                &mut score,
                UtilityTerm::CasualtiesSelf,
                fresh_units_delta * read_w(UtilityTerm::CasualtiesSelf),
            );
            // ResourceCost: recruit charges resources. Sign is
            // negative — the faction loses resources. A positive
            // weight on ResourceCost (frugality) multiplies into a
            // negative score, biasing away.
            let resource_delta = -1.0;
            add_contribution(
                &mut score,
                UtilityTerm::ResourceCost,
                resource_delta * read_w(UtilityTerm::ResourceCost),
            );
        },
        FactionAction::DeployTech { .. }
        | FactionAction::DiplomacyProposal { .. }
        | FactionAction::Sabotage { .. }
        | FactionAction::InfoOp { .. } => {
            // Round one only scores the four actions the AI actually
            // emits. The other variants exist on the action enum for
            // future epics (J round-two for DiplomacyProposal as a
            // belief-aware action; M for the deception-driven InfoOp
            // path). The utility here compresses to zero, which is
            // the no-op identity for the additive score composition.
        },
    }

    score
}

/// Add a contribution to the score, skipping zero entries so the
/// post-run decomposition is dense — unused terms shouldn't show up
/// in the report. Updates `total` and the per-term contribution map.
fn add_contribution(score: &mut UtilityScore, term: UtilityTerm, value: f64) {
    if value == 0.0 || !value.is_finite() {
        return;
    }
    score.total += value;
    *score.contributions.entry(term).or_insert(0.0) += value;
}

/// Sum of enemy strength in `region` from `faction_id`'s perspective.
/// Mirrors [`crate::ai::compute_enemy_presence`]'s diplomacy-aware
/// weighting so the utility evaluator reports the same threat the AI
/// sees.
fn enemy_strength_in_region(
    state: &SimulationState,
    faction_id: &FactionId,
    region: &faultline_types::ids::RegionId,
) -> f64 {
    let mut sum = 0.0;
    for (other_id, other_fs) in &state.faction_states {
        if other_id == faction_id || other_fs.eliminated {
            continue;
        }
        for force in other_fs.forces.values() {
            if force.region != *region {
                continue;
            }
            sum += force.strength;
        }
    }
    sum
}

/// Sum of enemy strength in any region adjacent to `region`. Used by
/// Defend scoring to estimate how much the position is under threat.
fn enemy_strength_in_adjacent(
    state: &SimulationState,
    faction_id: &FactionId,
    region: &faultline_types::ids::RegionId,
    map: &GameMap,
) -> f64 {
    let neighbors = adjacent_regions(region, map);
    let mut sum = 0.0;
    for n in &neighbors {
        sum += enemy_strength_in_region(state, faction_id, n);
    }
    sum
}

/// Number of friendly forces *other than* `excluded_force` already in
/// `region`. The exclusion is so a unit that's already at the
/// destination doesn't credit itself for being there — only the
/// pre-existing siblings count.
fn friendly_forces_in_region(
    state: &SimulationState,
    faction_id: &FactionId,
    region: &faultline_types::ids::RegionId,
    excluded_force: &faultline_types::ids::ForceId,
) -> u32 {
    state
        .faction_states
        .get(faction_id)
        .map(|fs| {
            fs.forces
                .values()
                .filter(|f| f.region == *region && f.id != *excluded_force)
                .count() as u32
        })
        .unwrap_or(0)
}

#[allow(dead_code)] // referenced from tests in this crate's other modules
pub(crate) fn _trigger_marker(_t: &AdaptiveTrigger) {}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::faction::{Faction, FactionType};
    use faultline_types::ids::FactionId;

    /// A scenario fixture built in-line so the utility tests don't
    /// depend on test_support helpers from other modules. Two
    /// factions, four-region square, no kill chains.
    fn minimal_scenario_with_two_factions() -> Scenario {
        let mut scenario = crate::tests::minimal_scenario();
        scenario.factions.insert(
            FactionId::from("alpha"),
            Faction {
                id: FactionId::from("alpha"),
                name: "Alpha".into(),
                faction_type: FactionType::Civilian,
                ..Default::default()
            },
        );
        scenario.factions.insert(
            FactionId::from("bravo"),
            Faction {
                id: FactionId::from("bravo"),
                name: "Bravo".into(),
                faction_type: FactionType::Civilian,
                ..Default::default()
            },
        );
        scenario.simulation.max_ticks = 100;
        scenario
    }

    /// Build a synthetic state with both factions present and a tick
    /// already advanced to 50 (mid-run).
    fn build_minimal_state() -> SimulationState {
        use crate::state::RuntimeFactionState;
        let alpha = FactionId::from("alpha");
        let bravo = FactionId::from("bravo");
        let mut faction_states = BTreeMap::new();
        for fid in [alpha.clone(), bravo.clone()] {
            faction_states.insert(
                fid.clone(),
                RuntimeFactionState {
                    faction_id: fid.clone(),
                    total_strength: 100.0,
                    morale: 0.6,
                    resources: 50.0,
                    resource_rate: 5.0,
                    logistics_capacity: 10.0,
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
        let mut initial = BTreeMap::new();
        initial.insert(alpha.clone(), 100.0);
        initial.insert(bravo.clone(), 100.0);
        SimulationState {
            tick: 50,
            faction_states,
            region_control: BTreeMap::new(),
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
            initial_faction_strengths: initial,
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

    /// Effective weights with no triggers are exactly the base.
    #[test]
    fn effective_weights_with_no_triggers_equals_base() {
        let mut terms = BTreeMap::new();
        terms.insert(UtilityTerm::Control, 1.5);
        terms.insert(UtilityTerm::CasualtiesSelf, 0.5);
        let profile = FactionUtility {
            terms: terms.clone(),
            triggers: vec![],
            time_horizon_ticks: None,
        };
        let scenario = minimal_scenario_with_two_factions();
        let state = build_minimal_state();
        let campaigns = BTreeMap::new();
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert_eq!(ew.weights, terms);
        assert!(ew.fired_triggers.is_empty());
    }

    /// A trigger that doesn't match leaves weights unchanged.
    #[test]
    fn unmatched_trigger_does_not_adjust_weights() {
        let mut terms = BTreeMap::new();
        terms.insert(UtilityTerm::Control, 1.0);
        let mut adj = BTreeMap::new();
        adj.insert(UtilityTerm::Control, 2.0);
        let profile = FactionUtility {
            terms,
            triggers: vec![AdaptiveTrigger {
                id: "deadline".into(),
                description: "".into(),
                condition: AdaptiveCondition::TickFraction { fraction: 0.99 },
                adjustments: adj,
            }],
            time_horizon_ticks: None,
        };
        let scenario = minimal_scenario_with_two_factions();
        let state = build_minimal_state();
        let campaigns = BTreeMap::new();
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert!((ew.weights[&UtilityTerm::Control] - 1.0).abs() < 1e-12);
        assert!(ew.fired_triggers.is_empty());
    }

    /// A matched trigger multiplies the named term's weight.
    #[test]
    fn matched_trigger_multiplies_weight() {
        let mut terms = BTreeMap::new();
        terms.insert(UtilityTerm::Control, 1.0);
        terms.insert(UtilityTerm::CasualtiesSelf, 0.5);
        let mut adj = BTreeMap::new();
        adj.insert(UtilityTerm::CasualtiesSelf, 2.0);
        let profile = FactionUtility {
            terms,
            triggers: vec![AdaptiveTrigger {
                id: "deadline".into(),
                description: "".into(),
                condition: AdaptiveCondition::TickFraction { fraction: 0.0 },
                adjustments: adj,
            }],
            time_horizon_ticks: None,
        };
        let scenario = minimal_scenario_with_two_factions();
        let state = build_minimal_state();
        let campaigns = BTreeMap::new();
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert!((ew.weights[&UtilityTerm::CasualtiesSelf] - 1.0).abs() < 1e-12);
        assert_eq!(ew.fired_triggers, vec!["deadline"]);
    }

    /// Multiple triggers compose multiplicatively in declaration order.
    #[test]
    fn multiple_matched_triggers_compose() {
        let mut terms = BTreeMap::new();
        terms.insert(UtilityTerm::Control, 1.0);
        let mut adj_a = BTreeMap::new();
        adj_a.insert(UtilityTerm::Control, 2.0);
        let mut adj_b = BTreeMap::new();
        adj_b.insert(UtilityTerm::Control, 0.5);
        let profile = FactionUtility {
            terms,
            triggers: vec![
                AdaptiveTrigger {
                    id: "double".into(),
                    description: "".into(),
                    condition: AdaptiveCondition::TickFraction { fraction: 0.0 },
                    adjustments: adj_a,
                },
                AdaptiveTrigger {
                    id: "halve".into(),
                    description: "".into(),
                    condition: AdaptiveCondition::TickFraction { fraction: 0.0 },
                    adjustments: adj_b,
                },
            ],
            time_horizon_ticks: None,
        };
        let scenario = minimal_scenario_with_two_factions();
        let state = build_minimal_state();
        let campaigns = BTreeMap::new();
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert!((ew.weights[&UtilityTerm::Control] - 1.0).abs() < 1e-12);
        assert_eq!(ew.fired_triggers.len(), 2);
    }

    #[test]
    fn time_horizon_override_shrinks_tick_fraction_threshold() {
        // tick = 50; max_ticks = 100; horizon override = 60. The
        // tick_fraction is 50/60 = 0.833, which crosses 0.8 even
        // though the scenario-level fraction would be 0.5.
        let scenario = minimal_scenario_with_two_factions();
        let state = build_minimal_state();
        let campaigns = BTreeMap::new();
        let mut adj = BTreeMap::new();
        adj.insert(UtilityTerm::Control, 2.0);
        let profile = FactionUtility {
            terms: BTreeMap::new(),
            triggers: vec![AdaptiveTrigger {
                id: "tight_deadline".into(),
                description: "".into(),
                condition: AdaptiveCondition::TickFraction { fraction: 0.8 },
                adjustments: adj,
            }],
            time_horizon_ticks: Some(60),
        };
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert_eq!(ew.fired_triggers, vec!["tight_deadline"]);
    }

    #[test]
    fn strength_loss_fraction_fires_after_loss() {
        let scenario = minimal_scenario_with_two_factions();
        let mut state = build_minimal_state();
        // Drop alpha's strength to 30 (lost 70%).
        let alpha_fs = state
            .faction_states
            .get_mut(&FactionId::from("alpha"))
            .expect("alpha");
        alpha_fs.total_strength = 30.0;
        let campaigns = BTreeMap::new();
        let mut adj = BTreeMap::new();
        adj.insert(UtilityTerm::CasualtiesSelf, 2.0);
        let profile = FactionUtility {
            terms: BTreeMap::new(),
            triggers: vec![AdaptiveTrigger {
                id: "bleeding".into(),
                description: "".into(),
                condition: AdaptiveCondition::StrengthLossFraction { fraction: 0.5 },
                adjustments: adj,
            }],
            time_horizon_ticks: None,
        };
        let ew = effective_weights(
            &profile,
            &FactionId::from("alpha"),
            &state,
            &scenario,
            &campaigns,
        );
        assert_eq!(ew.fired_triggers, vec!["bleeding"]);
    }
}
