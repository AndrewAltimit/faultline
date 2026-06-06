//! Attribution Fidelity section (Epic M round-two — believed-attribution
//! rolls).
//!
//! Surfaces, across runs, how often a defender's *believed* attribution
//! of an attack diverged from the true attacker, how much of that
//! divergence a planted false flag drove, and whether the
//! misattribution went on to break an alliance against an innocent
//! party.
//!
//! Pairs with the engine-side believed-attribution roll in
//! `crate::campaign` and the cross-run aggregator
//! `crate::misattribution::compute_misattribution_summary`.
//!
//! Elided when `summary.misattribution_summary` is `None` — i.e. the
//! scenario left `simulation.belief_model.believed_attribution = false`
//! (or no defender ever detected a phase). Scenarios that don't opt in
//! render byte-identically to before, so their manifest `output_hash`
//! is unchanged.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct AttributionFidelity;

impl ReportSection for AttributionFidelity {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let Some(report) = &summary.misattribution_summary else {
            return;
        };

        let _ = writeln!(out, "## Attribution Fidelity");
        let _ = writeln!(
            out,
            "Believed-attribution analytics from the kill-chain detection path (Epic M round-two). With `simulation.belief_model.believed_attribution = true`, a defender that detects an attack no longer reads ground truth — it draws a *believed attacker* from a distribution weighted by its `intelligence` and by any planted false-flag belief it holds. A low-intelligence or deceived defender can therefore finger the wrong faction and then **act on the misattribution**: the alliance-fracture accounting credits the attribution confidence to the *believed* faction, so the defender can break with an innocent ally."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "**Misattribution rate** is the headline signal — the fraction of detection-time attribution rolls where the believed attacker differed from the true one. **Deception-driven rate** decomposes how much of that a planted `Deceived` belief implicated. **Fracture misattributions** counts the times a faction fractured against a counterparty it had misattributed an attack to in the same run — the behavioral payoff."
        );
        let _ = writeln!(out);

        let _ = writeln!(out, "| Metric | Value |");
        let _ = writeln!(out, "|---|---|");
        let _ = writeln!(out, "| Total attribution rolls | {} |", report.total_rolls);
        let _ = writeln!(
            out,
            "| Misattributed rolls | {} ({:.1}%) |",
            report.misattributed_rolls,
            report.misattribution_rate * 100.0,
        );
        let _ = writeln!(
            out,
            "| Deception-driven rolls | {} ({:.1}%) |",
            report.deception_driven_rolls,
            report.deception_driven_rate * 100.0,
        );
        let _ = writeln!(
            out,
            "| Fracture misattributions | {} |",
            report.fracture_misattributions,
        );
        let _ = writeln!(out);

        if !report.confusion_pairs.is_empty() {
            let _ = writeln!(
                out,
                "**Confusion pairs** — where the blame landed when the defender got it wrong (`true → believed`):"
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "| True attacker → believed | Count |");
            let _ = writeln!(out, "|---|---|");
            for (pair, count) in &report.confusion_pairs {
                let _ = writeln!(out, "| `{}` | {} |", escape_md_cell(pair), count);
            }
            let _ = writeln!(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};
    use faultline_types::stats::MisattributionSummary;
    use std::collections::BTreeMap;

    #[test]
    fn elides_when_no_summary() {
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let mut out = String::new();
        AttributionFidelity.render(&summary, &scenario, &mut out);
        assert!(out.is_empty(), "should elide when summary is None");
    }

    #[test]
    fn renders_table_and_confusion_pairs() {
        let mut summary = empty_summary();
        let mut confusion_pairs = BTreeMap::new();
        confusion_pairs.insert("red→green".to_string(), 3u64);
        summary.misattribution_summary = Some(MisattributionSummary {
            total_rolls: 10,
            misattributed_rolls: 4,
            deception_driven_rolls: 2,
            misattribution_rate: 0.4,
            deception_driven_rate: 0.2,
            fracture_misattributions: 1,
            confusion_pairs,
        });
        let scenario = minimal_scenario();
        let mut out = String::new();
        AttributionFidelity.render(&summary, &scenario, &mut out);
        assert!(out.contains("## Attribution Fidelity"), "got: {out}");
        assert!(out.contains("40.0%"), "misattribution rate: {out}");
        assert!(out.contains("20.0%"), "deception rate: {out}");
        assert!(out.contains("`red→green`"), "confusion pair: {out}");
        assert!(out.contains("Fracture misattributions | 1"), "got: {out}");
    }

    #[test]
    fn confusion_table_elided_when_empty() {
        let mut summary = empty_summary();
        summary.misattribution_summary = Some(MisattributionSummary {
            total_rolls: 5,
            misattributed_rolls: 0,
            deception_driven_rolls: 0,
            misattribution_rate: 0.0,
            deception_driven_rate: 0.0,
            fracture_misattributions: 0,
            confusion_pairs: BTreeMap::new(),
        });
        let scenario = minimal_scenario();
        let mut out = String::new();
        AttributionFidelity.render(&summary, &scenario, &mut out);
        assert!(out.contains("## Attribution Fidelity"), "got: {out}");
        assert!(!out.contains("Confusion pairs"), "got: {out}");
    }
}
