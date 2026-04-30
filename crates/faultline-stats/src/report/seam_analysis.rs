//! Doctrinal Seam Analysis section: per-chain cross-domain phase
//! counts plus per-chain domain frequency tables.
//!
//! Elided when no seam scores were collected.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct SeamAnalysis;

impl ReportSection for SeamAnalysis {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.seam_scores.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Doctrinal Seam Analysis");
        let _ = writeln!(
            out,
            "| Chain | Cross-domain phases | Mean domains/phase | Seam exploitation share |"
        );
        let _ = writeln!(out, "|---|---|---|---|");
        for (chain_id, s) in &summary.seam_scores {
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.2} | {:.1}% |",
                chain_id,
                s.cross_domain_phase_count,
                s.mean_domains_per_phase,
                s.seam_exploitation_share * 100.0,
            );
        }
        let _ = writeln!(out);
        for (chain_id, s) in &summary.seam_scores {
            if s.domain_frequency.is_empty() {
                continue;
            }
            let _ = writeln!(out, "**`{}` domain frequency:**", chain_id);
            for (d, n) in &s.domain_frequency {
                let _ = writeln!(out, "- {}: {}", d, n);
            }
            let _ = writeln!(out);
        }
    }
}
