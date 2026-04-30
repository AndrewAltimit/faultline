//! Pareto Frontier section (Epic C): non-dominated runs across attacker
//! cost / success / stealth.
//!
//! Elided when no frontier was computed or the frontier is empty.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct ParetoFrontier;

impl ReportSection for ParetoFrontier {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let frontier = match summary.pareto_frontier.as_ref() {
            Some(f) if !f.points.is_empty() => f,
            _ => return,
        };
        let _ = writeln!(out, "## Pareto Frontier (cost · success · stealth)");
        let _ = writeln!(
            out,
            "Non-dominated runs across all {} runs in the batch. A run is on the frontier when no other run beat it on every axis simultaneously. Use this to identify the *envelope* of achievable trade-offs before reaching for a sensitivity sweep — runs *behind* the frontier had no realised advantage on any axis.",
            frontier.total_runs
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Run | Attacker cost | Success rate | Stealth (1 − max detection) |"
        );
        let _ = writeln!(out, "|---|---|---|---|");
        for p in &frontier.points {
            let _ = writeln!(
                out,
                "| `{}` | ${:.0} | {:.1}% | {:.1}% |",
                p.run_index,
                p.attacker_cost,
                p.success * 100.0,
                p.stealth * 100.0
            );
        }
        let _ = writeln!(out);
    }
}
