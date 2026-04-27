//! Monte Carlo runner and statistical output for Faultline simulation.
//!
//! Provides [`MonteCarloRunner`] which executes N simulation runs
//! sequentially, collects [`RunResult`]s, and computes summary
//! statistics including win probabilities, duration distributions,
//! and metric distributions.

pub mod analysis;
pub mod coevolve;
pub mod counterfactual;
pub mod delta;
pub mod manifest;
pub mod morris;
pub mod network_metrics;
pub mod report;
pub mod search;
pub mod sensitivity;
pub mod time_dynamics;
pub mod uncertainty;

use std::collections::BTreeMap;

use thiserror::Error;
use tracing::{debug, info};

use faultline_engine::{Engine, EngineError};
use faultline_types::ids::{EventId, FactionId, KillChainId, PhaseId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    CampaignSummary, ConfidenceInterval, DistributionStats, MetricType, MonteCarloConfig,
    MonteCarloResult, MonteCarloSummary, PhaseOutcome, PhaseStats, PhaseStatsCIs, RunResult,
};

use crate::uncertainty::{percentile_bootstrap_ci_seeded, wilson_score_interval};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during Monte Carlo simulation.
#[derive(Debug, Error)]
pub enum StatsError {
    #[error("engine error on run {run_index}: {source}")]
    Engine { run_index: u32, source: EngineError },

    #[error("no runs completed")]
    NoRuns,

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

// ---------------------------------------------------------------------------
// MonteCarloRunner
// ---------------------------------------------------------------------------

/// Executes multiple simulation runs and aggregates results.
pub struct MonteCarloRunner;

impl MonteCarloRunner {
    /// Run N simulations sequentially, collecting results.
    ///
    /// Each run creates a new [`Engine`] with a deterministic seed
    /// derived from `config.seed` (or a default) plus the run index.
    pub fn run(
        config: &MonteCarloConfig,
        scenario: &Scenario,
    ) -> Result<MonteCarloResult, StatsError> {
        if config.num_runs == 0 {
            return Err(StatsError::InvalidConfig(
                "num_runs must be greater than zero".into(),
            ));
        }

        let base_seed = config.seed.unwrap_or(0xDEAD_BEEF);
        let mut runs = Vec::with_capacity(config.num_runs as usize);

        info!(
            num_runs = config.num_runs,
            base_seed, "starting Monte Carlo batch"
        );

        for i in 0..config.num_runs {
            let seed = base_seed.wrapping_add(u64::from(i));
            debug!(run_index = i, seed, "starting run");

            let mut engine =
                Engine::with_seed(scenario.clone(), seed).map_err(|e| StatsError::Engine {
                    run_index: i,
                    source: e,
                })?;

            let mut result = engine.run().map_err(|e| StatsError::Engine {
                run_index: i,
                source: e,
            })?;

            result.run_index = i;
            result.seed = seed;
            if !config.collect_snapshots {
                result.snapshots.clear();
            }
            runs.push(result);
        }

        let summary = compute_summary(&runs, scenario);

        Ok(MonteCarloResult { runs, summary })
    }
}

// ---------------------------------------------------------------------------
// Summary computation
// ---------------------------------------------------------------------------

/// Compute aggregate statistics from a collection of run results.
///
/// Produces win probabilities per faction, average duration, and
/// metric distributions (duration, final tension).
pub fn compute_summary(runs: &[RunResult], scenario: &Scenario) -> MonteCarloSummary {
    let total = runs.len() as f64;
    if total == 0.0 {
        return MonteCarloSummary {
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
        };
    }

    // Win rates.
    let mut win_counts: BTreeMap<FactionId, u32> = BTreeMap::new();
    for run in runs {
        if let Some(ref victor) = run.outcome.victor {
            *win_counts.entry(victor.clone()).or_insert(0) += 1;
        }
    }
    let n_runs = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");
    let win_rates: BTreeMap<FactionId, f64> = win_counts
        .iter()
        .map(|(fid, count)| (fid.clone(), f64::from(*count) / total))
        .collect();
    // `n_runs > 0` here because the `total == 0.0` early return above
    // rejects empty input. This guarantees `win_rate_cis` and `win_rates`
    // share the same key set — a structural invariant the report layer
    // depends on.
    let win_rate_cis: BTreeMap<FactionId, _> = win_counts
        .iter()
        .map(|(fid, count)| {
            let ci = wilson_score_interval(*count, n_runs)
                .expect("n_runs > 0 after empty-runs early return above");
            (fid.clone(), ci.into())
        })
        .collect();

    // Duration distribution.
    let durations: Vec<f64> = runs.iter().map(|r| f64::from(r.final_tick)).collect();
    let average_duration = durations.iter().copied().sum::<f64>() / total;

    // Final tension distribution.
    let tensions: Vec<f64> = runs.iter().map(|r| r.outcome.final_tension).collect();

    // Bootstrap-CI seeds are derived deterministically from the scenario's
    // simulation seed so that `render_markdown` output is bit-identical
    // across re-runs of `MonteCarloRunner::run` on the same inputs. Each
    // metric gets a distinct salt so its resample sequence is independent
    // of the others — otherwise all four CIs would share the same draws
    // and look artificially correlated.
    let base_boot_seed = scenario
        .simulation
        .seed
        .unwrap_or(0)
        .wrapping_add(0xB005_7CA9);
    let mut metric_distributions = BTreeMap::new();
    metric_distributions.insert(
        MetricType::Duration,
        compute_distribution_with_bootstrap(&durations, base_boot_seed.wrapping_add(1)),
    );
    metric_distributions.insert(
        MetricType::FinalTension,
        compute_distribution_with_bootstrap(&tensions, base_boot_seed.wrapping_add(2)),
    );

    // Total casualties: sum of (initial_strength - final_strength) across all factions per run.
    let initial_total_strength: f64 = scenario
        .factions
        .values()
        .flat_map(|f| f.forces.values())
        .map(|u| u.strength)
        .sum();
    let casualties: Vec<f64> = runs
        .iter()
        .map(|run| {
            let final_strength: f64 = run
                .final_state
                .faction_states
                .values()
                .map(|fs| fs.total_strength)
                .sum();
            (initial_total_strength - final_strength).max(0.0)
        })
        .collect();
    metric_distributions.insert(
        MetricType::TotalCasualties,
        compute_distribution_with_bootstrap(&casualties, base_boot_seed.wrapping_add(3)),
    );

    // Infrastructure damage: sum of (initial_status - final_status) across all infra nodes.
    if !scenario.map.infrastructure.is_empty() {
        let infra_damage: Vec<f64> = runs
            .iter()
            .map(|run| {
                scenario
                    .map
                    .infrastructure
                    .iter()
                    .map(|(iid, node)| {
                        let initial = node.initial_status;
                        let final_status = run
                            .final_state
                            .infra_status
                            .get(iid)
                            .copied()
                            .unwrap_or(initial);
                        (initial - final_status).max(0.0)
                    })
                    .sum()
            })
            .collect();
        metric_distributions.insert(
            MetricType::InfrastructureDamage,
            compute_distribution_with_bootstrap(&infra_damage, base_boot_seed.wrapping_add(4)),
        );
    }

    // Resources expended: sum of (initial_resources - final_resources) across all factions.
    let initial_total_resources: f64 = scenario
        .factions
        .values()
        .map(|f| f.initial_resources)
        .sum();
    let resources_expended: Vec<f64> = runs
        .iter()
        .map(|run| {
            let final_resources: f64 = run
                .final_state
                .faction_states
                .values()
                .map(|fs| fs.resources)
                .sum();
            (initial_total_resources - final_resources).max(0.0)
        })
        .collect();
    metric_distributions.insert(
        MetricType::ResourcesExpended,
        compute_distribution_with_bootstrap(&resources_expended, base_boot_seed.wrapping_add(5)),
    );

    // Regional control probabilities from final state.
    let regional_control = compute_regional_control(runs);

    // Event firing probabilities across all runs.
    let event_probabilities = compute_event_probabilities(runs);

    // Campaign / kill chain aggregation.
    let campaign_summaries = compute_campaign_summaries(runs, scenario);

    // Feasibility matrix + doctrinal seam scores.
    let feasibility_matrix =
        analysis::compute_feasibility_matrix(runs, scenario, &campaign_summaries);
    let seam_scores = analysis::compute_seam_scores(runs, scenario);

    // Time-dynamics post-processing (Epic C). All operate on the
    // already-collected campaign reports; none re-runs the engine.
    let correlation_matrix = time_dynamics::output_correlation_matrix(runs, scenario);
    let pareto_frontier = time_dynamics::pareto_frontier(runs, scenario);

    // Defender capacity rollup (Epic K). Pure post-processing of
    // per-run queue reports; preserves determinism.
    let defender_capacity = compute_defender_capacity_summary(runs);

    // Network rollup (Epic L). Pure post-processing of per-run
    // network reports; preserves determinism. Empty when scenario
    // declares no networks.
    let network_summaries = network_metrics::compute_network_summaries(runs, scenario);

    MonteCarloSummary {
        total_runs: u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX"),
        win_rates,
        win_rate_cis,
        average_duration,
        metric_distributions,
        regional_control,
        event_probabilities,
        campaign_summaries,
        feasibility_matrix,
        seam_scores,
        correlation_matrix,
        pareto_frontier,
        defender_capacity,
        network_summaries,
    }
}

/// Aggregate per-(faction, role) defender queue analytics across runs.
///
/// Pure post-processing over the per-run [`DefenderQueueReport`]s the
/// engine emitted — no re-runs, no new RNG draws, so the rollup is
/// fully determined by the same `(scenario, seed)` inputs that
/// produced the runs themselves.
///
/// `time_to_saturation` is right-censored: runs where the queue never
/// hit capacity contribute to `right_censored` rather than being
/// treated as instant or infinite saturation. The shape mirrors
/// [`time_dynamics::time_to_first_detection`] so report renderers can
/// reuse the same "right-censored distribution" template.
fn compute_defender_capacity_summary(
    runs: &[RunResult],
) -> Vec<faultline_types::stats::DefenderCapacitySummary> {
    use faultline_types::stats::{DefenderCapacitySummary, TimeToSaturation};

    if runs.is_empty() {
        return Vec::new();
    }

    // Group reports by (faction, role) — preserve `BTreeMap` ordering
    // so output is deterministic.
    let mut grouped: BTreeMap<
        (FactionId, faultline_types::ids::DefenderRoleId),
        Vec<&faultline_types::stats::DefenderQueueReport>,
    > = BTreeMap::new();
    for run in runs {
        for report in &run.defender_queue_reports {
            grouped
                .entry((report.faction.clone(), report.role.clone()))
                .or_default()
                .push(report);
        }
    }

    let mut out = Vec::new();
    for ((faction, role), reports) in grouped {
        let n = reports.len() as f64;
        let n_runs = u32::try_from(reports.len())
            .expect("reports.len() bounded by MonteCarloConfig.num_runs: u32");
        let capacity = reports.iter().map(|r| r.capacity).max().unwrap_or(0);

        let mean_utilization = reports.iter().map(|r| r.utilization).sum::<f64>() / n;
        let max_utilization = reports
            .iter()
            .map(|r| r.utilization)
            .fold(f64::NEG_INFINITY, f64::max)
            .max(0.0);
        let mean_max_depth = reports.iter().map(|r| f64::from(r.max_depth)).sum::<f64>() / n;
        let mean_dropped = reports.iter().map(|r| r.total_dropped as f64).sum::<f64>() / n;
        let mean_shadow_detections = reports
            .iter()
            .map(|r| f64::from(r.shadow_detections))
            .sum::<f64>()
            / n;

        let mut sat_samples: Vec<u32> = reports
            .iter()
            .filter_map(|r| r.time_to_saturation)
            .collect();
        sat_samples.sort_unstable();
        let saturated_runs = u32::try_from(sat_samples.len())
            .expect("sat_samples.len() <= reports.len(), bounded by num_runs: u32");
        let right_censored = n_runs.saturating_sub(saturated_runs);
        let stats = if sat_samples.is_empty() {
            None
        } else {
            let as_f64: Vec<f64> = sat_samples.iter().map(|t| f64::from(*t)).collect();
            Some(compute_distribution_inner(&as_f64, None))
        };
        let time_to_saturation = TimeToSaturation {
            saturated_runs,
            right_censored,
            samples: sat_samples,
            stats,
        };

        out.push(DefenderCapacitySummary {
            faction,
            role,
            capacity,
            n_runs,
            mean_utilization,
            max_utilization,
            mean_max_depth,
            mean_dropped,
            mean_shadow_detections,
            time_to_saturation,
        });
    }
    out
}

/// Aggregate per-kill-chain statistics across runs.
fn compute_campaign_summaries(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<KillChainId, CampaignSummary> {
    if scenario.kill_chains.is_empty() {
        return BTreeMap::new();
    }

    // Pre-compute the time-dynamics tables for every chain. Doing it
    // once and indexing per-chain below is O(runs × chains) total
    // versus O(runs × chains²) if we recomputed inside the per-chain
    // loop.
    let mut ttd = time_dynamics::time_to_first_detection(runs, scenario);
    let mut react = time_dynamics::defender_reaction_time(runs, scenario);

    let total = runs.len() as f64;
    let mut out = BTreeMap::new();

    for (chain_id, chain) in &scenario.kill_chains {
        let mut phase_success: BTreeMap<PhaseId, u32> = BTreeMap::new();
        let mut phase_fail: BTreeMap<PhaseId, u32> = BTreeMap::new();
        let mut phase_detect: BTreeMap<PhaseId, u32> = BTreeMap::new();
        let mut phase_not_reached: BTreeMap<PhaseId, u32> = BTreeMap::new();
        let mut phase_tick_sums: BTreeMap<PhaseId, (u64, u32)> = BTreeMap::new();

        let mut detected_runs = 0u32;
        let mut any_success_runs = 0u32;
        let mut attacker_spend_sum = 0.0_f64;
        let mut defender_spend_sum = 0.0_f64;
        let mut attribution_sum = 0.0_f64;

        for run in runs {
            if let Some(report) = run.campaign_reports.get(chain_id) {
                if report.defender_alerted {
                    detected_runs += 1;
                }
                attacker_spend_sum += report.attacker_spend;
                defender_spend_sum += report.defender_spend;
                attribution_sum += report.attribution_confidence;

                let mut any_success = false;
                for pid in chain.phases.keys() {
                    let outcome = report
                        .phase_outcomes
                        .get(pid)
                        .cloned()
                        .unwrap_or(PhaseOutcome::Pending);
                    match outcome {
                        PhaseOutcome::Succeeded { tick } => {
                            *phase_success.entry(pid.clone()).or_insert(0) += 1;
                            any_success = true;
                            let entry = phase_tick_sums.entry(pid.clone()).or_insert((0, 0));
                            entry.0 += u64::from(tick);
                            entry.1 += 1;
                        },
                        PhaseOutcome::Failed { tick } => {
                            *phase_fail.entry(pid.clone()).or_insert(0) += 1;
                            let entry = phase_tick_sums.entry(pid.clone()).or_insert((0, 0));
                            entry.0 += u64::from(tick);
                            entry.1 += 1;
                        },
                        PhaseOutcome::Detected { tick } => {
                            *phase_detect.entry(pid.clone()).or_insert(0) += 1;
                            let entry = phase_tick_sums.entry(pid.clone()).or_insert((0, 0));
                            entry.0 += u64::from(tick);
                            entry.1 += 1;
                        },
                        PhaseOutcome::Pending | PhaseOutcome::Active => {
                            *phase_not_reached.entry(pid.clone()).or_insert(0) += 1;
                        },
                    }
                }
                if any_success {
                    any_success_runs += 1;
                }
            } else {
                // Run had no report for this chain — treat all phases as not reached.
                for pid in chain.phases.keys() {
                    *phase_not_reached.entry(pid.clone()).or_insert(0) += 1;
                }
            }
        }

        let n_runs = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");
        let mut phase_stats = BTreeMap::new();
        for pid in chain.phases.keys() {
            let s = *phase_success.get(pid).unwrap_or(&0);
            let f = *phase_fail.get(pid).unwrap_or(&0);
            let d = *phase_detect.get(pid).unwrap_or(&0);
            let nr = *phase_not_reached.get(pid).unwrap_or(&0);
            let mean_tick = phase_tick_sums.get(pid).and_then(|(sum, count)| {
                if *count > 0 {
                    Some(*sum as f64 / f64::from(*count))
                } else {
                    None
                }
            });
            // All four rates share the same denominator (`n_runs`), so
            // `wilson_score_interval` returns `Some` for all or none.
            // Computing per-field keeps the point estimate exact (counts,
            // not the round-tripped rate) even at small `n`.
            let ci_95 = match (
                wilson_score_interval(s, n_runs),
                wilson_score_interval(f, n_runs),
                wilson_score_interval(d, n_runs),
                wilson_score_interval(nr, n_runs),
            ) {
                (Some(sw), Some(fw), Some(dw), Some(nrw)) => Some(PhaseStatsCIs {
                    success_rate: ConfidenceInterval::from(sw),
                    failure_rate: ConfidenceInterval::from(fw),
                    detection_rate: ConfidenceInterval::from(dw),
                    not_reached_rate: ConfidenceInterval::from(nrw),
                }),
                _ => None,
            };
            phase_stats.insert(
                pid.clone(),
                PhaseStats {
                    phase_id: pid.clone(),
                    success_rate: f64::from(s) / total,
                    failure_rate: f64::from(f) / total,
                    detection_rate: f64::from(d) / total,
                    not_reached_rate: f64::from(nr) / total,
                    mean_completion_tick: mean_tick,
                    ci_95,
                },
            );
        }

        let mean_attacker_spend = attacker_spend_sum / total;
        let mean_defender_spend = defender_spend_sum / total;
        let cost_asymmetry_ratio = if mean_attacker_spend > 0.0 {
            mean_defender_spend / mean_attacker_spend
        } else {
            0.0
        };

        let phase_survival = time_dynamics::phase_kaplan_meier(runs, chain_id, chain);

        out.insert(
            chain_id.clone(),
            CampaignSummary {
                chain_id: chain_id.clone(),
                phase_stats,
                overall_success_rate: f64::from(any_success_runs) / total,
                detection_rate: f64::from(detected_runs) / total,
                mean_attacker_spend,
                mean_defender_spend,
                cost_asymmetry_ratio,
                mean_attribution_confidence: attribution_sum / total,
                time_to_first_detection: ttd.remove(chain_id),
                defender_reaction_time: react.remove(chain_id),
                phase_survival,
            },
        );
    }

    out
}

/// Compute per-region faction control probability from final states.
fn compute_regional_control(runs: &[RunResult]) -> BTreeMap<RegionId, BTreeMap<FactionId, f64>> {
    let total = runs.len() as f64;
    if total == 0.0 {
        return BTreeMap::new();
    }

    let mut counts: BTreeMap<RegionId, BTreeMap<FactionId, u32>> = BTreeMap::new();

    for run in runs {
        for (rid, controller) in &run.final_state.region_control {
            if let Some(fid) = controller {
                *counts
                    .entry(rid.clone())
                    .or_default()
                    .entry(fid.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|(rid, faction_counts)| {
            let probabilities = faction_counts
                .into_iter()
                .map(|(fid, count)| (fid, f64::from(count) / total))
                .collect();
            (rid, probabilities)
        })
        .collect()
}

/// Compute the probability of each event firing at least once across runs.
fn compute_event_probabilities(runs: &[RunResult]) -> BTreeMap<EventId, f64> {
    let total = runs.len() as f64;
    if total == 0.0 {
        return BTreeMap::new();
    }

    let mut event_run_counts: BTreeMap<EventId, u32> = BTreeMap::new();

    for run in runs {
        // Collect unique events from the complete event log for this run.
        let mut seen_in_run = std::collections::BTreeSet::new();
        for record in &run.event_log {
            seen_in_run.insert(record.event_id.clone());
        }
        for eid in seen_in_run {
            *event_run_counts.entry(eid).or_insert(0) += 1;
        }
    }

    event_run_counts
        .into_iter()
        .map(|(eid, count)| (eid, f64::from(count) / total))
        .collect()
}

// ---------------------------------------------------------------------------
// Distribution helpers
// ---------------------------------------------------------------------------

/// Compute descriptive statistics for a slice of values.
///
/// Returns mean, median, standard deviation, min, max, and
/// 5th/95th percentiles. Returns zeroed stats for empty input.
///
/// When `bootstrap_seed` is `Some`, a 95% percentile-bootstrap CI on
/// the mean is also populated using a fresh `ChaCha8Rng` seeded with
/// that value — deterministic given the seed and values. The test
/// helper [`compute_distribution`] passes `None`; production code
/// calls [`compute_distribution_with_bootstrap`] to fill the CI.
fn compute_distribution_inner(values: &[f64], bootstrap_seed: Option<u64>) -> DistributionStats {
    if values.is_empty() {
        return DistributionStats {
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
            min: 0.0,
            max: 0.0,
            percentile_5: 0.0,
            percentile_95: 0.0,
            bootstrap_ci_mean: None,
        };
    }

    let n = values.len() as f64;
    let mean = values.iter().copied().sum::<f64>() / n;

    let variance = if values.len() > 1 {
        values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    let std_dev = variance.sqrt();

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let min = sorted.first().copied().unwrap_or(0.0);
    let max = sorted.last().copied().unwrap_or(0.0);
    let median = percentile_of_sorted(&sorted, 50.0);
    let percentile_5 = percentile_of_sorted(&sorted, 5.0);
    let percentile_95 = percentile_of_sorted(&sorted, 95.0);

    // 500 resamples: small enough to keep compute_summary well under a
    // millisecond for normal MC sizes, large enough that endpoint
    // percentiles are stable to ~0.01 for samples of 100+.
    let bootstrap_ci_mean = bootstrap_seed.and_then(|seed| {
        percentile_bootstrap_ci_seeded(values, 500, 0.05, seed).map(ConfidenceInterval::from)
    });

    DistributionStats {
        mean,
        median,
        std_dev,
        min,
        max,
        percentile_5,
        percentile_95,
        bootstrap_ci_mean,
    }
}

/// Convenience wrapper: descriptive stats without a bootstrap CI.
/// Only used by in-file tests today; production paths always pass a seed
/// via [`compute_distribution_with_bootstrap`] so the report carries CIs.
#[cfg(test)]
fn compute_distribution(values: &[f64]) -> DistributionStats {
    compute_distribution_inner(values, None)
}

/// Descriptive stats + percentile-bootstrap CI on the mean. Seed is
/// carried from the caller (typically derived from the scenario) so
/// the report remains deterministic under fixed inputs.
fn compute_distribution_with_bootstrap(values: &[f64], seed: u64) -> DistributionStats {
    compute_distribution_inner(values, Some(seed))
}

/// Compute the p-th percentile from a pre-sorted slice using linear
/// interpolation.
fn percentile_of_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }

    let rank = (p / 100.0) * (sorted.len() as f64 - 1.0);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let frac = rank - rank.floor();

    if lower == upper || upper >= sorted.len() {
        sorted[lower.min(sorted.len() - 1)]
    } else {
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_distribution_empty() {
        let stats = compute_distribution(&[]);
        assert!((stats.mean - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_distribution_single_value() {
        let stats = compute_distribution(&[42.0]);
        assert!((stats.mean - 42.0).abs() < f64::EPSILON);
        assert!((stats.median - 42.0).abs() < f64::EPSILON);
        assert!((stats.std_dev - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_distribution_basic() {
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let stats = compute_distribution(&values);

        assert!((stats.mean - 50.5).abs() < 0.01);
        assert!((stats.median - 50.5).abs() < 0.01);
        assert!((stats.min - 1.0).abs() < f64::EPSILON);
        assert!((stats.max - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_of_sorted_interpolates() {
        let sorted = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let p50 = percentile_of_sorted(&sorted, 50.0);
        assert!((p50 - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn monte_carlo_runner_zero_runs_errors() {
        let config = MonteCarloConfig {
            num_runs: 0,
            seed: Some(1),
            collect_snapshots: false,
            parallel: false,
        };
        // Scenario doesn't matter — should fail before touching it.
        let scenario = minimal_scenario();
        let result = MonteCarloRunner::run(&config, &scenario);
        assert!(result.is_err(), "zero runs should produce an error");
        let err_msg = format!("{}", result.expect_err("just checked is_err"));
        assert!(
            err_msg.contains("num_runs"),
            "error should mention num_runs, got: {err_msg}"
        );
    }

    #[test]
    fn monte_carlo_runner_single_run() {
        let config = MonteCarloConfig {
            num_runs: 1,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };
        let scenario = minimal_scenario();
        let result = MonteCarloRunner::run(&config, &scenario).expect("single run should succeed");
        assert_eq!(result.runs.len(), 1, "should have exactly 1 run");
        assert_eq!(result.summary.total_runs, 1, "summary should report 1 run");
        // The run should have a valid final tick > 0 or == max_ticks.
        assert!(
            result.runs[0].final_tick > 0,
            "run should have progressed at least 1 tick"
        );
    }

    #[test]
    fn compute_summary_win_rates() {
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");
        let runs = vec![
            make_run(0, Some(f_gov.clone()), 10, 0.5),
            make_run(1, Some(f_gov.clone()), 12, 0.6),
            make_run(2, Some(f_rebel.clone()), 8, 0.4),
            make_run(3, Some(f_gov.clone()), 15, 0.7),
        ];
        let scenario = minimal_scenario();
        let summary = compute_summary(&runs, &scenario);

        assert_eq!(summary.total_runs, 4);
        let gov_rate = summary
            .win_rates
            .get(&f_gov)
            .copied()
            .expect("gov should have a win rate");
        let rebel_rate = summary
            .win_rates
            .get(&f_rebel)
            .copied()
            .expect("rebel should have a win rate");
        assert!(
            (gov_rate - 0.75).abs() < f64::EPSILON,
            "gov should win 75%, got {gov_rate}"
        );
        assert!(
            (rebel_rate - 0.25).abs() < f64::EPSILON,
            "rebel should win 25%, got {rebel_rate}"
        );
    }

    #[test]
    fn compute_summary_with_stalemates() {
        let f_gov = FactionId::from("gov");
        let runs = vec![
            make_run(0, Some(f_gov.clone()), 10, 0.5),
            make_run(1, None, 20, 0.8), // stalemate
            make_run(2, None, 20, 0.9), // stalemate
            make_run(3, Some(f_gov.clone()), 15, 0.6),
        ];
        let scenario = minimal_scenario();
        let summary = compute_summary(&runs, &scenario);

        assert_eq!(summary.total_runs, 4);
        // Only gov wins — 2 out of 4.
        let gov_rate = summary
            .win_rates
            .get(&f_gov)
            .copied()
            .expect("gov should have a win rate");
        assert!(
            (gov_rate - 0.5).abs() < f64::EPSILON,
            "gov should win 50% with stalemates, got {gov_rate}"
        );
        // Stalemates have no victor, so no other faction in win_rates.
        assert_eq!(
            summary.win_rates.len(),
            1,
            "only one faction should appear in win_rates"
        );
    }

    #[test]
    fn percentile_edge_cases() {
        let sorted = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let p0 = percentile_of_sorted(&sorted, 0.0);
        let p100 = percentile_of_sorted(&sorted, 100.0);
        assert!(
            (p0 - 10.0).abs() < f64::EPSILON,
            "0th percentile should be min, got {p0}"
        );
        assert!(
            (p100 - 50.0).abs() < f64::EPSILON,
            "100th percentile should be max, got {p100}"
        );

        // Single element.
        let single = vec![7.0];
        assert!(
            (percentile_of_sorted(&single, 0.0) - 7.0).abs() < f64::EPSILON,
            "0th percentile of single element"
        );
        assert!(
            (percentile_of_sorted(&single, 100.0) - 7.0).abs() < f64::EPSILON,
            "100th percentile of single element"
        );

        // Empty.
        let empty: Vec<f64> = vec![];
        assert!(
            (percentile_of_sorted(&empty, 50.0) - 0.0).abs() < f64::EPSILON,
            "percentile of empty should be 0.0"
        );
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
    use faultline_types::ids::{ForceId, RegionId, VictoryId};
    use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::strategy::Doctrine;
    use faultline_types::victory::{VictoryCondition, VictoryType};

    fn make_run(index: u32, victor: Option<FactionId>, ticks: u32, tension: f64) -> RunResult {
        use faultline_types::stats::{Outcome, StateSnapshot};
        RunResult {
            run_index: index,
            seed: u64::from(index),
            outcome: Outcome {
                victor,
                victory_condition: None,
                final_tension: tension,
            },
            final_tick: ticks,
            final_state: StateSnapshot {
                tick: ticks,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension,
                events_fired_this_tick: vec![],
            },
            snapshots: vec![],
            event_log: vec![],
            campaign_reports: Default::default(),
            defender_queue_reports: Vec::new(),
            network_reports: std::collections::BTreeMap::new(),
        }
    }

    fn minimal_scenario() -> Scenario {
        let r1 = RegionId::from("region-a");
        let r2 = RegionId::from("region-b");
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");

        let mut regions = BTreeMap::new();
        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "Region A".into(),
                population: 100_000,
                urbanization: 0.5,
                initial_control: Some(f_gov.clone()),
                strategic_value: 5.0,
                borders: vec![r2.clone()],
                centroid: None,
            },
        );
        regions.insert(
            r2.clone(),
            Region {
                id: r2.clone(),
                name: "Region B".into(),
                population: 50_000,
                urbanization: 0.3,
                initial_control: Some(f_rebel.clone()),
                strategic_value: 3.0,
                borders: vec![r1.clone()],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(
            f_gov.clone(),
            make_faction(f_gov.clone(), "Government", r1.clone()),
        );
        factions.insert(
            f_rebel.clone(),
            make_faction(f_rebel.clone(), "Rebels", r2.clone()),
        );

        let mut victory_conditions = BTreeMap::new();
        let vc_id = VictoryId::from("gov-win");
        victory_conditions.insert(
            vc_id.clone(),
            VictoryCondition {
                id: vc_id,
                name: "Government Dominance".into(),
                faction: f_gov,
                condition: VictoryType::MilitaryDominance {
                    enemy_strength_below: 0.01,
                },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "Test Scenario".into(),
                description: "Minimal scenario for testing".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 2,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![
                    TerrainModifier {
                        region: r1,
                        terrain_type: TerrainType::Urban,
                        movement_modifier: 1.0,
                        defense_modifier: 1.0,
                        visibility: 0.8,
                    },
                    TerrainModifier {
                        region: r2,
                        terrain_type: TerrainType::Rural,
                        movement_modifier: 1.0,
                        defense_modifier: 0.8,
                        visibility: 0.9,
                    },
                ],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.5,
                institutional_trust: 0.6,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.7,
                    internet_availability: 0.8,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 10,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(42),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: faultline_types::strategy_space::StrategySpace::default(),
            networks: std::collections::BTreeMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Metric distribution tests
    // -----------------------------------------------------------------------

    use faultline_types::ids::{EventId, InfraId};
    use faultline_types::map::{InfrastructureNode, InfrastructureType};
    use faultline_types::stats::{EventRecord, StateSnapshot};
    use faultline_types::strategy::FactionState;

    fn make_faction_state(
        fid: &FactionId,
        strength: f64,
        morale: f64,
        resources: f64,
    ) -> FactionState {
        FactionState {
            faction_id: fid.clone(),
            morale,
            resources,
            logistics_capacity: 100.0,
            tech_deployed: vec![],
            controlled_regions: vec![],
            total_strength: strength,
            institution_loyalty: BTreeMap::new(),
            current_leadership_rank: 0,
            leadership_decapitations: 0,
            last_decapitation_tick: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_rich_run(
        index: u32,
        victor: Option<FactionId>,
        ticks: u32,
        tension: f64,
        faction_states: BTreeMap<FactionId, FactionState>,
        region_control: BTreeMap<RegionId, Option<FactionId>>,
        infra_status: BTreeMap<InfraId, f64>,
        event_log: Vec<EventRecord>,
    ) -> RunResult {
        use faultline_types::stats::Outcome;
        RunResult {
            run_index: index,
            seed: u64::from(index),
            outcome: Outcome {
                victor,
                victory_condition: None,
                final_tension: tension,
            },
            final_tick: ticks,
            final_state: StateSnapshot {
                tick: ticks,
                faction_states,
                region_control,
                infra_status,
                tension,
                events_fired_this_tick: vec![],
            },
            snapshots: vec![],
            event_log,
            campaign_reports: Default::default(),
            defender_queue_reports: Vec::new(),
            network_reports: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn compute_summary_total_casualties() {
        // Scenario has 2 factions with 100 strength each (200 total).
        let scenario = minimal_scenario();
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");

        // Run 0: gov lost 30, rebel lost 50 → 80 total casualties.
        let mut fs0 = BTreeMap::new();
        fs0.insert(f_gov.clone(), make_faction_state(&f_gov, 70.0, 0.7, 900.0));
        fs0.insert(
            f_rebel.clone(),
            make_faction_state(&f_rebel, 50.0, 0.6, 800.0),
        );

        // Run 1: gov lost 10, rebel lost 90 → 100 total casualties.
        let mut fs1 = BTreeMap::new();
        fs1.insert(f_gov.clone(), make_faction_state(&f_gov, 90.0, 0.8, 950.0));
        fs1.insert(
            f_rebel.clone(),
            make_faction_state(&f_rebel, 10.0, 0.3, 700.0),
        );

        let runs = vec![
            make_rich_run(
                0,
                None,
                10,
                0.5,
                fs0,
                BTreeMap::new(),
                BTreeMap::new(),
                vec![],
            ),
            make_rich_run(
                1,
                Some(f_gov.clone()),
                8,
                0.4,
                fs1,
                BTreeMap::new(),
                BTreeMap::new(),
                vec![],
            ),
        ];

        let summary = compute_summary(&runs, &scenario);
        let casualties = summary
            .metric_distributions
            .get(&MetricType::TotalCasualties)
            .expect("should have TotalCasualties");

        // Mean: (80 + 100) / 2 = 90.
        assert!(
            (casualties.mean - 90.0).abs() < 0.01,
            "mean casualties should be 90, got {}",
            casualties.mean
        );
        assert!(
            (casualties.min - 80.0).abs() < 0.01,
            "min casualties should be 80"
        );
        assert!(
            (casualties.max - 100.0).abs() < 0.01,
            "max casualties should be 100"
        );
    }

    #[test]
    fn compute_summary_zero_casualties() {
        // Both factions at full strength.
        let scenario = minimal_scenario();
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");

        let mut fs = BTreeMap::new();
        fs.insert(
            f_gov.clone(),
            make_faction_state(&f_gov, 100.0, 0.8, 1000.0),
        );
        fs.insert(
            f_rebel.clone(),
            make_faction_state(&f_rebel, 100.0, 0.8, 1000.0),
        );

        let runs = vec![make_rich_run(
            0,
            None,
            10,
            0.5,
            fs,
            BTreeMap::new(),
            BTreeMap::new(),
            vec![],
        )];
        let summary = compute_summary(&runs, &scenario);
        let casualties = summary
            .metric_distributions
            .get(&MetricType::TotalCasualties)
            .expect("should have TotalCasualties");
        assert!(
            casualties.mean.abs() < f64::EPSILON,
            "zero casualties expected"
        );
    }

    #[test]
    fn compute_summary_resources_expended() {
        // Scenario has 2 factions with 1000 resources each (2000 total).
        let scenario = minimal_scenario();
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");

        // Run: gov spent 200, rebel spent 300 → 500 total expended.
        let mut fs = BTreeMap::new();
        fs.insert(f_gov.clone(), make_faction_state(&f_gov, 100.0, 0.8, 800.0));
        fs.insert(
            f_rebel.clone(),
            make_faction_state(&f_rebel, 100.0, 0.8, 700.0),
        );

        let runs = vec![make_rich_run(
            0,
            None,
            10,
            0.5,
            fs,
            BTreeMap::new(),
            BTreeMap::new(),
            vec![],
        )];
        let summary = compute_summary(&runs, &scenario);
        let resources = summary
            .metric_distributions
            .get(&MetricType::ResourcesExpended)
            .expect("should have ResourcesExpended");
        assert!(
            (resources.mean - 500.0).abs() < 0.01,
            "should expend 500 resources, got {}",
            resources.mean
        );
    }

    #[test]
    fn compute_summary_infrastructure_damage() {
        // Build a scenario with infrastructure.
        let mut scenario = minimal_scenario();
        let iid = InfraId::from("power_grid");
        scenario.map.infrastructure.insert(
            iid.clone(),
            InfrastructureNode {
                id: iid.clone(),
                name: "Power Grid".into(),
                region: RegionId::from("region-a"),
                infra_type: InfrastructureType::PowerGrid,
                criticality: 0.9,
                initial_status: 1.0,
                repairable: Some(30),
            },
        );

        // Run: infra dropped from 1.0 to 0.7 → 0.3 damage.
        let mut infra = BTreeMap::new();
        infra.insert(iid, 0.7);

        let runs = vec![make_rich_run(
            0,
            None,
            10,
            0.5,
            BTreeMap::new(),
            BTreeMap::new(),
            infra,
            vec![],
        )];
        let summary = compute_summary(&runs, &scenario);
        let damage = summary
            .metric_distributions
            .get(&MetricType::InfrastructureDamage)
            .expect("should have InfrastructureDamage");
        assert!(
            (damage.mean - 0.3).abs() < 0.01,
            "infra damage should be 0.3, got {}",
            damage.mean
        );
    }

    #[test]
    fn compute_summary_no_infrastructure_skips_metric() {
        // Default minimal scenario has no infrastructure.
        let scenario = minimal_scenario();
        let runs = vec![make_run(0, None, 10, 0.5)];
        let summary = compute_summary(&runs, &scenario);
        assert!(
            !summary
                .metric_distributions
                .contains_key(&MetricType::InfrastructureDamage),
            "should not have InfrastructureDamage when no infra exists"
        );
    }

    #[test]
    fn compute_summary_regional_control() {
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");
        let r1 = RegionId::from("region-a");
        let r2 = RegionId::from("region-b");

        // Run 0: gov controls r1, rebel controls r2.
        let mut rc0 = BTreeMap::new();
        rc0.insert(r1.clone(), Some(f_gov.clone()));
        rc0.insert(r2.clone(), Some(f_rebel.clone()));

        // Run 1: rebel controls both.
        let mut rc1 = BTreeMap::new();
        rc1.insert(r1.clone(), Some(f_rebel.clone()));
        rc1.insert(r2.clone(), Some(f_rebel.clone()));

        // Run 2: gov controls both.
        let mut rc2 = BTreeMap::new();
        rc2.insert(r1.clone(), Some(f_gov.clone()));
        rc2.insert(r2.clone(), Some(f_gov.clone()));

        let scenario = minimal_scenario();
        let runs = vec![
            make_rich_run(
                0,
                None,
                10,
                0.5,
                BTreeMap::new(),
                rc0,
                BTreeMap::new(),
                vec![],
            ),
            make_rich_run(
                1,
                None,
                10,
                0.5,
                BTreeMap::new(),
                rc1,
                BTreeMap::new(),
                vec![],
            ),
            make_rich_run(
                2,
                None,
                10,
                0.5,
                BTreeMap::new(),
                rc2,
                BTreeMap::new(),
                vec![],
            ),
        ];

        let summary = compute_summary(&runs, &scenario);

        // r1: gov=2/3, rebel=1/3.
        let r1_ctrl = summary
            .regional_control
            .get(&r1)
            .expect("should have region-a control");
        let r1_gov = r1_ctrl.get(&f_gov).copied().unwrap_or(0.0);
        let r1_rebel = r1_ctrl.get(&f_rebel).copied().unwrap_or(0.0);
        assert!(
            (r1_gov - 2.0 / 3.0).abs() < 0.01,
            "r1 gov control should be 2/3, got {r1_gov}"
        );
        assert!(
            (r1_rebel - 1.0 / 3.0).abs() < 0.01,
            "r1 rebel control should be 1/3, got {r1_rebel}"
        );

        // r2: rebel=2/3, gov=1/3.
        let r2_ctrl = summary
            .regional_control
            .get(&r2)
            .expect("should have region-b control");
        let r2_rebel = r2_ctrl.get(&f_rebel).copied().unwrap_or(0.0);
        let r2_gov = r2_ctrl.get(&f_gov).copied().unwrap_or(0.0);
        assert!(
            (r2_rebel - 2.0 / 3.0).abs() < 0.01,
            "r2 rebel should be 2/3"
        );
        assert!((r2_gov - 1.0 / 3.0).abs() < 0.01, "r2 gov should be 1/3");
    }

    #[test]
    fn compute_summary_regional_control_uncontrolled_ignored() {
        let r1 = RegionId::from("region-a");
        let mut rc = BTreeMap::new();
        rc.insert(r1.clone(), None); // No controller.

        let scenario = minimal_scenario();
        let runs = vec![make_rich_run(
            0,
            None,
            10,
            0.5,
            BTreeMap::new(),
            rc,
            BTreeMap::new(),
            vec![],
        )];

        let summary = compute_summary(&runs, &scenario);
        // r1 has no controller, so it shouldn't appear (or have empty map).
        let r1_ctrl = summary.regional_control.get(&r1);
        assert!(
            r1_ctrl.is_none() || r1_ctrl.expect("checked some").is_empty(),
            "uncontrolled region should have no faction entries"
        );
    }

    #[test]
    fn compute_summary_event_probabilities() {
        let scenario = minimal_scenario();

        let e_a = EventId::from("event_a");
        let e_b = EventId::from("event_b");

        // Run 0: event_a fires.
        let log0 = vec![EventRecord {
            tick: 5,
            event_id: e_a.clone(),
        }];
        // Run 1: event_a and event_b fire.
        let log1 = vec![
            EventRecord {
                tick: 3,
                event_id: e_a.clone(),
            },
            EventRecord {
                tick: 7,
                event_id: e_b.clone(),
            },
        ];
        // Run 2: no events.
        let log2 = vec![];

        let runs = vec![
            make_rich_run(
                0,
                None,
                10,
                0.5,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                log0,
            ),
            make_rich_run(
                1,
                None,
                10,
                0.5,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                log1,
            ),
            make_rich_run(
                2,
                None,
                10,
                0.5,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                log2,
            ),
        ];

        let summary = compute_summary(&runs, &scenario);

        // event_a fires in 2/3 runs.
        let prob_a = summary
            .event_probabilities
            .get(&e_a)
            .copied()
            .expect("event_a should have probability");
        assert!(
            (prob_a - 2.0 / 3.0).abs() < 0.01,
            "event_a should fire in 2/3 runs, got {prob_a}"
        );

        // event_b fires in 1/3 runs.
        let prob_b = summary
            .event_probabilities
            .get(&e_b)
            .copied()
            .expect("event_b should have probability");
        assert!(
            (prob_b - 1.0 / 3.0).abs() < 0.01,
            "event_b should fire in 1/3 runs, got {prob_b}"
        );
    }

    #[test]
    fn compute_summary_event_probability_deduplication() {
        // Same event fires multiple times in one run — should count as 1 for probability.
        let scenario = minimal_scenario();
        let e_a = EventId::from("event_a");

        let log = vec![
            EventRecord {
                tick: 1,
                event_id: e_a.clone(),
            },
            EventRecord {
                tick: 5,
                event_id: e_a.clone(),
            },
            EventRecord {
                tick: 10,
                event_id: e_a.clone(),
            },
        ];

        let runs = vec![make_rich_run(
            0,
            None,
            10,
            0.5,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            log,
        )];

        let summary = compute_summary(&runs, &scenario);
        let prob = summary
            .event_probabilities
            .get(&e_a)
            .copied()
            .expect("event_a should have probability");
        assert!(
            (prob - 1.0).abs() < f64::EPSILON,
            "event firing 3 times in 1 run should give probability 1.0"
        );
    }

    #[test]
    fn compute_summary_empty_runs_returns_zeroed() {
        let scenario = minimal_scenario();
        let summary = compute_summary(&[], &scenario);
        assert_eq!(summary.total_runs, 0);
        assert!(summary.win_rates.is_empty());
        assert!(summary.regional_control.is_empty());
        assert!(summary.event_probabilities.is_empty());
        assert!(summary.metric_distributions.is_empty());
    }

    #[test]
    fn compute_summary_has_all_expected_metrics() {
        let scenario = minimal_scenario();
        let runs = vec![make_run(0, None, 10, 0.5)];
        let summary = compute_summary(&runs, &scenario);

        assert!(
            summary
                .metric_distributions
                .contains_key(&MetricType::Duration),
            "should have Duration"
        );
        assert!(
            summary
                .metric_distributions
                .contains_key(&MetricType::FinalTension),
            "should have FinalTension"
        );
        assert!(
            summary
                .metric_distributions
                .contains_key(&MetricType::TotalCasualties),
            "should have TotalCasualties"
        );
        assert!(
            summary
                .metric_distributions
                .contains_key(&MetricType::ResourcesExpended),
            "should have ResourcesExpended"
        );
        // InfrastructureDamage is only present when scenario has infrastructure.
        assert!(
            !summary
                .metric_distributions
                .contains_key(&MetricType::InfrastructureDamage),
            "should NOT have InfrastructureDamage without infra"
        );
    }

    fn make_faction(id: FactionId, name: &str, region: RegionId) -> Faction {
        let force_id = ForceId::from(format!("{}-inf", id));
        let mut forces = BTreeMap::new();
        forces.insert(
            force_id.clone(),
            ForceUnit {
                id: force_id,
                name: format!("{name} Infantry"),
                unit_type: UnitType::Infantry,
                region,
                strength: 100.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );
        Faction {
            id,
            name: name.into(),
            faction_type: FactionType::Insurgent,
            description: "Test faction".into(),
            color: "#000000".into(),
            forces,
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 100.0,
            initial_resources: 1000.0,
            resource_rate: 10.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
            leadership: None,
        }
    }
}
