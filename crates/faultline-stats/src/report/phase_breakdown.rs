//! Kill Chain Phase Breakdown section: per-chain summary plus the
//! per-phase rate table with Wilson 95% CIs on success / detection /
//! failure / not-reached.
//!
//! Elided when no campaign summaries were collected.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{ConfidenceInterval, MonteCarloSummary};

use super::ReportSection;

pub(super) struct PhaseBreakdown;

impl ReportSection for PhaseBreakdown {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.campaign_summaries.is_empty() {
            return;
        }
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
                let cis = ps.ci_95.as_ref();
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} | {} | {} |",
                    pid,
                    fmt_rate_cell(ps.success_rate, cis.map(|c| &c.success_rate)),
                    fmt_rate_cell(ps.failure_rate, cis.map(|c| &c.failure_rate)),
                    fmt_rate_cell(ps.detection_rate, cis.map(|c| &c.detection_rate)),
                    fmt_rate_cell(ps.not_reached_rate, cis.map(|c| &c.not_reached_rate)),
                    mean_tick
                );
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_Rate cells show point estimate with 95% Wilson bounds when `n > 0`. Bounds widen for rare outcomes — a `0.0% (0.0–7.1)` success rate at `n = 50` is not the same as a deterministic zero._"
            );
            let _ = writeln!(out);
        }
    }
}

fn fmt_rate_cell(rate: f64, ci: Option<&ConfidenceInterval>) -> String {
    match ci {
        Some(ci) => format!(
            "{:.1}% ({:.1}–{:.1})",
            rate * 100.0,
            ci.lower * 100.0,
            ci.upper * 100.0
        ),
        None => format!("{:.1}%", rate * 100.0),
    }
}
