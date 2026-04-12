//! ETRA-style Markdown report generation from Monte Carlo summaries.
//!
//! Produces a structured document suitable for pasting into research
//! write-ups. Consumes only types from
//! `faultline_types` so it works against any summary source (native CLI,
//! WASM, or stored JSON).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    ConfidenceLevel, FeasibilityConfidence, FeasibilityRow, MonteCarloSummary,
};

/// Render a Markdown feasibility / cost asymmetry / seam analysis
/// report for a single Monte Carlo run.
pub fn render_markdown(summary: &MonteCarloSummary, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Analysis Report");
    let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
    let _ = writeln!(out, "_{}_", scenario.meta.description.trim());
    let _ = writeln!(out);
    let _ = writeln!(out, "- **Runs:** {}", summary.total_runs);
    let _ = writeln!(
        out,
        "- **Average duration (ticks):** {:.1}",
        summary.average_duration
    );
    let _ = writeln!(out);

    if !summary.win_rates.is_empty() {
        let _ = writeln!(out, "## Win Rates");
        let _ = writeln!(out, "| Faction | Probability |");
        let _ = writeln!(out, "|---|---|");
        for (fid, rate) in &summary.win_rates {
            let _ = writeln!(out, "| `{}` | {:.1}% |", fid, rate * 100.0);
        }
        let _ = writeln!(out);
    }

    if !summary.feasibility_matrix.is_empty() {
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
                row.chain_name,
                fmt_cell(
                    row.technology_readiness,
                    row.confidence.technology_readiness.clone()
                ),
                fmt_cell(
                    row.operational_complexity,
                    row.confidence.operational_complexity.clone()
                ),
                fmt_cell(
                    row.detection_probability,
                    row.confidence.detection_probability.clone()
                ),
                fmt_cell(
                    row.success_probability,
                    row.confidence.success_probability.clone()
                ),
                fmt_cell(
                    row.consequence_severity,
                    row.confidence.consequence_severity.clone()
                ),
                row.attribution_difficulty,
                row.cost_asymmetry_ratio,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Cell format: `value [confidence]`. Confidence derived from MC variance._"
        );
        let _ = writeln!(out);
    }

    if !summary.campaign_summaries.is_empty() {
        let _ = writeln!(out, "## Kill Chain Phase Breakdown");
        for (chain_id, cs) in &summary.campaign_summaries {
            let _ = writeln!(out, "### `{}`", chain_id);
            let _ = writeln!(
                out,
                "- Overall success: **{:.1}%** · Detection: **{:.1}%** · Attribution confidence: {:.2}",
                cs.overall_success_rate * 100.0,
                cs.detection_rate * 100.0,
                cs.mean_attribution_confidence
            );
            let _ = writeln!(
                out,
                "- Attacker spend: **${:.0}** · Defender spend: **${:.0}** · Asymmetry: **{:.0}×**",
                cs.mean_attacker_spend, cs.mean_defender_spend, cs.cost_asymmetry_ratio
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "| Phase | Success | Failure | Detection | Not reached | Mean completion tick |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|");
            for (pid, ps) in &cs.phase_stats {
                let mean_tick = ps
                    .mean_completion_tick
                    .map(|t| format!("{:.1}", t))
                    .unwrap_or_else(|| "—".to_string());
                let _ = writeln!(
                    out,
                    "| `{}` | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {} |",
                    pid,
                    ps.success_rate * 100.0,
                    ps.failure_rate * 100.0,
                    ps.detection_rate * 100.0,
                    ps.not_reached_rate * 100.0,
                    mean_tick
                );
            }
            let _ = writeln!(out);
        }
    }

    if !summary.seam_scores.is_empty() {
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

    if !summary.regional_control.is_empty() {
        let _ = writeln!(out, "## Regional Control (terminal)");
        for (rid, fmap) in &summary.regional_control {
            let _ = write!(out, "- `{}`: ", rid);
            let parts: Vec<String> = fmap
                .iter()
                .map(|(fid, p)| format!("{} {:.0}%", fid, p * 100.0))
                .collect();
            let _ = writeln!(out, "{}", parts.join(", "));
        }
        let _ = writeln!(out);
    }

    out
}

fn fmt_cell(value: f64, conf: ConfidenceLevel) -> String {
    let tag = match conf {
        ConfidenceLevel::High => "H",
        ConfidenceLevel::Medium => "M",
        ConfidenceLevel::Low => "L",
    };
    format!("{:.2} [{}]", value, tag)
}

// Silence unused import for FeasibilityRow / FeasibilityConfidence —
// they are referenced via `summary.feasibility_matrix` above.
#[allow(dead_code)]
fn _type_anchor(_r: &FeasibilityRow, _c: &FeasibilityConfidence) {}
