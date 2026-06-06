//! Force Projection section (standoff-strike reach).
//!
//! Surfaces per-attacker standoff-strike aggregates: how often a
//! projection-bearing force connected a strike on a hostile force in a
//! region beyond its own, and how much strength it removed. Pairs with
//! the engine-side force-projection phase that lets a unit declaring
//! `ForceProjection::StandoffStrike` apply damage within its range
//! (mapped to region-adjacency hops) without moving.
//!
//! Elided when `summary.force_projection_summaries` is empty — i.e. no
//! scenario unit declared `force_projection`, or none ever connected a
//! strike (no in-range hostile force during the run).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct ForceProjection;

impl ReportSection for ForceProjection {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.force_projection_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Force Projection");
        let _ = writeln!(
            out,
            "Per-attacker standoff-strike aggregates across the Monte Carlo batch. A unit declaring a standoff-strike *reach* can remove strength from a hostile force in a region within range of its own — along region-border adjacency — without moving into it. The strike honours the same alliance coupling as co-located combat (a unit will not strike a sworn ally) and inflicts no return attrition on the firing unit."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Attacker | Runs with strikes | Mean strikes/run | Mean strength removed/run | Worst-case strength removed |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|");

        for row in summary.force_projection_summaries.values() {
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.1} | {:.1} | {:.1} |",
                escape_md_cell(&row.attacker.0),
                row.runs_with_strikes,
                row.mean_strikes_per_run,
                row.mean_strength_removed_per_run,
                row.max_strength_removed,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Reading the table: `Mean strikes/run` counts individual (region, target) strike applications averaged over the runs where the attacker connected at least one. `Worst-case strength removed` is the single deepest run-total observed across the batch — useful for sizing the tail effect of a standoff-strike posture."
        );
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::ForceProjectionSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_force_projection_summaries() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        ForceProjection.render(&summary, &scenario, &mut out);
        assert!(
            out.is_empty(),
            "should elide when no force-projection summaries; got: {out}"
        );
    }

    #[test]
    fn renders_per_attacker_row() {
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let red = FactionId::from("red");
        sums.insert(
            red.clone(),
            ForceProjectionSummary {
                attacker: red,
                runs_with_strikes: 8,
                mean_strikes_per_run: 3.5,
                mean_strength_removed_per_run: 140.0,
                max_strength_removed: 300.0,
            },
        );
        summary.force_projection_summaries = sums;
        let mut out = String::new();
        ForceProjection.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Force Projection"));
        assert!(out.contains("`red`"));
        assert!(out.contains("3.5"));
        assert!(out.contains("300.0"));
    }
}
