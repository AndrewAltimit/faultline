//! Cross-run displacement-flow analytics (Epic D round-three item 4).
//!
//! Pure post-processing of [`RunResult.displacement_reports`]. Per
//! region, walks the run set and computes mean / max / mean-terminal /
//! mean-flow rollups. A region that never had a non-zero displaced
//! fraction in any run is not represented; the report section elides
//! entirely on the empty case.

use std::collections::BTreeMap;

use faultline_types::ids::RegionId;
use faultline_types::stats::{DisplacementSummary, RunResult};

/// Compute the cross-run displacement summary keyed by `RegionId`.
///
/// Empty `BTreeMap` when the run set has no displacement activity.
/// Iteration is `BTreeMap`-ordered for deterministic rendering.
pub fn compute_displacement_summaries(
    runs: &[RunResult],
) -> BTreeMap<RegionId, DisplacementSummary> {
    let mut out: BTreeMap<RegionId, DisplacementSummary> = BTreeMap::new();
    if runs.is_empty() {
        return out;
    }
    let n_runs = runs.len() as u32;
    let n_runs_f = f64::from(n_runs.max(1));

    // Aggregator: peak / max-peak / terminal / inflow / outflow / absorbed accumulators.
    struct Agg {
        stressed_runs: u32,
        peak_sum: f64,
        max_peak: f64,
        terminal_sum: f64,
        inflow_sum: f64,
        outflow_sum: f64,
        absorbed_sum: f64,
    }
    let mut per_region: BTreeMap<RegionId, Agg> = BTreeMap::new();

    for run in runs {
        for (rid, report) in &run.displacement_reports {
            if report.peak_displaced <= 0.0 {
                continue;
            }
            let agg = per_region.entry(rid.clone()).or_insert(Agg {
                stressed_runs: 0,
                peak_sum: 0.0,
                max_peak: 0.0,
                terminal_sum: 0.0,
                inflow_sum: 0.0,
                outflow_sum: 0.0,
                absorbed_sum: 0.0,
            });
            agg.stressed_runs += 1;
            agg.peak_sum += report.peak_displaced;
            if report.peak_displaced > agg.max_peak {
                agg.max_peak = report.peak_displaced;
            }
            agg.terminal_sum += report.terminal_displaced;
            agg.inflow_sum += report.total_inflow;
            agg.outflow_sum += report.total_outflow;
            agg.absorbed_sum += report.total_absorbed;
        }
    }

    for (rid, agg) in per_region {
        out.insert(
            rid.clone(),
            DisplacementSummary {
                region: rid,
                n_runs,
                stressed_runs: agg.stressed_runs,
                mean_peak: agg.peak_sum / n_runs_f,
                max_peak: agg.max_peak,
                mean_terminal: agg.terminal_sum / n_runs_f,
                mean_total_inflow: agg.inflow_sum / n_runs_f,
                mean_total_outflow: agg.outflow_sum / n_runs_f,
                mean_total_absorbed: agg.absorbed_sum / n_runs_f,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::{FactionId, RegionId};
    use faultline_types::stats::{Outcome, RegionDisplacementReport, RunResult, StateSnapshot};

    fn _suppress_unused() -> FactionId {
        FactionId::from("noop")
    }

    fn empty_run() -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 0,
            final_state: StateSnapshot {
                tick: 0,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: vec![],
            },
            snapshots: vec![],
            event_log: vec![],
            campaign_reports: BTreeMap::new(),
            defender_queue_reports: vec![],
            network_reports: BTreeMap::new(),
            fracture_events: vec![],
            supply_pressure_reports: BTreeMap::new(),
            civilian_activations: vec![],
            tech_costs: BTreeMap::new(),
            narrative_events: vec![],
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: BTreeMap::new(),
        }
    }

    #[test]
    fn empty_runs_produce_empty_summary() {
        assert!(compute_displacement_summaries(&[]).is_empty());
        assert!(compute_displacement_summaries(&[empty_run()]).is_empty());
    }

    #[test]
    fn aggregates_two_runs_correctly() {
        let region = RegionId::from("downtown");
        let mut run1 = empty_run();
        run1.displacement_reports.insert(
            region.clone(),
            RegionDisplacementReport {
                region: region.clone(),
                stressed_ticks: 5,
                peak_displaced: 0.4,
                terminal_displaced: 0.2,
                total_inflow: 0.5,
                total_outflow: 0.1,
                total_absorbed: 0.2,
            },
        );
        let mut run2 = empty_run();
        run2.displacement_reports.insert(
            region.clone(),
            RegionDisplacementReport {
                region: region.clone(),
                stressed_ticks: 3,
                peak_displaced: 0.2,
                terminal_displaced: 0.1,
                total_inflow: 0.3,
                total_outflow: 0.05,
                total_absorbed: 0.15,
            },
        );

        let summary = compute_displacement_summaries(&[run1, run2]);
        let row = summary.get(&region).expect("present");
        assert_eq!(row.n_runs, 2);
        assert_eq!(row.stressed_runs, 2);
        assert!((row.mean_peak - 0.3).abs() < 1e-9);
        assert!((row.max_peak - 0.4).abs() < 1e-9);
        assert!((row.mean_terminal - 0.15).abs() < 1e-9);
        assert!((row.mean_total_inflow - 0.4).abs() < 1e-9);
        assert!((row.mean_total_outflow - 0.075).abs() < 1e-9);
        assert!((row.mean_total_absorbed - 0.175).abs() < 1e-9);
    }
}
