//! Tech-Card Costs section.
//!
//! Surfaces per-faction tech-card cost activity: typical spend
//! (deployment + maintenance per run), how often deployment was
//! denied because the faction couldn't afford its `tech_access`
//! roster, and how often deployed cards collapsed mid-run from
//! maintenance starvation. Pairs with the engine's deployment-cost
//! deduction at init, the per-tick maintenance deduction in the
//! attrition phase, and the per-tick coverage gate in the combat
//! phase (the gate is field-internal — no separate per-faction
//! aggregate is needed since coverage is a within-tick limiter, not
//! a cumulative cost).
//!
//! Elided when `summary.tech_cost_summaries` is empty — i.e. no
//! faction's `tech_access` roster ever exercised the cost mechanic
//! (zero `deployment_cost`, zero `cost_per_tick`, no denials, no
//! decommissions). Legacy scenarios with all-zero tech costs see no
//! change in their report.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct TechCosts;

impl ReportSection for TechCosts {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.tech_cost_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Tech-Card Costs");
        let _ = writeln!(
            out,
            "Per-faction tech-card cost activity across the Monte Carlo batch. Each card declares a one-time `deployment_cost` charged at engine init and a per-tick `cost_per_tick` charged in the attrition phase. Cards the faction couldn't afford at init are *denied* (skipped in `tech_access` order); deployed cards that later run out of resource cover are *decommissioned* and contribute nothing for the rest of the run. The optional per-card `coverage_limit` caps how many (region, opponent) pairs the card touches per tick — that gate is enforced in combat but doesn't surface as a separate cost row here."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Faction | Runs | Mean deploy spend | Mean upkeep spend | Mean total | Denial rate | Decommission rate | Mean decom/run |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");

        for row in summary.tech_cost_summaries.values() {
            // `n_runs >= 1` for any row built by
            // `compute_tech_cost_summaries`, but `MonteCarloSummary`
            // is `Deserialize` so an externally-supplied row could
            // arrive with `n_runs = 0` and produce `NaN%` rates. Skip
            // it in that case rather than emitting garbage.
            if row.n_runs == 0 {
                continue;
            }
            let denial_rate = f64::from(row.runs_with_denial) / f64::from(row.n_runs);
            let decom_rate = f64::from(row.runs_with_decommission) / f64::from(row.n_runs);
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.2} | {:.2} | {:.2} | {:.0}% ({}/{}) | {:.0}% ({}/{}) | {:.2} |",
                escape_md_cell(&row.faction.0),
                row.n_runs,
                row.mean_deployment_spend,
                row.mean_maintenance_spend,
                row.mean_total_spend,
                denial_rate * 100.0,
                row.runs_with_denial,
                row.n_runs,
                decom_rate * 100.0,
                row.runs_with_decommission,
                row.n_runs,
                row.mean_decommissions_per_run,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Reading the table: a non-zero **Denial rate** signals the faction's `tech_access` roster exceeds what its `initial_resources` can cover at deploy time — either the roster is over-spec'd or the author meant to force a hard choice between cards. A non-zero **Decommission rate** signals the faction's `resource_rate` (after upkeep and any supply-pressure attenuation) can't sustain the active tech burn rate; cards lost mid-run never re-deploy. **Mean total** is *deployment + maintenance* — comparing it to the faction's run-end resources gives the share of total spend that went to tech."
        );
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::TechCostSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_tech_cost_summaries() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        TechCosts.render(&summary, &scenario, &mut out);
        assert!(
            out.is_empty(),
            "should elide when no tech-cost summaries; got: {out}"
        );
    }

    #[test]
    fn renders_per_faction_row() {
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let red = FactionId::from("red");
        sums.insert(
            red.clone(),
            TechCostSummary {
                faction: red,
                n_runs: 10,
                mean_deployment_spend: 50.0,
                mean_maintenance_spend: 12.0,
                mean_total_spend: 62.0,
                runs_with_denial: 3,
                runs_with_decommission: 2,
                mean_decommissions_per_run: 0.4,
            },
        );
        summary.tech_cost_summaries = sums;
        let mut out = String::new();
        TechCosts.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Tech-Card Costs"));
        assert!(out.contains("`red`"));
        assert!(out.contains("50.00"));
        assert!(
            out.contains("30% (3/10)"),
            "denial rate should render: {out}"
        );
        assert!(
            out.contains("20% (2/10)"),
            "decommission rate should render: {out}"
        );
    }

    #[test]
    fn renders_zero_cost_run_with_only_denials() {
        // A run-set where the faction never paid any tech cost but
        // suffered denials at init (e.g., it tried to deploy a tech
        // it never had the budget for) should still render — the
        // analyst gets a "this faction's roster is mis-sized" signal.
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let red = FactionId::from("red");
        sums.insert(
            red.clone(),
            TechCostSummary {
                faction: red,
                n_runs: 5,
                mean_deployment_spend: 0.0,
                mean_maintenance_spend: 0.0,
                mean_total_spend: 0.0,
                runs_with_denial: 5,
                runs_with_decommission: 0,
                mean_decommissions_per_run: 0.0,
            },
        );
        summary.tech_cost_summaries = sums;
        let mut out = String::new();
        TechCosts.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("100% (5/5)"));
        assert!(out.contains("0% (0/5)"));
    }
}
