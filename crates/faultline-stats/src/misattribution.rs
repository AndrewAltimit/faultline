//! Cross-run believed-attribution analytics (Epic M round-two —
//! believed-attribution rolls).
//!
//! Aggregates the per-run [`AttributionEventReport`] logs into a single
//! batch-level [`MisattributionSummary`] surfaced in the
//! `## Attribution Fidelity` report section.
//!
//! Headline analytical signals:
//! - `misattribution_rate` — fraction of detection-time attribution
//!   rolls where the defender fingered the *wrong* faction. The core
//!   "does the defender know who hit it?" diagnostic.
//! - `deception_driven_rate` — fraction of rolls implicated by a
//!   planted `Deceived` belief. Decomposes the divergence into
//!   intelligence-confusion vs. false-flag drivers.
//! - `fracture_misattributions` — count of (run, fracture) pairs where
//!   a faction fractured against a counterparty it had misattributed an
//!   attack to in the same run. The behavioral payoff: misattribution
//!   didn't just happen, it broke an alliance against an innocent party.
//! - `confusion_pairs` — per-`true→believed` divergence counts, so the
//!   analyst can see *which* faction the blame landed on.
//!
//! ## Gating
//!
//! Returns `None` when no run produced any believed-attribution roll —
//! either the scenario left
//! `simulation.belief_model.believed_attribution = false` or no
//! defender ever detected a phase. The report section elides on that
//! signal, so scenarios that don't opt in are unchanged.
//!
//! ## Determinism
//!
//! Pure function of `runs`. No RNG, no `HashMap`, `BTreeMap`-ordered
//! iteration. Same input ⇒ same output.

use std::collections::BTreeMap;

use faultline_types::stats::{MisattributionSummary, RunResult};

/// Aggregate per-run believed-attribution logs into one batch summary.
/// `None` when no run produced a single attribution roll.
pub fn compute_misattribution_summary(runs: &[RunResult]) -> Option<MisattributionSummary> {
    let any = runs.iter().any(|r| !r.attribution_events.is_empty());
    if !any {
        return None;
    }

    let mut total_rolls: u64 = 0;
    let mut misattributed_rolls: u64 = 0;
    let mut deception_driven_rolls: u64 = 0;
    let mut fracture_misattributions: u64 = 0;
    let mut confusion_pairs: BTreeMap<String, u64> = BTreeMap::new();

    for run in runs {
        for ev in &run.attribution_events {
            total_rolls = total_rolls.saturating_add(1);
            if ev.misattributed {
                misattributed_rolls = misattributed_rolls.saturating_add(1);
                let key = format!("{}→{}", ev.true_attacker.0, ev.believed_attacker.0);
                *confusion_pairs.entry(key).or_default() += 1;
            }
            if ev.deception_driven {
                deception_driven_rolls = deception_driven_rolls.saturating_add(1);
            }
        }

        // Correlate misattributions with alliance fractures in the same
        // run: a fracture firing where the fracturing faction is the
        // defender that misattributed an attack to the very counterparty
        // it is now turning against. This is the behavioral payoff —
        // the misattribution drove the break.
        for fr in &run.fracture_events {
            let drove = run.attribution_events.iter().any(|ev| {
                ev.misattributed
                    && ev.defender == fr.faction
                    && ev.believed_attacker == fr.counterparty
            });
            if drove {
                fracture_misattributions = fracture_misattributions.saturating_add(1);
            }
        }
    }

    let denom = total_rolls as f64;
    let misattribution_rate = if total_rolls > 0 {
        misattributed_rolls as f64 / denom
    } else {
        0.0
    };
    let deception_driven_rate = if total_rolls > 0 {
        deception_driven_rolls as f64 / denom
    } else {
        0.0
    };

    Some(MisattributionSummary {
        total_rolls,
        misattributed_rolls,
        deception_driven_rolls,
        misattribution_rate,
        deception_driven_rate,
        fracture_misattributions,
        confusion_pairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::faction::Diplomacy;
    use faultline_types::ids::{FactionId, KillChainId};
    use faultline_types::stats::{
        AttributionEventReport, FractureEvent, Outcome, RunResult, StateSnapshot,
    };

    fn minimal_run() -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 1,
            final_state: StateSnapshot {
                tick: 1,
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
            supply_pressure_reports: BTreeMap::new(),
            civilian_activations: Vec::new(),
            tech_costs: BTreeMap::new(),
            narrative_events: Vec::new(),
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: BTreeMap::new(),
            utility_decisions: BTreeMap::new(),
            belief_accuracy: BTreeMap::new(),
            belief_snapshots: BTreeMap::new(),
            attribution_events: Vec::new(),
            force_projection_reports: std::collections::BTreeMap::new(),
        }
    }

    fn roll(
        defender: &str,
        true_attacker: &str,
        believed: &str,
        deception_driven: bool,
    ) -> AttributionEventReport {
        AttributionEventReport {
            tick: 1,
            defender: FactionId::from(defender),
            chain: KillChainId::from("c1"),
            true_attacker: FactionId::from(true_attacker),
            believed_attacker: FactionId::from(believed),
            confidence: 0.7,
            misattributed: true_attacker != believed,
            deception_driven,
        }
    }

    #[test]
    fn none_when_no_rolls() {
        assert!(compute_misattribution_summary(&[]).is_none());
        assert!(compute_misattribution_summary(&[minimal_run()]).is_none());
    }

    #[test]
    fn counts_and_rates() {
        let mut run = minimal_run();
        // 4 rolls: 2 correct, 2 misattributed (1 deception-driven).
        run.attribution_events = vec![
            roll("blue", "red", "red", false),
            roll("blue", "red", "red", false),
            roll("blue", "red", "green", false),
            roll("blue", "red", "green", true),
        ];
        let s = compute_misattribution_summary(&[run]).expect("summary");
        assert_eq!(s.total_rolls, 4);
        assert_eq!(s.misattributed_rolls, 2);
        assert_eq!(s.deception_driven_rolls, 1);
        assert!((s.misattribution_rate - 0.5).abs() < 1e-9);
        assert!((s.deception_driven_rate - 0.25).abs() < 1e-9);
        assert_eq!(s.confusion_pairs.get("red→green").copied(), Some(2));
        assert_eq!(s.fracture_misattributions, 0);
    }

    #[test]
    fn fracture_misattribution_correlated() {
        let mut run = minimal_run();
        run.attribution_events = vec![roll("blue", "red", "green", false)];
        // Blue fractures against green — the faction it misattributed
        // the attack to.
        run.fracture_events = vec![FractureEvent {
            tick: 2,
            faction: FactionId::from("blue"),
            counterparty: FactionId::from("green"),
            rule_id: "r1".into(),
            previous_stance: Diplomacy::Allied,
            new_stance: Diplomacy::Hostile,
        }];
        let s = compute_misattribution_summary(&[run]).expect("summary");
        assert_eq!(s.fracture_misattributions, 1);
    }

    #[test]
    fn fracture_against_true_attacker_not_counted() {
        let mut run = minimal_run();
        // Correct attribution (not misattributed).
        run.attribution_events = vec![roll("blue", "red", "red", false)];
        run.fracture_events = vec![FractureEvent {
            tick: 2,
            faction: FactionId::from("blue"),
            counterparty: FactionId::from("red"),
            rule_id: "r1".into(),
            previous_stance: Diplomacy::Cooperative,
            new_stance: Diplomacy::Hostile,
        }];
        let s = compute_misattribution_summary(&[run]).expect("summary");
        assert_eq!(s.fracture_misattributions, 0);
    }

    #[test]
    fn determinism_same_input_same_output() {
        let mut run = minimal_run();
        run.attribution_events = vec![
            roll("blue", "red", "green", true),
            roll("blue", "red", "red", false),
        ];
        let runs = vec![run.clone(), run.clone()];
        let a = compute_misattribution_summary(&runs);
        let b = compute_misattribution_summary(&runs);
        let aj = serde_json::to_string(&a).expect("ser a");
        let bj = serde_json::to_string(&b).expect("ser b");
        assert_eq!(aj, bj);
    }
}
