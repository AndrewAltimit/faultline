//! Belief Asymmetry section (Epic M round-one).
//!
//! Surfaces per-faction belief-asymmetry analytics: how accurate the
//! faction's persistent beliefs were against ground truth, how many
//! deception / intelligence-share events landed against it, and how
//! many deception entries persisted to run end (i.e. the believer
//! never observed the truth and the deception drove behavior all
//! the way through).
//!
//! Pairs with the engine-side `crate::belief` module and the
//! cross-run aggregator `crate::belief::compute_belief_summaries`.
//!
//! Elided when `summary.belief_summaries` is empty — i.e. the
//! scenario opted out of the belief model
//! (`simulation.belief_model.enabled = false` or no `[simulation.belief_model]`
//! at all).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct BeliefAsymmetry;

impl ReportSection for BeliefAsymmetry {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.belief_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Belief Asymmetry");
        let _ = writeln!(
            out,
            "Per-faction belief-asymmetry analytics from the persistent belief model (Epic M round-one). Each faction with `simulation.belief_model.enabled = true` carries a per-tick belief about opponent force locations, region control, faction morale, and faction resources. Direct observation refreshes beliefs at full confidence; unrefreshed beliefs decay; `EventEffect::DeceptionOp` plants false beliefs that look identical to direct observation from the AI's perspective but are tagged for analytics. `IntelligenceShare` does the same but with truthful content."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "**Mean force-strength error** is the headline accuracy signal: how far off, on average, was this faction's belief about opponent force strength? Lower = better intel. **Region accuracy** (in `[0, 1]`) measures correctly-believed region-controllers across known regions — 1.0 = perfect awareness."
        );
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "| Faction | Runs w/ belief | Mean force-Δ | Max force-Δ | Mean region-acc | Mean deceptions | Mean intel-shares | Mean terminal-deceived |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");

        for row in summary.belief_summaries.values() {
            let _ = writeln!(
                out,
                "| `{}` | {}/{} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                escape_md_cell(&row.faction.0),
                row.runs_with_belief,
                summary.total_runs,
                row.mean_force_strength_error,
                row.max_force_strength_error,
                row.mean_region_accuracy,
                row.mean_deception_events,
                row.mean_intel_shares,
                row.mean_terminal_deceived_beliefs,
            );
        }
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};
    use faultline_types::ids::FactionId;
    use faultline_types::stats::BeliefAsymmetrySummary;

    #[test]
    fn elides_when_no_belief_summaries() {
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let mut out = String::new();
        BeliefAsymmetry.render(&summary, &scenario, &mut out);
        assert!(
            out.is_empty(),
            "should elide when summary.belief_summaries is empty"
        );
    }

    #[test]
    fn renders_table_with_entries() {
        let mut summary = empty_summary();
        summary.total_runs = 4;
        summary.belief_summaries.insert(
            FactionId::from("blue"),
            BeliefAsymmetrySummary {
                faction: FactionId::from("blue"),
                runs_with_belief: 4,
                mean_force_strength_error: 12.5,
                max_force_strength_error: 25.0,
                mean_region_accuracy: 0.85,
                mean_deception_events: 2.0,
                mean_intel_shares: 0.5,
                mean_terminal_deceived_beliefs: 1.25,
            },
        );
        let scenario = minimal_scenario();
        let mut out = String::new();
        BeliefAsymmetry.render(&summary, &scenario, &mut out);
        assert!(out.contains("## Belief Asymmetry"), "got: {out}");
        assert!(out.contains("`blue`"), "got: {out}");
        assert!(out.contains("12.50"), "got: {out}");
        assert!(out.contains("0.85"), "got: {out}");
    }

    #[test]
    fn renders_zero_row_for_pre_seeded_faction() {
        // Pre-seeded entry (runs_with_belief = 0) should still render
        // so the analyst sees "declared but never engaged" cleanly.
        let mut summary = empty_summary();
        summary.belief_summaries.insert(
            FactionId::from("red"),
            BeliefAsymmetrySummary {
                faction: FactionId::from("red"),
                runs_with_belief: 0,
                mean_force_strength_error: 0.0,
                max_force_strength_error: 0.0,
                mean_region_accuracy: 0.0,
                mean_deception_events: 0.0,
                mean_intel_shares: 0.0,
                mean_terminal_deceived_beliefs: 0.0,
            },
        );
        let scenario = minimal_scenario();
        let mut out = String::new();
        BeliefAsymmetry.render(&summary, &scenario, &mut out);
        assert!(out.contains("`red`"), "got: {out}");
        assert!(out.contains("0/0"), "got: {out}");
    }
}
