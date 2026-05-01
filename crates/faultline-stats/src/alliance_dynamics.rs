//! Cross-run alliance-fracture rollup (Epic D round two).
//!
//! Pure post-processing of `RunResult.fracture_events` and
//! `Faction.alliance_fracture` declarations. No engine re-runs, no
//! RNG draws — determinism follows directly from the input.
//!
//! Returns `None` when no scenario faction declares an
//! `alliance_fracture` block; the report renderer reads `Option::None`
//! as "elide the section entirely" so legacy scenarios produce no
//! output.

use std::collections::BTreeMap;

use faultline_engine::fracture as fracture_engine;
use faultline_types::faction::Diplomacy;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{AllianceDynamics, FractureRuleSummary, RunResult};

/// Build the cross-run [`AllianceDynamics`] roll-up.
///
/// Iterates the scenario's declared rules in `BTreeMap` order so the
/// emitted rule order is deterministic. For each rule we walk every
/// run's `fracture_events` log to count fires and capture fire ticks,
/// then walk the run's terminal `final_state.faction_states` view to
/// compute the final-stance distribution. The final stance is read by
/// re-applying the same priority order the engine uses
/// (`diplomacy_overrides` ⪰ `Faction.diplomacy` ⪰ `Diplomacy::Neutral`),
/// so the report shows the post-fracture stance even on runs where the
/// rule actually fired.
pub fn compute_alliance_dynamics(
    runs: &[RunResult],
    scenario: &Scenario,
) -> Option<AllianceDynamics> {
    if !scenario
        .factions
        .values()
        .any(|f| f.alliance_fracture.is_some())
    {
        return None;
    }

    let n_runs = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");
    let mut rules = Vec::new();

    for (faction_id, faction) in &scenario.factions {
        let Some(af) = &faction.alliance_fracture else {
            continue;
        };
        for rule in &af.rules {
            let mut fire_ticks: Vec<u32> = Vec::new();
            let mut final_stance_distribution: BTreeMap<Diplomacy, u32> = BTreeMap::new();

            for run in runs {
                // Did this rule fire in this run? Multiple rules may
                // have fired; we only count this rule's first fire.
                if let Some(ev) = run.fracture_events.iter().find(|ev| {
                    ev.faction == *faction_id
                        && ev.rule_id == rule.id
                        && ev.counterparty == rule.counterparty
                }) {
                    fire_ticks.push(ev.tick);
                }

                // Reconstruct the run's terminal stance for the
                // (faction, counterparty) pair. The engine state
                // isn't carried into the report layer — but the
                // fracture_events log captures every transition with
                // its `new_stance`, so the *latest* event for this
                // pair gives the terminal override; otherwise fall
                // back to the scenario baseline.
                let terminal = terminal_stance(scenario, run, faction_id, &rule.counterparty);
                *final_stance_distribution.entry(terminal).or_insert(0) += 1;
            }

            fire_ticks.sort_unstable();
            let fire_count = u32::try_from(fire_ticks.len()).expect("fires per rule fit u32");
            let fire_rate = if n_runs == 0 {
                0.0
            } else {
                f64::from(fire_count) / f64::from(n_runs)
            };
            let mean_fire_tick = if fire_count == 0 {
                None
            } else {
                let sum: u64 = fire_ticks.iter().map(|t| u64::from(*t)).sum();
                Some(sum as f64 / f64::from(fire_count))
            };

            rules.push(FractureRuleSummary {
                faction: faction_id.clone(),
                counterparty: rule.counterparty.clone(),
                rule_id: rule.id.clone(),
                description: rule.description.clone(),
                n_runs,
                fire_count,
                fire_rate,
                mean_fire_tick,
                fire_ticks,
                final_stance_distribution,
            });
        }
    }

    Some(AllianceDynamics { rules })
}

/// Reconstruct the terminal stance for `(source -> target)` in one
/// run. Walks `run.fracture_events` for the latest event affecting
/// the pair (latest = max tick, ties resolved by emission order so
/// the last-recorded fracture wins — matches the engine's
/// last-write-wins semantics on `diplomacy_overrides`). Falls back to
/// the scenario baseline (`Faction.diplomacy`) and finally to
/// `Neutral`.
///
/// `EventEffect::DiplomacyChange` overrides set in-engine are not
/// reconstructable from the run report (the report records only
/// alliance-fracture events, not arbitrary diplomacy changes). For
/// the alliance-dynamics rollup that's the right contract — the
/// section's whole job is to characterize fracture-rule firings —
/// but downstream tooling that wants the full diplomacy trace would
/// need a richer log.
fn terminal_stance(
    scenario: &Scenario,
    run: &RunResult,
    source: &faultline_types::ids::FactionId,
    target: &faultline_types::ids::FactionId,
) -> Diplomacy {
    let latest = run
        .fracture_events
        .iter()
        .filter(|ev| ev.faction == *source && ev.counterparty == *target)
        .max_by_key(|ev| ev.tick);
    if let Some(ev) = latest {
        return ev.new_stance;
    }
    // Fall through to the engine's resolution helper to keep the
    // baseline semantics in one place.
    fracture_engine::current_stance(
        // Build a throwaway empty SimulationState? No — current_stance
        // needs `&SimulationState` to read overrides, but we already
        // know there are none for this branch. Use the baseline-only
        // fallback directly.
        &empty_state(scenario),
        scenario,
        source,
        target,
    )
}

/// Construct an empty `SimulationState` for the baseline-resolution
/// path. Avoids cloning the engine's startup-init logic — we only
/// need the empty `diplomacy_overrides` map so `current_stance` falls
/// through to the scenario baseline.
fn empty_state(scenario: &Scenario) -> faultline_engine::SimulationState {
    use std::collections::{BTreeMap, BTreeSet};
    faultline_engine::SimulationState {
        tick: 0,
        faction_states: BTreeMap::new(),
        region_control: BTreeMap::new(),
        infra_status: BTreeMap::new(),
        institution_loyalty: BTreeMap::new(),
        political_climate: scenario.political_climate.clone(),
        events_fired: BTreeSet::new(),
        events_fired_this_tick: Vec::new(),
        snapshots: Vec::new(),
        non_kinetic: Default::default(),
        metric_history: Vec::new(),
        defender_queues: BTreeMap::new(),
        network_states: BTreeMap::new(),
        defender_over_budget_tick: None,
        diplomacy_overrides: BTreeMap::new(),
        fired_fractures: BTreeSet::new(),
        initial_faction_strengths: BTreeMap::new(),
        fracture_events: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::faction::{
        AllianceFracture, Diplomacy, DiplomaticStance, Faction, FractureCondition, FractureRule,
    };
    use faultline_types::ids::FactionId;
    use faultline_types::stats::{FractureEvent, Outcome, RunResult, StateSnapshot};

    fn empty_run() -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 10,
            final_state: StateSnapshot {
                tick: 10,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: Vec::new(),
            },
            snapshots: Vec::new(),
            event_log: Vec::new(),
            campaign_reports: BTreeMap::new(),
            defender_queue_reports: Vec::new(),
            network_reports: BTreeMap::new(),
            fracture_events: Vec::new(),
        }
    }

    fn scenario_with_rule() -> (Scenario, FactionId, FactionId) {
        let alpha = FactionId::from("alpha");
        let beta = FactionId::from("beta");
        let mut s = Scenario::default();
        let f_alpha = Faction {
            id: alpha.clone(),
            diplomacy: vec![DiplomaticStance {
                target_faction: beta.clone(),
                stance: Diplomacy::Cooperative,
            }],
            alliance_fracture: Some(AllianceFracture {
                rules: vec![FractureRule {
                    id: "betrayed".into(),
                    counterparty: beta.clone(),
                    new_stance: Diplomacy::Hostile,
                    condition: FractureCondition::TensionThreshold { threshold: 0.7 },
                    description: String::new(),
                }],
            }),
            ..Default::default()
        };
        let f_beta = Faction {
            id: beta.clone(),
            ..Default::default()
        };
        s.factions.insert(alpha.clone(), f_alpha);
        s.factions.insert(beta.clone(), f_beta);
        (s, alpha, beta)
    }

    #[test]
    fn returns_none_when_no_rules_declared() {
        let s = Scenario::default();
        assert!(compute_alliance_dynamics(&[], &s).is_none());
    }

    #[test]
    fn empty_runs_with_rules_emits_zero_fire_summary() {
        let (s, alpha, beta) = scenario_with_rule();
        let dyn_ = compute_alliance_dynamics(&[], &s).expect("rule declared");
        assert_eq!(dyn_.rules.len(), 1);
        let row = &dyn_.rules[0];
        assert_eq!(row.faction, alpha);
        assert_eq!(row.counterparty, beta);
        assert_eq!(row.fire_count, 0);
        assert!(row.mean_fire_tick.is_none());
        // No runs -> no terminal-stance counts at all (sum is zero).
        assert!(row.final_stance_distribution.is_empty());
    }

    #[test]
    fn aggregates_fires_and_baseline_stances() {
        let (s, alpha, beta) = scenario_with_rule();
        // Three runs: 2 fired, 1 didn't.
        let mut runs = vec![empty_run(), empty_run(), empty_run()];
        runs[0].fracture_events.push(FractureEvent {
            tick: 5,
            faction: alpha.clone(),
            counterparty: beta.clone(),
            rule_id: "betrayed".into(),
            previous_stance: Diplomacy::Cooperative,
            new_stance: Diplomacy::Hostile,
        });
        runs[1].fracture_events.push(FractureEvent {
            tick: 7,
            faction: alpha.clone(),
            counterparty: beta.clone(),
            rule_id: "betrayed".into(),
            previous_stance: Diplomacy::Cooperative,
            new_stance: Diplomacy::Hostile,
        });

        let dyn_ = compute_alliance_dynamics(&runs, &s).expect("rule declared");
        let row = &dyn_.rules[0];
        assert_eq!(row.n_runs, 3);
        assert_eq!(row.fire_count, 2);
        assert!((row.fire_rate - 2.0 / 3.0).abs() < 1e-9);
        assert!((row.mean_fire_tick.expect("fires") - 6.0).abs() < 1e-9);
        // Two runs ended Hostile (fired), one ended Cooperative (baseline).
        assert_eq!(
            row.final_stance_distribution.get(&Diplomacy::Hostile),
            Some(&2)
        );
        assert_eq!(
            row.final_stance_distribution.get(&Diplomacy::Cooperative),
            Some(&1)
        );
    }
}
