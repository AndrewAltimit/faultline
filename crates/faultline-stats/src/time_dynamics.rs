//! Time & attribution dynamics post-processing.
//!
//! All functions here operate on already-collected
//! [`RunResult`](faultline_types::stats::RunResult) data — they never
//! re-run the engine and never touch RNG state, so output is a pure
//! function of the input runs.
//!
//! Three families of analytics live in this module:
//!
//! * Time-to-first-detection per kill chain
//!   ([`time_to_first_detection`]) — distribution of ticks from
//!   start-of-run to the first phase that flipped to
//!   `PhaseOutcome::Detected`. Right-censored at the run's terminal
//!   tick when the chain was never detected.
//! * Defender exposure / reaction time per kill chain
//!   ([`defender_reaction_time`]) — gap between the first detection
//!   event and the run's terminal tick. Captures the post-detection
//!   runway the operation kept.
//! * Per-phase Kaplan-Meier survival ([`phase_kaplan_meier`]) — `S(t)`
//!   that the phase is still pending at tick `t`, with right-censoring
//!   for runs that ended before reaching the phase.
//!
//! Output structures live on
//! [`MonteCarloSummary`](faultline_types::stats::MonteCarloSummary) and
//! [`CampaignSummary`](faultline_types::stats::CampaignSummary). The
//! report renderer surfaces the summaries; this module produces them.

use std::collections::BTreeMap;

use faultline_types::campaign::KillChain;
use faultline_types::ids::{KillChainId, PhaseId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    CorrelationMatrix, DefenderReactionTime, DistributionStats, KaplanMeierCurve, ParetoFrontier,
    ParetoPoint, PhaseOutcome, RunResult, TimeToFirstDetection,
};

// ---------------------------------------------------------------------------
// Time-to-first-detection
// ---------------------------------------------------------------------------

/// Compute time-to-first-detection per kill chain.
///
/// For each chain, scans every run's campaign report for the earliest
/// `PhaseOutcome::Detected { tick }`. Runs where the defender was never
/// alerted contribute to `right_censored` only — they do *not* land in
/// `samples`. Returns an entry only for chains that had at least one
/// run produce a campaign report (i.e. chains that the engine actually
/// initialized).
pub fn time_to_first_detection(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<KillChainId, TimeToFirstDetection> {
    let mut out = BTreeMap::new();
    for chain_id in scenario.kill_chains.keys() {
        let mut samples = Vec::new();
        let mut censored = 0u32;
        let mut had_report = false;
        for run in runs {
            let report = match run.campaign_reports.get(chain_id) {
                Some(r) => r,
                None => continue,
            };
            had_report = true;
            match first_detection_tick(report) {
                Some(t) => samples.push(t),
                None => censored = censored.saturating_add(1),
            }
        }
        if !had_report {
            continue;
        }
        samples.sort_unstable();
        let stats = if samples.is_empty() {
            None
        } else {
            let as_f64: Vec<f64> = samples.iter().map(|t| f64::from(*t)).collect();
            Some(distribution_stats(&as_f64))
        };
        let detected_runs = u32::try_from(samples.len()).unwrap_or(u32::MAX);
        out.insert(
            chain_id.clone(),
            TimeToFirstDetection {
                detected_runs,
                right_censored: censored,
                samples,
                stats,
            },
        );
    }
    out
}

/// First detection tick from a campaign report, if any phase was detected.
fn first_detection_tick(report: &faultline_types::stats::CampaignReport) -> Option<u32> {
    report
        .phase_outcomes
        .values()
        .filter_map(|o| match o {
            PhaseOutcome::Detected { tick } => Some(*tick),
            _ => None,
        })
        .min()
}

// ---------------------------------------------------------------------------
// Defender reaction time
// ---------------------------------------------------------------------------

/// Compute defender exposure time per kill chain.
///
/// For runs where the chain was detected, the sample is `final_tick -
/// first_detection_tick`. Non-detected runs are skipped (they have no
/// reaction-time signal — the defender never had to react). Returns
/// `None` for chains where no run was ever alerted.
pub fn defender_reaction_time(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<KillChainId, DefenderReactionTime> {
    let mut out = BTreeMap::new();
    for chain_id in scenario.kill_chains.keys() {
        let mut samples = Vec::new();
        for run in runs {
            let report = match run.campaign_reports.get(chain_id) {
                Some(r) => r,
                None => continue,
            };
            if let Some(t_det) = first_detection_tick(report) {
                // Defensive `saturating_sub` even though `t_det <=
                // final_tick` should hold by construction: a phase
                // detected at tick `T` means the engine produced
                // `Detected { tick: T }` no later than the loop's
                // termination at `final_tick`. A logic bug elsewhere
                // shouldn't surface as a panic here.
                let gap = run.final_tick.saturating_sub(t_det);
                samples.push(gap);
            }
        }
        if samples.is_empty() {
            continue;
        }
        samples.sort_unstable();
        let as_f64: Vec<f64> = samples.iter().map(|t| f64::from(*t)).collect();
        let detected_runs = u32::try_from(samples.len()).unwrap_or(u32::MAX);
        out.insert(
            chain_id.clone(),
            DefenderReactionTime {
                detected_runs,
                samples,
                stats: Some(distribution_stats(&as_f64)),
            },
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Kaplan-Meier survival
// ---------------------------------------------------------------------------

/// Compute per-phase Kaplan-Meier survival curves for a single chain.
///
/// "Event" = the phase reached a terminal status (Succeeded / Failed /
/// Detected). "Right-censored" = the run ended with the phase still
/// pending or active (or no campaign report produced for that run).
///
/// The estimator follows the textbook product-limit form: at each
/// distinct event time `t_i`, `S(t_i) = S(t_{i-1}) * (1 - d_i / n_i)`,
/// where `d_i` is the number of events at `t_i` and `n_i` is the
/// at-risk set just before `t_i`. Censored observations leave the
/// at-risk set without producing an event. `cumulative_hazard = -ln(S)`.
pub fn phase_kaplan_meier(
    runs: &[RunResult],
    chain_id: &KillChainId,
    chain: &KillChain,
) -> BTreeMap<PhaseId, KaplanMeierCurve> {
    let mut out = BTreeMap::new();
    for pid in chain.phases.keys() {
        let mut events: Vec<u32> = Vec::new();
        let mut censored: Vec<u32> = Vec::new();
        for run in runs {
            let report = match run.campaign_reports.get(chain_id) {
                Some(r) => r,
                None => {
                    // Run produced no report for this chain — treat as
                    // censored at the run's terminal tick. This matches
                    // the existing `compute_campaign_summaries` behavior
                    // of counting these as "not reached."
                    censored.push(run.final_tick);
                    continue;
                },
            };
            let outcome = report
                .phase_outcomes
                .get(pid)
                .cloned()
                .unwrap_or(PhaseOutcome::Pending);
            match outcome {
                PhaseOutcome::Succeeded { tick }
                | PhaseOutcome::Failed { tick }
                | PhaseOutcome::Detected { tick } => events.push(tick),
                PhaseOutcome::Pending | PhaseOutcome::Active => censored.push(run.final_tick),
            }
        }
        if events.is_empty() && censored.is_empty() {
            continue;
        }
        out.insert(pid.clone(), kaplan_meier_curve(&events, &censored));
    }
    out
}

/// Build a Kaplan-Meier curve from raw event / censored times.
///
/// Both inputs may be unsorted; this routine sorts internally and
/// merges them on a shared time axis. Ties between an event and a
/// censoring at the same tick are handled by the standard convention
/// (events occur *before* censoring at the same time).
fn kaplan_meier_curve(events: &[u32], censored: &[u32]) -> KaplanMeierCurve {
    let total = events.len() + censored.len();
    if total == 0 {
        return KaplanMeierCurve {
            times: Vec::new(),
            survival: Vec::new(),
            cumulative_hazard: Vec::new(),
            events: Vec::new(),
            at_risk: Vec::new(),
            censored: 0,
        };
    }

    // Group events and censorings by their time stamp so that ties
    // resolve correctly: at a tied tick, the event count is `d_i` and
    // the censored count exits the risk set *after* the event step.
    let mut event_counts: BTreeMap<u32, u32> = BTreeMap::new();
    for &t in events {
        *event_counts.entry(t).or_insert(0) += 1;
    }
    let mut censor_counts: BTreeMap<u32, u32> = BTreeMap::new();
    for &t in censored {
        *censor_counts.entry(t).or_insert(0) += 1;
    }

    let mut all_times: Vec<u32> = event_counts
        .keys()
        .chain(censor_counts.keys())
        .copied()
        .collect();
    all_times.sort_unstable();
    all_times.dedup();

    let mut at_risk = u32::try_from(total).unwrap_or(u32::MAX);
    let mut survival = 1.0_f64;

    let mut times_out = Vec::new();
    let mut survival_out = Vec::new();
    let mut hazard_out = Vec::new();
    let mut events_out = Vec::new();
    let mut at_risk_out = Vec::new();

    for t in &all_times {
        let d = *event_counts.get(t).unwrap_or(&0);
        let c = *censor_counts.get(t).unwrap_or(&0);

        // Only record a step in the curve when an event happened —
        // pure censoring times don't move S(t). We still account for
        // them by removing from the at-risk set after the event step.
        if d > 0 {
            // Edge case: at_risk == 0 should never occur if the input
            // bookkeeping is consistent. Guard anyway so a logic bug
            // upstream doesn't produce NaN survival.
            if at_risk > 0 {
                survival *= 1.0 - f64::from(d) / f64::from(at_risk);
            }
            // Numerical guard: floating-point drift can push survival
            // below 0 by a sub-ulp; clamp to keep `-ln(S)` finite.
            survival = survival.clamp(0.0, 1.0);
            times_out.push(*t);
            survival_out.push(survival);
            // Cumulative hazard is undefined at S = 0 (-ln 0 = +inf).
            // Use `None` rather than `f64::INFINITY` so the value
            // round-trips through JSON; `serde_json` would otherwise
            // serialize `INFINITY` as `null` and then fail to
            // deserialize it back into `f64`.
            hazard_out.push(if survival > 0.0 {
                Some(-survival.ln())
            } else {
                None
            });
            events_out.push(d);
            at_risk_out.push(at_risk);
        }

        // Remove all subjects (events + censorings) from the at-risk
        // set for the *next* time step.
        at_risk = at_risk.saturating_sub(d).saturating_sub(c);
    }

    let total_censored = u32::try_from(censored.len()).unwrap_or(u32::MAX);

    KaplanMeierCurve {
        times: times_out,
        survival: survival_out,
        cumulative_hazard: hazard_out,
        events: events_out,
        at_risk: at_risk_out,
        censored: total_censored,
    }
}

// ---------------------------------------------------------------------------
// Pareto frontier
// ---------------------------------------------------------------------------

/// Build the Pareto frontier across runs over (attacker_cost, success,
/// stealth).
///
/// Returns `None` when fewer than two runs exist (a Pareto frontier of
/// one point is degenerate) or when no scenario kill chains exist (no
/// per-run scalars to project to).
pub fn pareto_frontier(runs: &[RunResult], scenario: &Scenario) -> Option<ParetoFrontier> {
    if runs.len() < 2 || scenario.kill_chains.is_empty() {
        return None;
    }
    let chain_count = scenario.kill_chains.len() as f64;

    let points: Vec<ParetoPoint> = runs
        .iter()
        .map(|run| {
            let mut attacker_cost = 0.0_f64;
            let mut succeeded_chains = 0u32;
            // Stealth = 1 - max(detection_accumulation across chains).
            // `detection_accumulation` is `1 - prod(1 - p_i)` so it's
            // already a probability in [0, 1] — the right granularity
            // for "how visible was this run."
            let mut max_detection = 0.0_f64;
            for report in run.campaign_reports.values() {
                attacker_cost += report.attacker_spend;
                let any_succeeded = report
                    .phase_outcomes
                    .values()
                    .any(|o| matches!(o, PhaseOutcome::Succeeded { .. }));
                if any_succeeded {
                    succeeded_chains += 1;
                }
                let chain_max = report
                    .detection_accumulation
                    .values()
                    .copied()
                    .fold(0.0_f64, f64::max);
                if chain_max > max_detection {
                    max_detection = chain_max;
                }
            }
            let success = f64::from(succeeded_chains) / chain_count;
            let stealth = (1.0 - max_detection).clamp(0.0, 1.0);
            ParetoPoint {
                run_index: run.run_index,
                attacker_cost,
                success,
                stealth,
            }
        })
        .collect();

    // Brute-force O(n²) dominance check — frontier sizes are bounded by
    // MC run counts (a few hundred at most for normal use), so the
    // simple algorithm is plenty.
    let mut frontier: Vec<ParetoPoint> = Vec::new();
    for (i, candidate) in points.iter().enumerate() {
        let dominated = points.iter().enumerate().any(|(j, other)| {
            if i == j {
                return false;
            }
            // Other dominates candidate iff other is no worse on every
            // axis and strictly better on at least one. "Better"
            // direction: cost lower, success higher, stealth higher.
            let no_worse = other.attacker_cost <= candidate.attacker_cost
                && other.success >= candidate.success
                && other.stealth >= candidate.stealth;
            let strictly_better = other.attacker_cost < candidate.attacker_cost
                || other.success > candidate.success
                || other.stealth > candidate.stealth;
            no_worse && strictly_better
        });
        if !dominated {
            frontier.push(candidate.clone());
        }
    }
    frontier.sort_by(|a, b| a.attacker_cost.total_cmp(&b.attacker_cost));

    Some(ParetoFrontier {
        points: frontier,
        total_runs: u32::try_from(runs.len()).unwrap_or(u32::MAX),
    })
}

// ---------------------------------------------------------------------------
// Output-output correlation matrix
// ---------------------------------------------------------------------------

/// Compute a Pearson correlation matrix over per-run scalar outputs.
///
/// The columns are a fixed list — duration, total casualties, total
/// attacker spend, total defender spend, mean attribution confidence,
/// max chain detection accumulation. Returns `None` when fewer than
/// two runs exist (correlation undefined). Entries where one of the
/// two series has zero variance are `None` (undefined correlation)
/// rather than silently zeroed — see [`CorrelationMatrix`] for the
/// JSON-safety rationale.
pub fn output_correlation_matrix(
    runs: &[RunResult],
    scenario: &Scenario,
) -> Option<CorrelationMatrix> {
    if runs.len() < 2 {
        return None;
    }
    let initial_total_strength: f64 = scenario
        .factions
        .values()
        .flat_map(|f| f.forces.values())
        .map(|u| u.strength)
        .sum();

    let labels: Vec<&'static str> = vec![
        "duration",
        "total_casualties",
        "attacker_spend",
        "defender_spend",
        "mean_attribution",
        "max_detection",
    ];
    let n_runs = runs.len();
    let n_cols = labels.len();
    let mut series: Vec<Vec<f64>> = vec![Vec::with_capacity(n_runs); n_cols];

    for run in runs {
        series[0].push(f64::from(run.final_tick));
        let final_strength: f64 = run
            .final_state
            .faction_states
            .values()
            .map(|fs| fs.total_strength)
            .sum();
        series[1].push((initial_total_strength - final_strength).max(0.0));
        let mut atk = 0.0_f64;
        let mut def = 0.0_f64;
        let mut attribution = 0.0_f64;
        let mut chain_count = 0_u32;
        let mut max_det = 0.0_f64;
        for report in run.campaign_reports.values() {
            atk += report.attacker_spend;
            def += report.defender_spend;
            attribution += report.attribution_confidence;
            chain_count = chain_count.saturating_add(1);
            let chain_max = report
                .detection_accumulation
                .values()
                .copied()
                .fold(0.0_f64, f64::max);
            if chain_max > max_det {
                max_det = chain_max;
            }
        }
        series[2].push(atk);
        series[3].push(def);
        series[4].push(if chain_count > 0 {
            attribution / f64::from(chain_count)
        } else {
            0.0
        });
        series[5].push(max_det);
    }

    // Guard against degenerate scenarios where every series is constant
    // (e.g. zero-chain scenarios will give all-zero columns 2-5). The
    // matrix is still returned — callers can detect "all-None matrix"
    // and elide the section in their renderer rather than us deciding
    // here. `Option<f64>` instead of NaN keeps the matrix JSON-safe;
    // see [`CorrelationMatrix`] for the rationale.
    let mut values: Vec<Option<f64>> = vec![None; n_cols * n_cols];
    for i in 0..n_cols {
        for j in 0..n_cols {
            values[i * n_cols + j] = pearson(&series[i], &series[j]);
        }
    }

    Some(CorrelationMatrix {
        labels: labels.iter().map(|s| (*s).to_string()).collect(),
        values,
        n: u32::try_from(n_runs).unwrap_or(u32::MAX),
    })
}

/// Pearson correlation. Returns `None` if either input has zero
/// variance — surfacing the missing relationship rather than implying
/// "perfectly uncorrelated."
fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    debug_assert_eq!(a.len(), b.len(), "pearson inputs must be same length");
    if a.len() < 2 {
        return None;
    }
    let n = a.len() as f64;
    let mean_a = a.iter().copied().sum::<f64>() / n;
    let mean_b = b.iter().copied().sum::<f64>() / n;
    let mut cov = 0.0_f64;
    let mut var_a = 0.0_f64;
    let mut var_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let dx = *x - mean_a;
        let dy = *y - mean_b;
        cov += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    let denom = (var_a * var_b).sqrt();
    if denom <= 0.0 || !denom.is_finite() {
        return None;
    }
    Some(cov / denom)
}

// ---------------------------------------------------------------------------
// DistributionStats helper (reused without bootstrap CI)
// ---------------------------------------------------------------------------

fn distribution_stats(values: &[f64]) -> DistributionStats {
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
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let min = *sorted.first().unwrap_or(&0.0);
    let max = *sorted.last().unwrap_or(&0.0);
    DistributionStats {
        mean,
        median: pct(&sorted, 50.0),
        std_dev: variance.sqrt(),
        min,
        max,
        percentile_5: pct(&sorted, 5.0),
        percentile_95: pct(&sorted, 95.0),
        bootstrap_ci_mean: None,
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
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
    use std::collections::BTreeMap;

    use faultline_types::ids::{FactionId, KillChainId, PhaseId};
    use faultline_types::stats::{CampaignReport, Outcome, RunResult, StateSnapshot};

    fn make_report(
        chain: &str,
        phase_outcomes: Vec<(&str, PhaseOutcome)>,
        defender_alerted: bool,
    ) -> (KillChainId, CampaignReport) {
        let cid = KillChainId::from(chain);
        let mut po = BTreeMap::new();
        for (pid, oc) in phase_outcomes {
            po.insert(PhaseId::from(pid), oc);
        }
        (
            cid.clone(),
            CampaignReport {
                chain_id: cid,
                phase_outcomes: po,
                detection_accumulation: BTreeMap::new(),
                defender_alerted,
                attacker_spend: 0.0,
                defender_spend: 0.0,
                attribution_confidence: 0.0,
                information_dominance: 0.0,
                institutional_erosion: 0.0,
                coercion_pressure: 0.0,
                political_cost: 0.0,
            },
        )
    }

    fn make_run(
        run_index: u32,
        final_tick: u32,
        reports: Vec<(KillChainId, CampaignReport)>,
    ) -> RunResult {
        let mut campaign_reports = BTreeMap::new();
        for (cid, r) in reports {
            campaign_reports.insert(cid, r);
        }
        RunResult {
            run_index,
            seed: u64::from(run_index),
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick,
            final_state: StateSnapshot {
                tick: final_tick,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: vec![],
            },
            snapshots: vec![],
            event_log: vec![],
            campaign_reports,
            defender_queue_reports: Vec::new(),
            network_reports: std::collections::BTreeMap::new(),
            fracture_events: Vec::new(),
            supply_pressure_reports: std::collections::BTreeMap::new(),
            civilian_activations: Vec::new(),
            tech_costs: std::collections::BTreeMap::new(),
            narrative_events: Vec::new(),
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: std::collections::BTreeMap::new(),
            utility_decisions: BTreeMap::new(),
            belief_accuracy: ::std::collections::BTreeMap::new(),
            belief_snapshots: ::std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn first_detection_finds_earliest_detected_phase() {
        let (_, r) = make_report(
            "alpha",
            vec![
                ("a", PhaseOutcome::Succeeded { tick: 2 }),
                ("b", PhaseOutcome::Detected { tick: 7 }),
                ("c", PhaseOutcome::Detected { tick: 5 }),
            ],
            true,
        );
        assert_eq!(first_detection_tick(&r), Some(5));
    }

    #[test]
    fn first_detection_none_for_undetected() {
        let (_, r) = make_report(
            "alpha",
            vec![("a", PhaseOutcome::Succeeded { tick: 1 })],
            false,
        );
        assert_eq!(first_detection_tick(&r), None);
    }

    #[test]
    fn kaplan_meier_two_events_no_censoring() {
        // Two runs both have an event: at tick 2 and tick 5.
        // n=2 throughout. S(2) = 1 * (1 - 1/2) = 0.5; S(5) = 0.5 * (1 - 1/1) = 0.
        let curve = kaplan_meier_curve(&[2, 5], &[]);
        assert_eq!(curve.times, vec![2, 5]);
        assert!((curve.survival[0] - 0.5).abs() < 1e-12);
        assert!((curve.survival[1] - 0.0).abs() < 1e-12);
        assert_eq!(curve.events, vec![1, 1]);
        assert_eq!(curve.at_risk, vec![2, 1]);
        assert_eq!(curve.censored, 0);
    }

    #[test]
    fn kaplan_meier_with_censoring_does_not_step() {
        // 3 subjects: event at t=3, censored at t=4, event at t=6.
        // n at t=3 is 3 → S(3) = 2/3. After censoring n drops to 1.
        // Event at t=6 with n=1 → S(6) = 0.
        let curve = kaplan_meier_curve(&[3, 6], &[4]);
        assert_eq!(curve.times, vec![3, 6]);
        assert!((curve.survival[0] - 2.0 / 3.0).abs() < 1e-12);
        assert!((curve.survival[1] - 0.0).abs() < 1e-12);
        assert_eq!(curve.censored, 1);
    }

    #[test]
    fn kaplan_meier_all_censored_yields_empty_curve() {
        let curve = kaplan_meier_curve(&[], &[1, 2, 3]);
        assert!(curve.times.is_empty());
        assert_eq!(curve.censored, 3);
    }

    #[test]
    fn cumulative_hazard_matches_neg_log_survival() {
        let curve = kaplan_meier_curve(&[2, 5, 10], &[7]);
        for (s, h) in curve.survival.iter().zip(curve.cumulative_hazard.iter()) {
            match (s, h) {
                (s, Some(h)) if *s > 0.0 => {
                    assert!((h + s.ln()).abs() < 1e-12, "H = -ln(S) violated");
                },
                (s, None) => {
                    assert!(*s == 0.0, "None hazard must coincide with S=0, got {s}");
                },
                (s, Some(h)) => panic!("S={s} > 0 but hazard = Some({h}) — must be -ln(S)"),
            }
        }
    }

    #[test]
    fn pearson_perfect_positive_correlation_is_one() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![2.0, 4.0, 6.0, 8.0];
        let r = pearson(&a, &b).expect("non-degenerate series should produce a value");
        assert!((r - 1.0).abs() < 1e-12);
    }

    #[test]
    fn pearson_perfect_negative_correlation_is_minus_one() {
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![4.0, 3.0, 2.0, 1.0];
        let r = pearson(&a, &b).expect("non-degenerate series should produce a value");
        assert!((r + 1.0).abs() < 1e-12);
    }

    #[test]
    fn pearson_zero_variance_is_none() {
        let a = vec![1.0, 1.0, 1.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(
            pearson(&a, &b),
            None,
            "constant series must produce None (undefined correlation)"
        );
    }

    #[test]
    fn time_to_first_detection_handles_mix() {
        use faultline_types::campaign::{CampaignPhase, KillChain as KC, PhaseCost};
        use faultline_types::map::{MapConfig, MapSource};
        use faultline_types::politics::{MediaLandscape, PoliticalClimate};
        use faultline_types::scenario::{Scenario, ScenarioMeta};
        use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};

        let cid = KillChainId::from("alpha");
        let pid = PhaseId::from("a");
        let mut phases = BTreeMap::new();
        phases.insert(
            pid.clone(),
            CampaignPhase {
                id: pid.clone(),
                name: "A".into(),
                description: "".into(),
                prerequisites: vec![],
                base_success_probability: 0.5,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.1,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        let mut chains = BTreeMap::new();
        chains.insert(
            cid.clone(),
            KC {
                id: cid.clone(),
                name: "Alpha".into(),
                description: "".into(),
                attacker: FactionId::from("red"),
                target: FactionId::from("blue"),
                entry_phase: pid.clone(),
                phases,
            },
        );
        let scenario = Scenario {
            meta: ScenarioMeta {
                name: "t".into(),
                description: "".into(),
                author: "".into(),
                version: "0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
                historical_analogue: None,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions: BTreeMap::new(),
                infrastructure: BTreeMap::new(),
                terrain: vec![],
            },
            factions: BTreeMap::new(),
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.0,
                institutional_trust: 0.5,
                media_landscape: MediaLandscape {
                    fragmentation: 0.0,
                    disinformation_susceptibility: 0.0,
                    state_control: 0.0,
                    social_media_penetration: 0.0,
                    internet_availability: 0.0,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 10,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 3,
                seed: Some(0),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
                belief_model: None,
            },
            victory_conditions: BTreeMap::new(),
            kill_chains: chains,
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: faultline_types::strategy_space::StrategySpace::default(),
            networks: std::collections::BTreeMap::new(),
        };

        // Run 0: detected at tick 3.
        let r0 = make_run(
            0,
            10,
            vec![make_report(
                "alpha",
                vec![("a", PhaseOutcome::Detected { tick: 3 })],
                true,
            )],
        );
        // Run 1: detected at tick 6.
        let r1 = make_run(
            1,
            10,
            vec![make_report(
                "alpha",
                vec![("a", PhaseOutcome::Detected { tick: 6 })],
                true,
            )],
        );
        // Run 2: never detected (succeeded).
        let r2 = make_run(
            2,
            10,
            vec![make_report(
                "alpha",
                vec![("a", PhaseOutcome::Succeeded { tick: 4 })],
                false,
            )],
        );
        let runs = vec![r0, r1, r2];
        let out = time_to_first_detection(&runs, &scenario);
        let entry = out.get(&KillChainId::from("alpha")).expect("entry");
        assert_eq!(entry.detected_runs, 2);
        assert_eq!(entry.right_censored, 1);
        assert_eq!(entry.samples, vec![3, 6]);
        let stats = entry.stats.as_ref().expect("stats");
        assert!((stats.mean - 4.5).abs() < 1e-12);

        let react = defender_reaction_time(&runs, &scenario);
        let r_entry = react.get(&KillChainId::from("alpha")).expect("react entry");
        assert_eq!(r_entry.detected_runs, 2);
        // Reaction times: 10 - 3 = 7, 10 - 6 = 4.
        assert_eq!(r_entry.samples, vec![4, 7]);
    }
}
