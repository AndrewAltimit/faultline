//! Time & Attribution Dynamics section (Epic C):
//! per-chain time-to-first-detection (right-censored), defender-
//! reaction-time distribution, per-phase Kaplan-Meier survival curves
//! with cumulative hazard.
//!
//! Elided when no chain produces any signal.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{CampaignSummary, MonteCarloSummary};

use super::ReportSection;

pub(super) struct TimeDynamics;

impl ReportSection for TimeDynamics {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let any_ttd = summary
            .campaign_summaries
            .values()
            .any(|cs| cs.time_to_first_detection.is_some());
        let any_react = summary
            .campaign_summaries
            .values()
            .any(|cs| cs.defender_reaction_time.is_some());
        let any_km = summary
            .campaign_summaries
            .values()
            .any(|cs| !cs.phase_survival.is_empty());
        if !any_ttd && !any_react && !any_km {
            return;
        }
        let _ = writeln!(out, "## Time & Attribution Dynamics");
        let _ = writeln!(
            out,
            "Per-chain timing of the first defender alert, the post-detection runway the operation kept, and Kaplan-Meier survival curves for each phase. Detection times are right-censored when the defender was never alerted in a run — those runs sit in the `censored` column and do *not* contribute to the mean. Reaction time = `final_tick - first_detection_tick`; longer means the defender saw the operation but had no time to interrupt it."
        );
        let _ = writeln!(out);

        if any_ttd || any_react {
            let _ = writeln!(
                out,
                "| Chain | Detected runs | Censored | TTD mean | TTD p5 | TTD p95 | Reaction mean | Reaction p5 | Reaction p95 |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
            for (cid, cs) in &summary.campaign_summaries {
                let (det, cen, tm, tp5, tp95) = match &cs.time_to_first_detection {
                    Some(ttd) => {
                        let (m, p5, p95) = match &ttd.stats {
                            Some(s) => (
                                format!("{:.1}", s.mean),
                                format!("{:.1}", s.percentile_5),
                                format!("{:.1}", s.percentile_95),
                            ),
                            None => ("—".into(), "—".into(), "—".into()),
                        };
                        (
                            ttd.detected_runs.to_string(),
                            ttd.right_censored.to_string(),
                            m,
                            p5,
                            p95,
                        )
                    },
                    None => ("—".into(), "—".into(), "—".into(), "—".into(), "—".into()),
                };
                let (rm, rp5, rp95) = match &cs.defender_reaction_time {
                    Some(rt) => match &rt.stats {
                        Some(s) => (
                            format!("{:.1}", s.mean),
                            format!("{:.1}", s.percentile_5),
                            format!("{:.1}", s.percentile_95),
                        ),
                        None => ("—".into(), "—".into(), "—".into()),
                    },
                    None => ("—".into(), "—".into(), "—".into()),
                };
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
                    cid, det, cen, tm, tp5, tp95, rm, rp5, rp95
                );
            }
            let _ = writeln!(out);
        }

        if any_km {
            for (cid, cs) in &summary.campaign_summaries {
                if cs.phase_survival.is_empty() {
                    continue;
                }
                let _ = writeln!(out, "### `{}` — phase survival (Kaplan-Meier)", cid);
                let _ = writeln!(
                    out,
                    "| Phase | n events | Censored | S(median tick) | S(p90 tick) | Median time-to-event |"
                );
                let _ = writeln!(out, "|---|---|---|---|---|---|");
                render_phase_km_rows(out, cs);
                let _ = writeln!(out);
            }
            let _ = writeln!(
                out,
                "_S(t) is the probability the phase is still pending at tick `t`, with right-censoring for runs that ended without reaching the phase. Median time-to-event is the smallest tick where `S` first dropped to ≤ 0.5; `—` means it never did (most runs censored)._"
            );
            let _ = writeln!(out);
        }
    }
}

fn render_phase_km_rows(out: &mut String, cs: &CampaignSummary) {
    for (pid, curve) in &cs.phase_survival {
        let n_events: u32 = curve.events.iter().sum();
        // Median tick of the run distribution serves as a representative
        // probe point for `S`. Hand-pick one rather than sample many to
        // keep rows compact.
        let median_tick = if curve.times.is_empty() {
            None
        } else {
            curve.times.get(curve.times.len() / 2).copied()
        };
        let p90_tick = if curve.times.is_empty() {
            None
        } else {
            // 90th-percentile event tick — the right tail of the curve.
            let idx = ((curve.times.len() as f64 - 1.0) * 0.9).round() as usize;
            curve.times.get(idx).copied()
        };
        let s_at_median = match median_tick {
            Some(t) => surv_at(curve, t),
            None => "—".into(),
        };
        let s_at_p90 = match p90_tick {
            Some(t) => surv_at(curve, t),
            None => "—".into(),
        };
        let median_event_time = curve
            .survival
            .iter()
            .position(|s| *s <= 0.5)
            .and_then(|i| curve.times.get(i))
            .map(|t| format!("{} ticks", t))
            .unwrap_or_else(|| "—".into());
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            pid, n_events, curve.censored, s_at_median, s_at_p90, median_event_time
        );
    }
}

fn surv_at(curve: &faultline_types::stats::KaplanMeierCurve, t: u32) -> String {
    // Right-continuous step function: S is held constant between event
    // times; the value at `t` is `S(t_i)` for the largest `t_i <= t`.
    let mut s = 1.0_f64;
    for (i, ti) in curve.times.iter().enumerate() {
        if *ti <= t {
            s = curve.survival[i];
        } else {
            break;
        }
    }
    format!("{:.2}", s)
}
