//! Cross-run belief-asymmetry analytics (Epic M round-one).
//!
//! This module aggregates per-run [`BeliefAccuracyReport`]s into
//! per-faction [`BeliefAsymmetrySummary`] rows surfaced in the
//! `## Belief Asymmetry` report section.
//!
//! Headline analytical signals (per faction):
//! - `mean_force_strength_error` — average per-run mean absolute
//!   error between believed and actual opponent force strength. Low
//!   = the faction's intelligence is good. High paired with non-zero
//!   `mean_deception_events` suggests deception drove the inaccuracy.
//! - `mean_region_accuracy` — average per-run fraction of
//!   correctly-believed region-controllers, in `[0, 1]`. 1.0 =
//!   perfect awareness across the believer's known regions.
//! - `mean_deception_events` / `mean_intel_shares` — raw counters
//!   for "how often was this faction deceived / informed?"
//! - `mean_terminal_deceived_beliefs` — average count of
//!   `BeliefSource::Deceived` entries persisting at run end. Distinct
//!   from `mean_deception_events` because a deception that gets
//!   refreshed away by direct observation is no longer "active" at
//!   run end.
//! - `max_force_strength_error` — worst-case per-run error across
//!   the run set. Captures "what was the most-fooled this faction
//!   ever was, in any run?" — paired with the mean to surface
//!   variance.
//!
//! ## Pre-seeding
//!
//! When the belief model is enabled but a faction never produced a
//! belief report (e.g. the run finished before the belief phase
//! ever ran for some pathological short scenario), the aggregator
//! still emits a zero-valued entry so the analyst sees "declared
//! but never engaged" rather than silent omission. This mirrors the
//! Epic J round-one pattern from
//! `utility_decomposition::compute_utility_decompositions`.
//!
//! ## Determinism
//!
//! Pure function of `(runs, scenario)`. No RNG, no `HashMap`,
//! `BTreeMap`-ordered iteration. Same input ⇒ same output.

use std::collections::BTreeMap;

use faultline_types::ids::FactionId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{BeliefAsymmetrySummary, RunResult};

/// Aggregate per-run belief-accuracy reports into per-faction
/// summaries. Empty when the scenario opted out of the belief model
/// (`simulation.belief_model.enabled = false`).
pub fn compute_belief_summaries(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<FactionId, BeliefAsymmetrySummary> {
    if !belief_enabled(scenario) {
        return BTreeMap::new();
    }
    let mut out: BTreeMap<FactionId, BeliefAsymmetrySummary> = BTreeMap::new();

    // Pre-seed entries for every faction so the analyst can see
    // "this faction declared belief mode but never engaged" cleanly
    // rather than as silent omission.
    for fid in scenario.factions.keys() {
        out.insert(
            fid.clone(),
            BeliefAsymmetrySummary {
                faction: fid.clone(),
                runs_with_belief: 0,
                mean_force_strength_error: 0.0,
                mean_region_accuracy: 0.0,
                mean_deception_events: 0.0,
                mean_intel_shares: 0.0,
                mean_terminal_deceived_beliefs: 0.0,
                max_force_strength_error: 0.0,
            },
        );
    }

    if runs.is_empty() {
        return out;
    }

    // Accumulators, keyed by faction.
    let mut force_error_sum: BTreeMap<FactionId, f64> = BTreeMap::new();
    let mut force_error_max: BTreeMap<FactionId, f64> = BTreeMap::new();
    let mut region_acc_sum: BTreeMap<FactionId, f64> = BTreeMap::new();
    let mut deception_sum: BTreeMap<FactionId, u64> = BTreeMap::new();
    let mut intel_sum: BTreeMap<FactionId, u64> = BTreeMap::new();
    let mut terminal_deceived_sum: BTreeMap<FactionId, u64> = BTreeMap::new();
    let mut runs_with_belief: BTreeMap<FactionId, u32> = BTreeMap::new();

    for run in runs {
        for (fid, report) in &run.belief_accuracy {
            let force_mean = if report.force_belief_ticks > 0 {
                report.force_strength_error_sum / f64::from(report.force_belief_ticks)
            } else {
                0.0
            };
            let region_mean = if report.region_belief_ticks > 0 {
                report.region_accuracy_sum / f64::from(report.region_belief_ticks)
            } else {
                0.0
            };

            *force_error_sum.entry(fid.clone()).or_default() += force_mean;
            let prev_max = force_error_max.entry(fid.clone()).or_default();
            if force_mean > *prev_max {
                *prev_max = force_mean;
            }
            *region_acc_sum.entry(fid.clone()).or_default() += region_mean;
            *deception_sum.entry(fid.clone()).or_default() +=
                u64::from(report.deception_events_received);
            *intel_sum.entry(fid.clone()).or_default() += u64::from(report.intel_shares_received);
            *terminal_deceived_sum.entry(fid.clone()).or_default() +=
                u64::from(report.deceived_beliefs_terminal);
            *runs_with_belief.entry(fid.clone()).or_default() += 1;
        }
    }

    for (fid, summary) in &mut out {
        let runs_n = runs_with_belief.get(fid).copied().unwrap_or(0);
        if runs_n == 0 {
            continue;
        }
        let denom = f64::from(runs_n);
        summary.runs_with_belief = runs_n;
        summary.mean_force_strength_error =
            force_error_sum.get(fid).copied().unwrap_or(0.0) / denom;
        summary.max_force_strength_error = force_error_max.get(fid).copied().unwrap_or(0.0);
        summary.mean_region_accuracy = region_acc_sum.get(fid).copied().unwrap_or(0.0) / denom;
        summary.mean_deception_events = deception_sum.get(fid).copied().unwrap_or(0) as f64 / denom;
        summary.mean_intel_shares = intel_sum.get(fid).copied().unwrap_or(0) as f64 / denom;
        summary.mean_terminal_deceived_beliefs =
            terminal_deceived_sum.get(fid).copied().unwrap_or(0) as f64 / denom;
    }

    out
}

fn belief_enabled(scenario: &Scenario) -> bool {
    scenario
        .simulation
        .belief_model
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::belief::BeliefModelConfig;
    use faultline_types::stats::{BeliefAccuracyReport, MonteCarloSummary, Outcome, StateSnapshot};

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
        }
    }

    fn scenario_with_belief(enabled: bool) -> Scenario {
        let mut s = Scenario::default();
        s.simulation.belief_model = Some(BeliefModelConfig {
            enabled,
            ..Default::default()
        });
        s.factions
            .insert(FactionId::from("red"), Default::default());
        s.factions
            .insert(FactionId::from("blue"), Default::default());
        s
    }

    #[test]
    fn empty_when_belief_disabled() {
        let s = scenario_with_belief(false);
        let runs: Vec<RunResult> = vec![];
        let out = compute_belief_summaries(&runs, &s);
        assert!(out.is_empty());
    }

    #[test]
    fn pre_seeds_factions_when_enabled_no_runs() {
        let s = scenario_with_belief(true);
        let out = compute_belief_summaries(&[], &s);
        assert_eq!(out.len(), 2);
        let red = out.get(&FactionId::from("red")).expect("entry");
        assert_eq!(red.runs_with_belief, 0);
        assert_eq!(red.mean_force_strength_error, 0.0);
    }

    #[test]
    fn averages_across_runs() {
        let s = scenario_with_belief(true);
        let mut run_a = minimal_run();
        run_a.belief_accuracy.insert(
            FactionId::from("red"),
            BeliefAccuracyReport {
                faction: FactionId::from("red"),
                force_belief_ticks: 10,
                force_strength_error_sum: 100.0, // mean 10.0 / tick
                region_belief_ticks: 10,
                region_accuracy_sum: 8.0, // mean 0.8 / tick
                deception_events_received: 2,
                intel_shares_received: 1,
                deceived_beliefs_terminal: 1,
            },
        );
        let mut run_b = minimal_run();
        run_b.belief_accuracy.insert(
            FactionId::from("red"),
            BeliefAccuracyReport {
                faction: FactionId::from("red"),
                force_belief_ticks: 10,
                force_strength_error_sum: 200.0, // mean 20.0 / tick
                region_belief_ticks: 10,
                region_accuracy_sum: 6.0, // mean 0.6 / tick
                deception_events_received: 4,
                intel_shares_received: 1,
                deceived_beliefs_terminal: 3,
            },
        );
        let out = compute_belief_summaries(&[run_a, run_b], &s);
        let red = out.get(&FactionId::from("red")).expect("entry");
        assert_eq!(red.runs_with_belief, 2);
        // Mean of run-means: (10 + 20) / 2 = 15.
        assert!((red.mean_force_strength_error - 15.0).abs() < 1e-9);
        // Max of run-means: 20.
        assert!((red.max_force_strength_error - 20.0).abs() < 1e-9);
        assert!((red.mean_region_accuracy - 0.7).abs() < 1e-9);
        assert!((red.mean_deception_events - 3.0).abs() < 1e-9);
        assert!((red.mean_intel_shares - 1.0).abs() < 1e-9);
        assert!((red.mean_terminal_deceived_beliefs - 2.0).abs() < 1e-9);
    }

    #[test]
    fn determinism_same_input_same_output() {
        let s = scenario_with_belief(true);
        let mut run = minimal_run();
        run.belief_accuracy.insert(
            FactionId::from("red"),
            BeliefAccuracyReport {
                faction: FactionId::from("red"),
                force_belief_ticks: 5,
                force_strength_error_sum: 50.0,
                region_belief_ticks: 5,
                region_accuracy_sum: 4.0,
                deception_events_received: 1,
                intel_shares_received: 0,
                deceived_beliefs_terminal: 0,
            },
        );
        let runs = vec![run.clone(), run.clone(), run.clone()];
        let a = compute_belief_summaries(&runs, &s);
        let b = compute_belief_summaries(&runs, &s);
        let aj = serde_json::to_string(&a).expect("ser a");
        let bj = serde_json::to_string(&b).expect("ser b");
        assert_eq!(aj, bj);
    }

    #[test]
    fn faction_with_no_belief_data_keeps_pre_seeded_zero() {
        let s = scenario_with_belief(true);
        let out = compute_belief_summaries(&[minimal_run()], &s);
        let blue = out.get(&FactionId::from("blue")).expect("entry");
        assert_eq!(blue.runs_with_belief, 0);
        assert_eq!(blue.mean_force_strength_error, 0.0);
    }

    #[test]
    fn full_summary_serializes() {
        let s = scenario_with_belief(true);
        let summary = MonteCarloSummary {
            total_runs: 0,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 0.0,
            metric_distributions: BTreeMap::new(),
            regional_control: BTreeMap::new(),
            event_probabilities: BTreeMap::new(),
            campaign_summaries: BTreeMap::new(),
            feasibility_matrix: Vec::new(),
            seam_scores: BTreeMap::new(),
            correlation_matrix: None,
            pareto_frontier: None,
            defender_capacity: Vec::new(),
            network_summaries: BTreeMap::new(),
            alliance_dynamics: None,
            supply_pressure_summaries: BTreeMap::new(),
            civilian_activation_summaries: BTreeMap::new(),
            tech_cost_summaries: BTreeMap::new(),
            calibration: None,
            narrative_dynamics: None,
            displacement_summaries: BTreeMap::new(),
            utility_decompositions: BTreeMap::new(),
            belief_summaries: compute_belief_summaries(&[], &s),
        };
        let json = serde_json::to_string(&summary).expect("serialize");
        assert!(json.contains("belief_summaries"));
    }
}
