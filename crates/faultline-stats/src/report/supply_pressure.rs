//! Supply Pressure section (Epic D round three, item 2 — supply-network
//! interdiction).
//!
//! Surfaces per-faction supply-pressure aggregates: the typical
//! operating supply level (mean of per-run means), the typical
//! worst-case dip (mean of per-run minima), the run-set worst case
//! (`worst_min`), and the duration of meaningful stress
//! (mean pressured ticks). Pairs with the engine-side supply phase
//! that scales income by the residual capacity of each owned
//! `kind = "supply"` network.
//!
//! Elided when `summary.supply_pressure_summaries` is empty — i.e.
//! the scenario declared no `kind = "supply"` networks (or every
//! supply network's owner was eliminated before any tick of attrition
//! ran on them).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct SupplyPressure;

impl ReportSection for SupplyPressure {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.supply_pressure_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Supply Pressure");
        let _ = writeln!(
            out,
            "Per-faction supply-pressure aggregates across the Monte Carlo batch. *Pressure* is the per-tick ratio of residual to baseline capacity across every `kind = \"supply\"` network the faction owns; `1.0` means supply is intact, `0.0` means supply is fully cut. Income is multiplied by this value each tick, so a sustained dip translates directly into resource starvation. Upkeep is **not** attenuated — units still consume regardless of whether resupply is reaching them, which is exactly why cut supply lines bite."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Faction | Runs | Mean pressure | Mean min | Worst min | Mean pressured ticks | Runs under stress |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");

        for row in summary.supply_pressure_summaries.values() {
            // `n_runs >= 1` for any entry by construction in
            // `compute_supply_pressure_summaries` (the same `or_insert`
            // that creates an `Acc` increments `n_runs`).
            debug_assert!(row.n_runs > 0);
            let stress_rate = f64::from(row.runs_with_any_pressure) / f64::from(row.n_runs);
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.2} | {:.2} | {:.2} | {:.1} | {:.0}% ({}/{}) |",
                escape_md_cell(&row.faction.0),
                row.n_runs,
                row.mean_of_means,
                row.mean_of_mins,
                row.worst_min,
                row.mean_pressured_ticks,
                stress_rate * 100.0,
                row.runs_with_any_pressure,
                row.n_runs,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Reading the table: `Mean pressure` reports the typical operating supply level — a value near `1.0` means most ticks ran intact. `Worst min` is the single deepest dip observed across the batch — useful for sizing how bad a tail-event interdiction can get. `Runs under stress` is the fraction of runs where pressure ever fell below the engine's reporting threshold (currently 0.9), separating *severity* (`Worst min`) from *frequency* (this column)."
        );
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::SupplyPressureSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_supply_summaries() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        SupplyPressure.render(&summary, &scenario, &mut out);
        assert!(
            out.is_empty(),
            "should elide when no supply summaries; got: {out}"
        );
    }

    #[test]
    fn renders_per_faction_row() {
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let blue = FactionId::from("blue");
        sums.insert(
            blue.clone(),
            SupplyPressureSummary {
                faction: blue,
                n_runs: 10,
                mean_of_means: 0.85,
                mean_of_mins: 0.65,
                worst_min: 0.30,
                mean_pressured_ticks: 12.5,
                runs_with_any_pressure: 7,
            },
        );
        summary.supply_pressure_summaries = sums;
        let mut out = String::new();
        SupplyPressure.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Supply Pressure"));
        assert!(out.contains("`blue`"));
        assert!(out.contains("0.85"));
        assert!(out.contains("0.30"));
        assert!(out.contains("70%"), "stress rate should render: {out}");
    }

    #[test]
    fn renders_pristine_run_with_full_pressure() {
        // A run-set where supply was never interdicted should still
        // render — the analyst gets a "yes, I know about your supply
        // network and it was fine" signal rather than the section
        // disappearing.
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let blue = FactionId::from("blue");
        sums.insert(
            blue.clone(),
            SupplyPressureSummary {
                faction: blue,
                n_runs: 5,
                mean_of_means: 1.0,
                mean_of_mins: 1.0,
                worst_min: 1.0,
                mean_pressured_ticks: 0.0,
                runs_with_any_pressure: 0,
            },
        );
        summary.supply_pressure_summaries = sums;
        let mut out = String::new();
        SupplyPressure.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Supply Pressure"));
        assert!(out.contains("0% (0/5)"));
    }
}
