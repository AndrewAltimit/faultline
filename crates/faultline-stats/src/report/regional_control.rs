//! Regional Control (terminal) section: per-region faction control
//! shares at the end of the run.
//!
//! Elided when no regional-control rollup is present.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct RegionalControl;

impl ReportSection for RegionalControl {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.regional_control.is_empty() {
            return;
        }
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
}
