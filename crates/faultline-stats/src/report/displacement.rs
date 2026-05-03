//! Displacement Flows section (Epic D round-three item 4 — refugee /
//! displacement flows).
//!
//! Surfaces per-region displacement analytics across the run set so
//! analysts can see which regions absorbed the worst displacement
//! pressure, how much the displaced population churned through
//! adjacent regions, and how much settled back into the resident
//! population. Pairs with the engine-side displacement phase that
//! propagates displaced fractions across `Region.borders` and absorbs
//! a fraction back into population each tick.
//!
//! Elided when `summary.displacement_summaries` is empty — i.e. no run
//! had a non-zero displaced fraction in any region.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct Displacement;

impl ReportSection for Displacement {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.displacement_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Displacement Flows");
        let _ = writeln!(
            out,
            "Per-region refugee / displacement analytics from the displacement phase. Sources: scripted `EventEffect::Displacement` and civilian-segment `Flee` actions. Each tick, a fraction of the displaced population in each region propagates to adjacent regions (10% / tick split evenly) and a fraction absorbs back into the resident population (5% / tick). The cumulative inflow / outflow / absorbed flows describe the *churn* a region experienced; the peak captures worst-case stress; the terminal value reports whether the region was still under stress at run end."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Region | Stressed runs | Mean peak | Max peak | Mean terminal | Mean total inflow | Mean total outflow |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");
        for row in summary.displacement_summaries.values() {
            let _ = writeln!(
                out,
                "| `{}` | {}/{} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |",
                escape_md_cell(&row.region.0),
                row.stressed_runs,
                row.n_runs,
                row.mean_peak,
                row.max_peak,
                row.mean_terminal,
                row.mean_total_inflow,
                row.mean_total_outflow,
            );
        }
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::RegionId;
    use faultline_types::stats::DisplacementSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_empty() {
        let mut out = String::new();
        Displacement.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(out.is_empty(), "should elide on empty");
    }

    #[test]
    fn renders_with_one_region() {
        let mut summary = empty_summary();
        let region = RegionId::from("downtown");
        summary.displacement_summaries.insert(
            region.clone(),
            DisplacementSummary {
                region,
                n_runs: 10,
                stressed_runs: 7,
                mean_peak: 0.25,
                max_peak: 0.40,
                mean_terminal: 0.10,
                mean_total_inflow: 0.50,
                mean_total_outflow: 0.30,
            },
        );
        let mut out = String::new();
        Displacement.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Displacement Flows"));
        assert!(out.contains("`downtown`"));
        assert!(out.contains("7/10"));
    }

    #[test]
    fn determines_peak_columns() {
        let mut summary = empty_summary();
        let region = RegionId::from("downtown");
        summary.displacement_summaries.insert(
            region.clone(),
            DisplacementSummary {
                region,
                n_runs: 1,
                stressed_runs: 1,
                mean_peak: 0.5,
                max_peak: 0.5,
                mean_terminal: 0.5,
                mean_total_inflow: 0.5,
                mean_total_outflow: 0.0,
            },
        );
        let mut out = String::new();
        Displacement.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("0.500"));
    }
}
