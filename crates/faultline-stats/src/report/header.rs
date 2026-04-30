//! Header section: title, scenario name + description, the optional
//! `[meta].confidence` banner, and the run / duration intro line.
//!
//! Always emitted — the report would be unparseable without a header.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{ConfidenceLevel, MonteCarloSummary};

use super::ReportSection;
use super::util::confidence_word;

pub(super) struct Header;

impl ReportSection for Header {
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let _ = writeln!(out, "# Faultline Analysis Report");
        let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
        let _ = writeln!(out, "_{}_", scenario.meta.description.trim());
        let _ = writeln!(out);
        if let Some(conf) = &scenario.meta.confidence {
            // Banner is distinct from the Wilson CIs — it flags *parameter*
            // defensibility, not sampling precision. Symbol is intentionally
            // plain ASCII so reports render identically in stripped terminals.
            let (glyph, label) = match conf {
                ConfidenceLevel::High => ("[H]", "publication-ready rigor"),
                ConfidenceLevel::Medium => ("[M]", "working draft"),
                ConfidenceLevel::Low => ("[L]", "conceptual sketch"),
            };
            let _ = writeln!(
                out,
                "> **Scenario confidence: {} {} — _{}_.** See Methodology for how this interacts with the Wilson CIs below.",
                glyph,
                confidence_word(conf),
                label
            );
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "- **Runs:** {}", summary.total_runs);
        let _ = writeln!(
            out,
            "- **Average duration (ticks):** {:.1}",
            summary.average_duration
        );
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn renders_no_banner_when_confidence_unset() {
        let mut out = String::new();
        Header.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(!out.contains("Scenario confidence:"));
    }

    #[test]
    fn renders_banner_glyph_per_confidence_level() {
        // The banner glyph is plain ASCII deliberately (see comment in
        // the renderer); pin each variant so a font-substitution
        // refactor can't quietly swap them for unicode glyphs.
        for (level, glyph, label) in [
            (ConfidenceLevel::High, "[H]", "publication-ready rigor"),
            (ConfidenceLevel::Medium, "[M]", "working draft"),
            (ConfidenceLevel::Low, "[L]", "conceptual sketch"),
        ] {
            let mut scenario = minimal_scenario();
            scenario.meta.confidence = Some(level.clone());
            let mut out = String::new();
            Header.render(&empty_summary(), &scenario, &mut out);
            assert!(
                out.contains(glyph) && out.contains(label),
                "banner for {level:?} missing glyph/label; got:\n{out}"
            );
        }
    }
}
