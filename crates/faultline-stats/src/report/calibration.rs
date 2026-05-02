//! Calibration section.
//!
//! Two emission modes:
//!
//! - **Calibrated:** scenario declares `meta.historical_analogue` and
//!   the summary carries a `CalibrationReport`. Renders the analogue
//!   header (name, period, sources, description) plus a per-observation
//!   table with verdicts and a roll-up.
//! - **Synthetic disclaimer:** scenario declares no analogue. Renders a
//!   one-paragraph "purely synthetic" notice so the analyst is told
//!   what the absence means rather than left to assume calibration.
//!
//! Always emitted — even on an empty MC summary that elides every other
//! data section, this one renders the synthetic disclaimer. Reasoning:
//! a report without a calibration statement leaves the reader to assume
//! the numbers are externally anchored, which is exactly the trust gap
//! Epic N is designed to close.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::{CalibrationVerdict, MonteCarloSummary};

use super::ReportSection;
use super::util::{confidence_word, escape_md_cell};

pub(super) struct Calibration;

impl ReportSection for Calibration {
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let _ = writeln!(out, "## Calibration");
        let analogue = scenario.meta.historical_analogue.as_ref();
        match (analogue, summary.calibration.as_ref()) {
            (None, _) => render_synthetic(out),
            (Some(_), None) => {
                // Analogue declared but summary is missing — happens
                // for empty run sets (compute_summary's early-return
                // path returns calibration: None). Render the analogue
                // header so the reader sees the claim, and a
                // disclaimer about why the verdict is missing.
                render_analogue_header(out, scenario);
                let _ = writeln!(
                    out,
                    "_No Monte Carlo runs were available, so no calibration verdict was computed._"
                );
                let _ = writeln!(out);
            },
            (Some(_), Some(report)) => {
                render_analogue_header(out, scenario);
                render_observation_table(out, report);
                render_overall(out, report.overall);
            },
        }
    }
}

fn render_synthetic(out: &mut String) {
    let _ = writeln!(
        out,
        "_This scenario is **purely synthetic** — no `historical_analogue` is declared, so there is no externally-anchored reference distribution to compare the Monte Carlo output against._"
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "What that means for interpretation: the Wilson and bootstrap intervals above are sound *given the parameters*, but the parameters themselves are not back-tested against a documented precedent. Treat the numbers as exploring the implications of a structural model, not as a calibrated forecast. Authors who want their scenario to carry a calibration verdict should add a `[meta.historical_analogue]` block — see the schema reference in `docs/scenario_schema.md`."
    );
    let _ = writeln!(out);
}

fn render_analogue_header(out: &mut String, scenario: &Scenario) {
    // Safe to unwrap: this function is only called from the two arms
    // of the match where analogue is Some.
    let analogue = scenario
        .meta
        .historical_analogue
        .as_ref()
        .expect("render_analogue_header called with analogue absent");

    let _ = writeln!(out, "**Analogue:** {}", escape_md_cell(&analogue.name));
    let _ = writeln!(out, "**Period:** {}", escape_md_cell(&analogue.period));
    if let Some(c) = analogue.confidence.as_ref() {
        let _ = writeln!(
            out,
            "**Author confidence in analogue fit:** {}",
            confidence_word(c)
        );
    }
    let _ = writeln!(out);
    if !analogue.description.is_empty() {
        let _ = writeln!(out, "{}", analogue.description);
        let _ = writeln!(out);
    }
    if !analogue.sources.is_empty() {
        let _ = writeln!(out, "**Sources:**");
        for s in &analogue.sources {
            let _ = writeln!(out, "- {}", escape_md_cell(s));
        }
        let _ = writeln!(out);
    }
}

fn render_observation_table(out: &mut String, report: &faultline_types::stats::CalibrationReport) {
    let _ = writeln!(
        out,
        "| Observation | MC outcome | Source confidence | Verdict | Notes |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|");
    for obs in &report.observations {
        let source_conf = obs
            .source_confidence
            .as_ref()
            .map(confidence_word)
            .unwrap_or("—");
        let notes = if obs.notes.is_empty() {
            String::new()
        } else {
            escape_md_cell(&obs.notes)
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            escape_md_cell(&obs.label),
            escape_md_cell(&obs.mc_summary),
            source_conf,
            verdict_word(obs.verdict),
            notes,
        );
    }
    let _ = writeln!(out);
}

fn render_overall(out: &mut String, overall: CalibrationVerdict) {
    let _ = writeln!(
        out,
        "**Overall calibration: {}** _(rolled up as the worst per-observation verdict — calibration claims compose as ANDs, not ORs)._",
        verdict_word(overall)
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "_Verdict thresholds — `Winner`: Pass when the observed faction is the MC modal winner with ≥ 50% mass; Marginal when modal-but-below-majority or non-modal-but-≥ 25%; Fail otherwise. `WinRate`: Pass when the MC point estimate falls in the historical interval; Marginal when the Wilson 95% CI overlaps the interval; Fail otherwise. `DurationTicks`: Pass when ≥ 50% of MC runs fall in the interval; Marginal when ≥ 25%; Fail otherwise._"
    );
    let _ = writeln!(out);
}

fn verdict_word(v: CalibrationVerdict) -> &'static str {
    match v {
        CalibrationVerdict::Pass => "Pass",
        CalibrationVerdict::Marginal => "Marginal",
        CalibrationVerdict::Fail => "Fail",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::scenario::{HistoricalAnalogue, HistoricalMetric, HistoricalObservation};
    use faultline_types::stats::{
        CalibrationReport, CalibrationVerdict, ConfidenceLevel, ObservationCalibration,
    };

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn synthetic_disclaimer_emits_on_no_analogue() {
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let mut out = String::new();
        Calibration.render(&summary, &scenario, &mut out);
        assert!(
            out.contains("## Calibration"),
            "header should emit; got: {out}"
        );
        assert!(
            out.contains("purely synthetic"),
            "should mention synthetic; got: {out}"
        );
        assert!(
            !out.contains("Pass") && !out.contains("Fail"),
            "should not include verdict words; got: {out}"
        );
    }

    #[test]
    fn full_calibration_renders_table_and_rollup() {
        let mut scenario = minimal_scenario();
        scenario.meta.historical_analogue = Some(HistoricalAnalogue {
            name: "Test Analogue".into(),
            description: "A short prose description.".into(),
            period: "2010-01-01 to 2010-02-01".into(),
            sources: vec!["Cited Source A".into(), "Cited Source B".into()],
            confidence: Some(ConfidenceLevel::Medium),
            observations: vec![HistoricalObservation {
                metric: HistoricalMetric::Winner {
                    faction: faultline_types::ids::FactionId::from("blue"),
                },
                confidence: Some(ConfidenceLevel::High),
                notes: "decisive outcome".into(),
            }],
        });
        let mut summary = empty_summary();
        summary.calibration = Some(CalibrationReport {
            analogue_name: "Test Analogue".into(),
            observations: vec![ObservationCalibration {
                label: "winner = blue".into(),
                mc_summary: "blue wins 80.0% (MC modal)".into(),
                source_confidence: Some(ConfidenceLevel::High),
                verdict: CalibrationVerdict::Pass,
                notes: "decisive outcome".into(),
            }],
            overall: CalibrationVerdict::Pass,
        });

        let mut out = String::new();
        Calibration.render(&summary, &scenario, &mut out);

        assert!(out.contains("## Calibration"));
        assert!(out.contains("Test Analogue"));
        assert!(out.contains("2010-01-01"));
        assert!(out.contains("Cited Source A"));
        assert!(out.contains("A short prose description."));
        assert!(out.contains("winner = blue"));
        assert!(out.contains("Pass"), "verdict word should appear");
        assert!(
            out.contains("Overall calibration: Pass"),
            "rollup should appear; got: {out}"
        );
    }

    #[test]
    fn missing_summary_emits_header_and_disclaimer() {
        // Analogue declared but no MC summary — the empty-runs path of
        // compute_summary returns calibration: None. The renderer
        // should still emit the analogue header and a disclaimer
        // explaining the missing verdict.
        let mut scenario = minimal_scenario();
        scenario.meta.historical_analogue = Some(HistoricalAnalogue {
            name: "Empty Analogue".into(),
            description: String::new(),
            period: "test".into(),
            sources: vec![],
            confidence: None,
            observations: vec![],
        });
        let summary = empty_summary();
        let mut out = String::new();
        Calibration.render(&summary, &scenario, &mut out);
        assert!(out.contains("## Calibration"));
        assert!(out.contains("Empty Analogue"));
        assert!(out.contains("No Monte Carlo runs"));
    }

    #[test]
    fn always_emits_section_heading() {
        // Pin the always-emit invariant explicitly: the calibration
        // section is the one data-section that emits even on the empty
        // run-set path, because absence-of-statement is a worse signal
        // than a synthetic-scenario disclaimer.
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let mut out = String::new();
        Calibration.render(&summary, &scenario, &mut out);
        assert!(out.starts_with("## Calibration"));
    }
}
