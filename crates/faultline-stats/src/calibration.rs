//! Back-testing calibration against a scenario's `historical_analogue`.
//!
//! This is the foundation of Epic N. A scenario without an analogue has
//! no externally-anchored point of comparison — the engine output may be
//! internally consistent but there's nothing to say it's *right*. When
//! a scenario declares an analogue, this module compares the MC outcome
//! distribution against each declared observation and produces a
//! per-observation verdict plus a rolled-up Pass/Marginal/Fail count.
//!
//! ## What this module is not
//!
//! - It is *not* a prediction claim. A Pass on every observation does
//!   not say the scenario predicts the future — it says the scenario
//!   reproduces a documented past within the declared uncertainty.
//! - It is *not* an automated parameter-fitter. The author's
//!   responsibility is to choose parameters that defensibly model the
//!   precedent; this module just reports whether the resulting MC
//!   matches.
//! - It does not adjudicate the *quality* of the analogue itself. An
//!   author who tags a casual analogy as `confidence = High` will get
//!   the same calibration verdict as an author who tags a meticulously-
//!   sourced analogue as `confidence = High`. The author-flagged source
//!   confidence is surfaced alongside the verdict so the reader can
//!   weight the result accordingly.
//!
//! ## Verdict ladder
//!
//! Each `HistoricalMetric` variant has its own thresholds:
//!
//! | Metric | Pass | Marginal | Fail |
//! | --- | --- | --- | --- |
//! | `Winner` | observed faction is MC modal *and* MC mass ≥ 0.50 | observed faction is MC modal *or* MC mass ≥ 0.25 | otherwise |
//! | `WinRate` | MC point in `[low, high]` | Wilson 95% CI overlaps `[low, high]` | otherwise |
//! | `DurationTicks` | coverage ≥ 0.50 (≥ half of MC runs in interval) | coverage ≥ 0.25 | otherwise |
//!
//! Coverage = fraction of MC runs whose `final_tick` falls in `[low, high]`.
//! The `Winner` ladder treats "right faction, low confidence" and
//! "wrong faction, high confidence" symmetrically as Marginal because
//! both are "calibrated to the same precedent but with caveats."
//!
//! ## Determinism contract
//!
//! Pure function of `(scenario, runs, win_rates)`. No RNG, no
//! `HashMap`. Adding an analogue to a scenario *does* change the
//! manifest content hash because `MonteCarloSummary.calibration` is
//! serialized into the report — that's the intended behavior, since
//! the analogue declaration is part of the scenario's analytical claim.
//! Scenarios without an analogue see no change in summary shape (the
//! field is `#[serde(skip_serializing_if = ...)]` when None).

use std::collections::BTreeMap;

use faultline_types::ids::FactionId;
use faultline_types::scenario::{HistoricalMetric, HistoricalObservation, Scenario};
use faultline_types::stats::{
    CalibrationReport, CalibrationVerdict, ObservationCalibration, RunResult,
};

use crate::uncertainty::wilson_score_interval;

/// Verdict thresholds. Pulled out as constants so a future epic can
/// tune them without rewriting the dispatch logic. The values are
/// deliberately coarse — fine-grained tuning would imply more
/// confidence in the calibration framework than Epic N itself claims.
const WINRATE_MARGINAL_MASS: f64 = 0.25;
const WINRATE_PASS_MASS: f64 = 0.50;
const COVERAGE_MARGINAL: f64 = 0.25;
const COVERAGE_PASS: f64 = 0.50;

/// Compute the calibration verdict for a scenario's MC run set.
///
/// Returns `None` when the scenario declares no `historical_analogue`
/// — the report section then renders a "purely synthetic" disclaimer
/// rather than a verdict. Returns `Some` with one row per observation
/// when the analogue is present.
///
/// `win_rates` is taken as a parameter rather than recomputed because
/// `compute_summary` already has it in scope; sharing the value avoids
/// re-iterating the run set and keeps the two derivations in lockstep.
pub fn compute_calibration(
    scenario: &Scenario,
    runs: &[RunResult],
    win_rates: &BTreeMap<FactionId, f64>,
) -> Option<CalibrationReport> {
    let analogue = scenario.meta.historical_analogue.as_ref()?;

    let n_runs = u32::try_from(runs.len()).unwrap_or(u32::MAX);
    let modal_winner = compute_modal_winner(win_rates);

    let observations: Vec<ObservationCalibration> = analogue
        .observations
        .iter()
        .map(|obs| evaluate_observation(obs, runs, win_rates, n_runs, modal_winner.as_ref()))
        .collect();

    let overall = roll_up_verdict(&observations);

    Some(CalibrationReport {
        analogue_name: analogue.name.clone(),
        observations,
        overall,
    })
}

/// The faction with the largest MC win rate, ties broken
/// deterministically by `BTreeMap` iteration order (lexicographic
/// largest `FactionId` wins on a tie because `Iterator::max_by` keeps
/// the *last* maximum). Returns `None` when no faction ever won (every
/// MC run was a stalemate).
///
/// NaN handling: any NaN win rate compares as `Less` than every finite
/// value, so a NaN entry can never become the modal winner. Win rates
/// are produced by integer-count division upstream and should never be
/// NaN in practice; this guard exists so a future producer-side bug
/// can't silently flip the modal-winner verdict to an arbitrary
/// last-iterated faction.
fn compute_modal_winner(win_rates: &BTreeMap<FactionId, f64>) -> Option<FactionId> {
    win_rates
        .iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less))
        .map(|(fid, _)| fid.clone())
}

fn evaluate_observation(
    obs: &HistoricalObservation,
    runs: &[RunResult],
    win_rates: &BTreeMap<FactionId, f64>,
    n_runs: u32,
    modal_winner: Option<&FactionId>,
) -> ObservationCalibration {
    let (label, mc_summary, verdict) = match &obs.metric {
        HistoricalMetric::Winner { faction } => {
            evaluate_winner(faction, win_rates, n_runs, modal_winner)
        },
        HistoricalMetric::WinRate { faction, low, high } => {
            evaluate_win_rate(faction, *low, *high, win_rates, n_runs)
        },
        HistoricalMetric::DurationTicks { low, high } => evaluate_duration(*low, *high, runs),
    };

    ObservationCalibration {
        label,
        mc_summary,
        source_confidence: obs.confidence.clone(),
        verdict,
        notes: obs.notes.clone(),
    }
}

fn evaluate_winner(
    observed: &FactionId,
    win_rates: &BTreeMap<FactionId, f64>,
    n_runs: u32,
    modal_winner: Option<&FactionId>,
) -> (String, String, CalibrationVerdict) {
    let label = format!("winner = {observed}");
    let observed_rate = win_rates.get(observed).copied().unwrap_or(0.0);
    let is_modal = modal_winner == Some(observed);

    let mc_summary = if let Some(modal) = modal_winner {
        if is_modal {
            format!("{observed} wins {:.1}% (MC modal)", observed_rate * 100.0)
        } else {
            let modal_rate = win_rates.get(modal).copied().unwrap_or(0.0);
            format!(
                "{observed} wins {:.1}%; MC modal is {modal} at {:.1}%",
                observed_rate * 100.0,
                modal_rate * 100.0
            )
        }
    } else {
        format!(
            "{observed} wins {:.1}%; every MC run was a stalemate",
            observed_rate * 100.0
        )
    };

    // Verdict: Pass when the MC reproduces the observed winner with
    // majority mass; Marginal when the observed faction is the modal
    // winner OR mass crosses the marginal threshold (covers "right
    // faction, low confidence" and "wrong faction, high confidence"
    // symmetrically); Fail otherwise. n_runs == 0 collapses to Fail
    // because there's nothing to calibrate against.
    let verdict = if n_runs == 0 {
        CalibrationVerdict::Fail
    } else if is_modal && observed_rate >= WINRATE_PASS_MASS {
        CalibrationVerdict::Pass
    } else if is_modal || observed_rate >= WINRATE_MARGINAL_MASS {
        CalibrationVerdict::Marginal
    } else {
        CalibrationVerdict::Fail
    };

    (label, mc_summary, verdict)
}

fn evaluate_win_rate(
    faction: &FactionId,
    low: f64,
    high: f64,
    win_rates: &BTreeMap<FactionId, f64>,
    n_runs: u32,
) -> (String, String, CalibrationVerdict) {
    let label = format!(
        "win_rate({faction}) ∈ [{:.1}%, {:.1}%]",
        low * 100.0,
        high * 100.0
    );
    let observed_rate = win_rates.get(faction).copied().unwrap_or(0.0);

    // Wilson CI uses the count, not the rate, so convert back. Capping
    // at n_runs handles the edge case of a faction whose win-rate
    // produces a fractional count due to floating-point round-trip.
    let count = (observed_rate * f64::from(n_runs)).round() as u32;
    let count = count.min(n_runs);
    let ci = wilson_score_interval(count, n_runs);

    let mc_summary = match &ci {
        Some(w) => format!(
            "{faction} wins {:.1}% [Wilson 95% CI {:.1}% – {:.1}%]",
            observed_rate * 100.0,
            w.lower * 100.0,
            w.upper * 100.0
        ),
        None => format!("{faction} wins {:.1}% (n_runs = 0)", observed_rate * 100.0),
    };

    let verdict = if n_runs == 0 {
        CalibrationVerdict::Fail
    } else if observed_rate >= low && observed_rate <= high {
        CalibrationVerdict::Pass
    } else if let Some(w) = ci.as_ref() {
        // Wilson CI overlaps the historical interval iff the lower
        // bound is ≤ historical high and the upper bound ≥
        // historical low.
        if w.lower <= high && w.upper >= low {
            CalibrationVerdict::Marginal
        } else {
            CalibrationVerdict::Fail
        }
    } else {
        CalibrationVerdict::Fail
    };

    (label, mc_summary, verdict)
}

fn evaluate_duration(
    low: u64,
    high: u64,
    runs: &[RunResult],
) -> (String, String, CalibrationVerdict) {
    let label = format!("duration_ticks ∈ [{low}, {high}]");

    if runs.is_empty() {
        return (label, "n_runs = 0".to_string(), CalibrationVerdict::Fail);
    }

    let mut in_range = 0u32;
    let mut sum = 0u64;
    let mut sum_sq = 0u128;
    for r in runs {
        let t = u64::from(r.final_tick);
        if t >= low && t <= high {
            in_range += 1;
        }
        sum += t;
        sum_sq += u128::from(t) * u128::from(t);
    }
    let n = runs.len() as f64;
    let coverage = f64::from(in_range) / n;
    let mean = sum as f64 / n;
    let variance = if runs.len() > 1 {
        // Sample variance (n-1 denominator) over the MC run set,
        // computed from the running sums to avoid a second pass.
        // Matches the distribution-stats convention used in
        // compute_distribution_inner.
        let sum_sq_f = sum_sq as f64;
        let mean_sq_n = mean * mean * n;
        ((sum_sq_f - mean_sq_n) / (n - 1.0)).max(0.0)
    } else {
        0.0
    };
    let std_dev = variance.sqrt();

    let mc_summary = format!(
        "coverage {:.0}% ({in_range}/{}); MC mean {:.1}, σ {:.1}",
        coverage * 100.0,
        runs.len(),
        mean,
        std_dev
    );

    let verdict = if coverage >= COVERAGE_PASS {
        CalibrationVerdict::Pass
    } else if coverage >= COVERAGE_MARGINAL {
        CalibrationVerdict::Marginal
    } else {
        CalibrationVerdict::Fail
    };

    (label, mc_summary, verdict)
}

/// Roll-up rule: the overall verdict is the *worst* per-observation
/// verdict. A scenario with three Pass observations and one Fail rolls
/// up to Fail — calibration claims are ANDs, not ORs. Scenarios with
/// zero observations roll up to Fail (validation should already have
/// rejected this shape, but the calibration module stays defensive
/// rather than panicking).
fn roll_up_verdict(observations: &[ObservationCalibration]) -> CalibrationVerdict {
    if observations.is_empty() {
        return CalibrationVerdict::Fail;
    }
    let mut worst = CalibrationVerdict::Pass;
    for obs in observations {
        worst = match (worst, obs.verdict) {
            (CalibrationVerdict::Fail, _) | (_, CalibrationVerdict::Fail) => {
                CalibrationVerdict::Fail
            },
            (CalibrationVerdict::Marginal, _) | (_, CalibrationVerdict::Marginal) => {
                CalibrationVerdict::Marginal
            },
            _ => CalibrationVerdict::Pass,
        };
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::FactionId;
    use faultline_types::scenario::{HistoricalAnalogue, HistoricalMetric, HistoricalObservation};
    use faultline_types::stats::{ConfidenceLevel, Outcome, StateSnapshot};

    fn run_with(victor: Option<FactionId>, ticks: u32) -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: ticks,
            final_state: StateSnapshot {
                tick: ticks,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.5,
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
            narrative_events: Vec::new(),
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: BTreeMap::new(),
        }
    }

    fn analogue_with(observations: Vec<HistoricalObservation>) -> HistoricalAnalogue {
        HistoricalAnalogue {
            name: "Test Analogue".into(),
            description: "test".into(),
            period: "test".into(),
            sources: vec!["unit-test".into()],
            confidence: Some(ConfidenceLevel::Medium),
            observations,
        }
    }

    fn observation(metric: HistoricalMetric) -> HistoricalObservation {
        HistoricalObservation {
            metric,
            confidence: Some(ConfidenceLevel::Medium),
            notes: String::new(),
        }
    }

    fn scenario_with(analogue: Option<HistoricalAnalogue>) -> Scenario {
        let mut s = Scenario::default();
        s.meta.historical_analogue = analogue;
        s
    }

    #[test]
    fn no_analogue_returns_none() {
        let scenario = scenario_with(None);
        let runs = vec![run_with(Some(FactionId::from("a")), 5)];
        let win_rates = BTreeMap::from([(FactionId::from("a"), 1.0)]);
        assert!(compute_calibration(&scenario, &runs, &win_rates).is_none());
    }

    #[test]
    fn winner_pass_when_observed_is_modal_with_majority_mass() {
        let blue = FactionId::from("blue");
        let red = FactionId::from("red");
        let analogue = analogue_with(vec![observation(HistoricalMetric::Winner {
            faction: blue.clone(),
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs = vec![run_with(Some(blue.clone()), 5); 10];
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue.clone(), 0.8);
        win_rates.insert(red.clone(), 0.2);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations.len(), 1);
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Pass);
        assert_eq!(report.overall, CalibrationVerdict::Pass);
    }

    #[test]
    fn winner_marginal_when_observed_is_modal_below_pass_mass() {
        let blue = FactionId::from("blue");
        let red = FactionId::from("red");
        let analogue = analogue_with(vec![observation(HistoricalMetric::Winner {
            faction: blue.clone(),
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs = vec![run_with(Some(blue.clone()), 5)];
        let mut win_rates = BTreeMap::new();
        // blue is modal but only at 30% — Marginal because it's the
        // top finisher but not by a confident majority.
        win_rates.insert(blue, 0.30);
        win_rates.insert(red, 0.20);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Marginal);
    }

    #[test]
    fn winner_fail_when_observed_is_distant_from_modal() {
        let blue = FactionId::from("blue");
        let red = FactionId::from("red");
        let analogue = analogue_with(vec![observation(HistoricalMetric::Winner {
            faction: blue.clone(),
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs = vec![run_with(Some(red.clone()), 5)];
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 0.10);
        win_rates.insert(red, 0.85);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Fail);
    }

    #[test]
    fn win_rate_pass_when_point_in_interval() {
        let blue = FactionId::from("blue");
        let analogue = analogue_with(vec![observation(HistoricalMetric::WinRate {
            faction: blue.clone(),
            low: 0.5,
            high: 0.7,
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs: Vec<_> = (0..100).map(|_| run_with(Some(blue.clone()), 5)).collect();
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 0.6);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Pass);
    }

    #[test]
    fn win_rate_marginal_when_wilson_overlaps() {
        let blue = FactionId::from("blue");
        let analogue = analogue_with(vec![observation(HistoricalMetric::WinRate {
            faction: blue.clone(),
            low: 0.5,
            high: 0.7,
        })]);
        let scenario = scenario_with(Some(analogue));
        // 30 runs, 12 wins → point estimate 0.40. Wilson CI at n=30
        // for p=0.4 is roughly [0.244, 0.581], which still overlaps
        // [0.5, 0.7] on the upper end. Marginal.
        let runs: Vec<_> = (0..30).map(|_| run_with(Some(blue.clone()), 5)).collect();
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 0.40);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Marginal);
    }

    #[test]
    fn win_rate_fail_when_no_overlap() {
        let blue = FactionId::from("blue");
        let analogue = analogue_with(vec![observation(HistoricalMetric::WinRate {
            faction: blue.clone(),
            low: 0.5,
            high: 0.7,
        })]);
        let scenario = scenario_with(Some(analogue));
        // 1000 runs, 50 wins → point 0.05. Wilson at n=1000 narrow
        // enough that the CI doesn't reach 0.5. Fail.
        let runs: Vec<_> = (0..1000).map(|_| run_with(Some(blue.clone()), 5)).collect();
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 0.05);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Fail);
    }

    #[test]
    fn duration_pass_when_majority_in_interval() {
        let analogue = analogue_with(vec![observation(HistoricalMetric::DurationTicks {
            low: 5,
            high: 12,
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs: Vec<_> = (5..15).map(|t| run_with(None, t)).collect();
        // Ticks 5..14 → 8 of 10 in [5, 12] = 0.80 coverage. Pass.
        let win_rates = BTreeMap::new();

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Pass);
    }

    #[test]
    fn duration_marginal_at_exactly_threshold() {
        let analogue = analogue_with(vec![observation(HistoricalMetric::DurationTicks {
            low: 1,
            high: 4,
        })]);
        let scenario = scenario_with(Some(analogue));
        // 10 runs at ticks 1..10 → 4 of 10 in [1, 4] = 0.40 coverage.
        // Above the 0.25 marginal threshold but below the 0.50 pass.
        let runs: Vec<_> = (1..11).map(|t| run_with(None, t)).collect();
        let win_rates = BTreeMap::new();

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Marginal);
    }

    #[test]
    fn duration_fail_when_no_runs_in_range() {
        let analogue = analogue_with(vec![observation(HistoricalMetric::DurationTicks {
            low: 100,
            high: 200,
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs: Vec<_> = (1..11).map(|t| run_with(None, t)).collect();
        let win_rates = BTreeMap::new();

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Fail);
    }

    #[test]
    fn overall_verdict_is_worst_per_observation() {
        let blue = FactionId::from("blue");
        // One Pass + one Fail → overall Fail.
        let analogue = analogue_with(vec![
            observation(HistoricalMetric::Winner {
                faction: blue.clone(),
            }),
            observation(HistoricalMetric::DurationTicks {
                low: 100,
                high: 200,
            }),
        ]);
        let scenario = scenario_with(Some(analogue));
        let runs: Vec<_> = (1..11).map(|_| run_with(Some(blue.clone()), 5)).collect();
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 1.0);

        let report = compute_calibration(&scenario, &runs, &win_rates).expect("analogue present");
        assert_eq!(report.observations[0].verdict, CalibrationVerdict::Pass);
        assert_eq!(report.observations[1].verdict, CalibrationVerdict::Fail);
        assert_eq!(report.overall, CalibrationVerdict::Fail);
    }

    #[test]
    fn determinism_same_input_same_output() {
        // Pure-function contract: identical inputs produce identical
        // CalibrationReports. Compares JSON because the report nests
        // strings + verdict enums and field-by-field comparison would
        // miss a future field addition.
        let blue = FactionId::from("blue");
        let analogue = analogue_with(vec![observation(HistoricalMetric::Winner {
            faction: blue.clone(),
        })]);
        let scenario = scenario_with(Some(analogue));
        let runs: Vec<_> = (0..5).map(|_| run_with(Some(blue.clone()), 7)).collect();
        let mut win_rates = BTreeMap::new();
        win_rates.insert(blue, 1.0);

        let a = compute_calibration(&scenario, &runs, &win_rates).expect("analogue");
        let b = compute_calibration(&scenario, &runs, &win_rates).expect("analogue");
        assert_eq!(
            serde_json::to_string(&a).expect("serialize a"),
            serde_json::to_string(&b).expect("serialize b")
        );
    }
}
