//! Win Rates section: per-faction terminal win probability with Wilson
//! 95% CIs when the runner populated them.
//!
//! Elided when the summary records no win rates (e.g. a single-run
//! report or a run that never resolved a victor).

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{ConfidenceInterval, MonteCarloSummary};

use super::ReportSection;

pub(super) struct WinRates;

impl ReportSection for WinRates {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.win_rates.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Win Rates");
        let has_cis = !summary.win_rate_cis.is_empty();
        if has_cis {
            let _ = writeln!(out, "| Faction | Probability | 95% CI |");
            let _ = writeln!(out, "|---|---|---|");
        } else {
            let _ = writeln!(out, "| Faction | Probability |");
            let _ = writeln!(out, "|---|---|");
        }
        for (fid, rate) in &summary.win_rates {
            let ci_cell = summary.win_rate_cis.get(fid);
            // `has_cis` fixes the table column count for the whole
            // section. If any individual faction is missing a CI entry,
            // emit a placeholder rather than a short row — otherwise
            // the Markdown table becomes malformed. The two maps are
            // built from the same iterator in the runner today, so this
            // branch is defensive against divergence if `MonteCarloSummary`
            // is constructed by other callers.
            if has_cis {
                let cell = ci_cell.map(fmt_ci_pct).unwrap_or_else(|| "—".to_string());
                let _ = writeln!(out, "| `{}` | {:.1}% | {} |", fid, rate * 100.0, cell);
            } else {
                debug_assert!(
                    ci_cell.is_none(),
                    "win_rate_cis populated but has_cis is false for `{fid}`",
                );
                let _ = writeln!(out, "| `{}` | {:.1}% |", fid, rate * 100.0);
            }
        }
        if has_cis {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_Win-rate CIs use the Wilson score interval (95%, z ≈ 1.960)._"
            );
        }
        let _ = writeln!(out);
    }
}

fn fmt_ci_pct(ci: &ConfidenceInterval) -> String {
    format!("{:.1}% – {:.1}%", ci.lower * 100.0, ci.upper * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};
    use faultline_types::ids::FactionId;

    #[test]
    fn elides_when_no_win_rates() {
        let mut out = String::new();
        WinRates.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn renders_ci_column_when_cis_present() {
        let mut summary = empty_summary();
        summary.total_runs = 100;
        let fid = FactionId::from("alpha");
        summary.win_rates.insert(fid.clone(), 0.4);
        summary
            .win_rate_cis
            .insert(fid, ConfidenceInterval::new(0.4, 0.31, 0.50, 100));
        let mut out = String::new();
        WinRates.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("95% CI"));
        assert!(out.contains("31.0% – 50.0%"));
    }
}
