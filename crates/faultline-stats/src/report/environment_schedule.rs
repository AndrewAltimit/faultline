//! Environment Schedule section: per-window weather /
//! time-of-day modifiers with per-terrain factors and the global
//! detection factor.
//!
//! Elided when the scenario declares no environment windows.

use std::fmt::Write;

use faultline_types::map::Activation;
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct EnvironmentSchedule;

impl ReportSection for EnvironmentSchedule {
    fn render(&self, _summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        if scenario.environment.windows.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Environment Schedule");
        let _ = writeln!(
            out,
            "Active environmental windows. Per-terrain factors apply when the engaged region's terrain is in `applies_to` (empty = every terrain). The `detection` factor is global — it multiplies every kill-chain phase's per-tick detection probability regardless of region."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Window | Activation | Applies to | Movement | Defense | Visibility | Detection |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");
        for w in &scenario.environment.windows {
            let activation = match &w.activation {
                Activation::Always => "Always".to_string(),
                Activation::TickRange { start, end } => {
                    format!("Ticks {start}–{end}")
                },
                Activation::Cycle {
                    period,
                    phase,
                    duration,
                } => {
                    format!("Cycle p={period} φ={phase} d={duration}")
                },
            };
            let applies = if w.applies_to.is_empty() {
                "all".to_string()
            } else {
                w.applies_to
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let fmt_factor = |f: f64| {
                if (f - 1.0).abs() < 1e-9 {
                    "—".to_string()
                } else {
                    format!("{f:.2}×")
                }
            };
            let _ = writeln!(
                out,
                "| `{}` ({}) | {} | {} | {} | {} | {} | {} |",
                escape_md_cell(&w.id),
                escape_md_cell(&w.name),
                activation,
                escape_md_cell(&applies),
                fmt_factor(w.movement_factor),
                fmt_factor(w.defense_factor),
                fmt_factor(w.visibility_factor),
                fmt_factor(w.detection_factor),
            );
        }
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_support::{empty_summary, minimal_scenario};
    use faultline_types::map::EnvironmentWindow;

    #[test]
    fn elides_when_no_windows() {
        let mut out = String::new();
        EnvironmentSchedule.render(&empty_summary(), &minimal_scenario(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn cycle_activation_renders_phase_glyph() {
        // The Cycle row uses a literal `φ=` (lowercase phi) in the
        // activation cell. It's the kind of glyph easy to mis-encode if
        // someone refactors the format string — pin it.
        let mut scenario = minimal_scenario();
        scenario.environment.windows.push(EnvironmentWindow {
            id: "monsoon".into(),
            name: "Monsoon".into(),
            activation: faultline_types::map::Activation::Cycle {
                period: 30,
                phase: 5,
                duration: 10,
            },
            applies_to: vec![],
            movement_factor: 0.5,
            defense_factor: 1.0,
            visibility_factor: 1.0,
            detection_factor: 1.0,
        });
        let mut out = String::new();
        EnvironmentSchedule.render(&empty_summary(), &scenario, &mut out);
        assert!(
            out.contains("Cycle p=30 φ=5 d=10"),
            "cycle activation should render with phi glyph; got:\n{out}"
        );
        // Identity factors elide to em-dash; non-identity render as `0.50×`.
        assert!(
            out.contains("0.50×"),
            "non-identity movement factor should render numerically; got:\n{out}"
        );
    }
}
