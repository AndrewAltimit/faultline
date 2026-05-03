//! Utility Decomposition section (Epic J round-one — adaptive AI
//! scaffold).
//!
//! Surfaces per-faction utility-driven decision analytics: which
//! axes drove which decisions across the run set, and how often
//! adaptive triggers fired. Pairs with the engine-side
//! `crate::utility::evaluate_action_utility` and the cross-run
//! aggregator `crate::utility_decomposition::compute_utility_decompositions`.
//!
//! Elided when `summary.utility_decompositions` is empty — i.e. no
//! scenario faction declared `[utility]`.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;
use crate::utility_decomposition::ordered_term_keys;

pub(super) struct UtilityDecomposition;

impl ReportSection for UtilityDecomposition {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.utility_decompositions.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Utility Decomposition");
        let _ = writeln!(
            out,
            "Per-faction adaptive utility analytics from the AI scoring path. Each faction declaring `[utility]` re-scores candidate actions against named axes (control, casualties_self, casualties_inflicted, attribution_risk, time_to_objective, resource_cost, force_concentration). The mean per-decision contribution describes which axis drove the action selection — a faction whose `control` mean is much larger than its `casualties_self` mean was operating like a control-maximiser; the inverse profile reads as cautious. The trigger fire rate captures how often each adaptive condition held."
        );
        let _ = writeln!(out);

        // Per-faction term means table. Columns are the canonical
        // term order; rows are factions. Empty cells render as `—`.
        let term_keys = ordered_term_keys();
        let _ = write!(
            out,
            "| Faction | Runs (contributing/total) | Mean ticks/run | Mean decisions/run"
        );
        for k in &term_keys {
            let _ = write!(out, " | {}", k);
        }
        let _ = writeln!(out, " |");
        let _ = write!(out, "|---|---|---|---");
        for _ in &term_keys {
            let _ = write!(out, "|---");
        }
        let _ = writeln!(out, "|");

        for row in summary.utility_decompositions.values() {
            let _ = write!(
                out,
                "| `{}` | {}/{} | {:.1} | {:.1}",
                escape_md_cell(&row.faction.0),
                row.runs_with_contribution,
                summary.total_runs,
                row.mean_tick_count,
                row.mean_decision_count,
            );
            for k in &term_keys {
                let v = row.mean_contributions_per_decision.get(*k);
                match v {
                    Some(value) if *value != 0.0 => {
                        let _ = write!(out, " | {:+.4}", value);
                    },
                    _ => {
                        let _ = write!(out, " | —");
                    },
                }
            }
            let _ = writeln!(out, " |");
        }
        let _ = writeln!(out);

        // Per-trigger fire-rate table — emitted only when at least
        // one faction declared a trigger. Rate is fires-per-decision-
        // phase across the whole run set; "—" for factions whose row
        // has no triggers (i.e. profile is purely static).
        let any_trigger = summary
            .utility_decompositions
            .values()
            .any(|row| !row.trigger_fire_rates.is_empty());
        if any_trigger {
            let _ = writeln!(out, "**Adaptive trigger fire rates**");
            let _ = writeln!(out);
            let _ = writeln!(out, "| Faction | Trigger | Fire rate |");
            let _ = writeln!(out, "|---|---|---|");
            for row in summary.utility_decompositions.values() {
                if row.trigger_fire_rates.is_empty() {
                    continue;
                }
                for (trigger_id, rate) in &row.trigger_fire_rates {
                    let _ = writeln!(
                        out,
                        "| `{}` | `{}` | {:.1}% |",
                        escape_md_cell(&row.faction.0),
                        escape_md_cell(trigger_id),
                        rate * 100.0,
                    );
                }
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_A 100% rate means the trigger held in every decision phase the faction's profile contributed to. A 0% rate means the trigger was declared but never matched — analyst flag for either a typo'd condition or a tighter threshold than the run hit._"
            );
            let _ = writeln!(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::UtilityDecompositionSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_empty() {
        let mut out = String::new();
        UtilityDecomposition.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(out.is_empty(), "should elide on empty");
    }

    #[test]
    fn renders_with_static_profile() {
        let mut summary = empty_summary();
        summary.total_runs = 10;
        let alpha = FactionId::from("alpha");
        let mut means = BTreeMap::new();
        means.insert("control".into(), 0.5);
        means.insert("casualties_self".into(), -0.1);
        summary.utility_decompositions.insert(
            alpha.clone(),
            UtilityDecompositionSummary {
                faction: alpha,
                runs_with_contribution: 10,
                mean_tick_count: 50.0,
                mean_decision_count: 150.0,
                mean_contributions_per_decision: means,
                trigger_fire_rates: BTreeMap::new(),
            },
        );
        let mut out = String::new();
        UtilityDecomposition.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Utility Decomposition"));
        assert!(out.contains("`alpha`"));
        assert!(out.contains("control"));
        assert!(out.contains("+0.5000"));
        assert!(out.contains("-0.1000"));
        // No triggers declared, so the trigger fire-rate sub-table
        // should not appear.
        assert!(!out.contains("Adaptive trigger fire rates"));
    }

    #[test]
    fn renders_trigger_fire_rates_when_declared() {
        let mut summary = empty_summary();
        summary.total_runs = 4;
        let alpha = FactionId::from("alpha");
        let mut rates = BTreeMap::new();
        rates.insert("panic".into(), 0.25);
        rates.insert("frenzy".into(), 0.0);
        summary.utility_decompositions.insert(
            alpha.clone(),
            UtilityDecompositionSummary {
                faction: alpha,
                runs_with_contribution: 4,
                mean_tick_count: 10.0,
                mean_decision_count: 30.0,
                mean_contributions_per_decision: BTreeMap::new(),
                trigger_fire_rates: rates,
            },
        );
        let mut out = String::new();
        UtilityDecomposition.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("Adaptive trigger fire rates"));
        assert!(out.contains("`panic`"));
        assert!(out.contains("25.0%"));
        assert!(out.contains("`frenzy`"));
        assert!(out.contains("0.0%"));
    }

    #[test]
    fn empty_term_means_render_as_em_dash() {
        let mut summary = empty_summary();
        summary.total_runs = 1;
        let alpha = FactionId::from("alpha");
        summary.utility_decompositions.insert(
            alpha.clone(),
            UtilityDecompositionSummary {
                faction: alpha,
                runs_with_contribution: 0,
                mean_tick_count: 0.0,
                mean_decision_count: 0.0,
                mean_contributions_per_decision: BTreeMap::new(),
                trigger_fire_rates: BTreeMap::new(),
            },
        );
        let mut out = String::new();
        UtilityDecomposition.render(&summary, &minimal_scenario(), &mut out);
        // Several columns of "—" between the summary numbers and the
        // end-of-row pipe.
        assert!(out.contains("| —"));
    }
}
