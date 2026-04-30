//! Feasibility Matrix section: per-chain readiness, complexity,
//! detection, success, severity, attribution-difficulty, and cost-ratio
//! cells with author-confidence tags and Wilson 95% CIs where defined.
//!
//! Elided when the runner produced no feasibility rows.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{ConfidenceInterval, ConfidenceLevel, MonteCarloSummary};

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct Feasibility;

impl ReportSection for Feasibility {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.feasibility_matrix.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Feasibility Matrix");
        let _ = writeln!(
            out,
            "| Chain | Tech Readiness | Op Complexity | Detection | Success | Severity | Attribution Diff | Cost Ratio |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
        for row in &summary.feasibility_matrix {
            let _ = writeln!(
                out,
                "| **{}** | {} | {} | {} | {} | {} | {:.2} | **{:.0}×** |",
                escape_md_cell(&row.chain_name),
                fmt_cell(
                    row.technology_readiness,
                    row.confidence.technology_readiness.clone(),
                    None,
                ),
                fmt_cell(
                    row.operational_complexity,
                    row.confidence.operational_complexity.clone(),
                    None,
                ),
                fmt_cell(
                    row.detection_probability,
                    row.confidence.detection_probability.clone(),
                    row.ci_95.detection_probability.as_ref(),
                ),
                fmt_cell(
                    row.success_probability,
                    row.confidence.success_probability.clone(),
                    row.ci_95.success_probability.as_ref(),
                ),
                fmt_cell(
                    row.consequence_severity,
                    row.confidence.consequence_severity.clone(),
                    row.ci_95.consequence_severity.as_ref(),
                ),
                row.attribution_difficulty,
                row.cost_asymmetry_ratio,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Cell format: `value [confidence]` or `value [confidence] (lo–hi)` when a 95% Wilson CI is available. Confidence bucket is derived from the CI half-width; see Methodology._"
        );
        let _ = writeln!(out);
    }
}

fn fmt_cell(value: f64, conf: ConfidenceLevel, ci: Option<&ConfidenceInterval>) -> String {
    let tag = match conf {
        ConfidenceLevel::High => "H",
        ConfidenceLevel::Medium => "M",
        ConfidenceLevel::Low => "L",
    };
    match ci {
        Some(ci) => format!("{:.2} [{}] ({:.2}–{:.2})", value, tag, ci.lower, ci.upper),
        None => format!("{:.2} [{}]", value, tag),
    }
}
