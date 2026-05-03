//! Defender Capacity section: per-role investigative-queue
//! analytics — utilization, dropped alerts, shadow detections, time-
//! to-saturation distribution.
//!
//! Elided when no faction declares `defender_capacities`.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct DefenderCapacity;

impl ReportSection for DefenderCapacity {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.defender_capacity.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Defender Capacity");
        let _ = writeln!(
            out,
            "Per-role investigative-queue analytics across the {} runs in the batch. Utilization is mean-depth / capacity; shadow detections are detection rolls suppressed by saturation (the defender would have caught the operation at idle but missed it under load).",
            summary.total_runs
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Faction | Role | Capacity | Mean util. | Max util. | Mean dropped | Mean shadow det. | Saturated runs |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
        for q in &summary.defender_capacity {
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | {:.1}% | {:.1}% | {:.1} | {:.2} | {}/{} |",
                q.faction,
                q.role,
                q.capacity,
                q.mean_utilization * 100.0,
                q.max_utilization * 100.0,
                q.mean_dropped,
                q.mean_shadow_detections,
                q.time_to_saturation.saturated_runs,
                q.n_runs,
            );
        }
        let _ = writeln!(out);
        // Time-to-saturation distribution per role. Right-censored: runs
        // that never saturated do not appear in the descriptive stats.
        for q in &summary.defender_capacity {
            let Some(stats) = q.time_to_saturation.stats.as_ref() else {
                continue;
            };
            let _ = writeln!(
                out,
                "**`{}` / `{}` time-to-saturation:** {} of {} runs saturated; mean {:.1} ticks (5th–95th percentile {:.1}–{:.1}).",
                q.faction,
                q.role,
                q.time_to_saturation.saturated_runs,
                q.n_runs,
                stats.mean,
                stats.percentile_5,
                stats.percentile_95,
            );
        }
        if summary
            .defender_capacity
            .iter()
            .any(|q| q.time_to_saturation.stats.is_some())
        {
            let _ = writeln!(out);
        }
        // Spillover sub-section (Epic D round-three item 3 — multi-
        // front resource contention). Renders only when at least one
        // role observed cross-role escalation, so legacy single-queue
        // scenarios elide it. The "in" / "out" pair lets the analyst
        // audit chain conservation by inspection — a chain that
        // terminated cleanly should have its leaf role's
        // `spillover_in` add up to the sum of upstream `spillover_out`.
        let any_spillover = summary
            .defender_capacity
            .iter()
            .any(|q| q.mean_spillover_in > 0.0 || q.mean_spillover_out > 0.0);
        if any_spillover {
            let _ = writeln!(
                out,
                "### Cross-role escalation\n\nMean items per run that arrived via or were redirected through `overflow_to` chains. `In` is inbound spillover from a saturated upstream role; `out` is outbound spillover this role redirected to its own escalation target. A non-leaf role with `out > 0` should have a downstream sibling whose `in` accounts for the escalation."
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "| Faction | Role | Mean spillover in | Mean spillover out |"
            );
            let _ = writeln!(out, "|---|---|---|---|");
            for q in &summary.defender_capacity {
                if q.mean_spillover_in == 0.0 && q.mean_spillover_out == 0.0 {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "| `{}` | `{}` | {:.2} | {:.2} |",
                    q.faction, q.role, q.mean_spillover_in, q.mean_spillover_out,
                );
            }
            let _ = writeln!(out);
        }
    }
}
