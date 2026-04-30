//! Structured Markdown report generation from Monte Carlo summaries.
//!
//! Produces a structured document suitable for pasting into research
//! write-ups. Consumes only types from `faultline_types` so it works
//! against any summary source (native CLI, WASM, or stored JSON).
//!
//! The module is decomposed by section: each Monte Carlo section is its
//! own submodule (one struct, one `ReportSection` impl), and each of
//! the four other report types (comparison, search, coevolve,
//! robustness) lives in its own submodule. `render_markdown` is the
//! composer — it iterates a `&[&dyn ReportSection]` array so section
//! ordering is declarative and adding a new section is a matter of
//! creating one file plus one entry in the array.
//!
//! Section gating (elision when the underlying data is empty) lives in
//! the `ReportSection` impl, not the composer. This means the composer
//! never grows a long chain of `if let Some(..) { render_X(..) }`
//! conditionals as more analytics ship.

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

mod util;

mod continuous_metrics;
mod correlation;
mod countermeasure;
mod defender_capacity;
mod environment_schedule;
mod feasibility;
mod header;
mod leadership_disruption;
mod low_confidence;
mod methodology;
mod network_resilience;
mod pareto_frontier;
mod phase_breakdown;
mod policy_implications;
mod regional_control;
mod seam_analysis;
mod time_dynamics;
mod win_rates;

mod coevolve;
mod comparison;
mod robustness;
mod search;

#[cfg(test)]
pub(crate) mod test_support;

pub use coevolve::render_coevolve_markdown;
pub use comparison::render_comparison_markdown;
pub use robustness::render_robustness_markdown;
pub use search::render_search_markdown;

/// One contribution to a Monte Carlo Markdown report.
///
/// Implementations own their own elision logic — the composer never
/// asks "should I call you?". A section that has nothing useful to
/// emit for the given inputs returns without writing.
pub trait ReportSection {
    /// Append this section's contribution to `out`.
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String);
}

/// Render the Markdown analysis report for a Monte Carlo batch.
///
/// Walks the section composer in declared order. Output is byte-
/// stable for a fixed `(summary, scenario)` pair — required by the
/// manifest determinism contract.
pub fn render_markdown(summary: &MonteCarloSummary, scenario: &Scenario) -> String {
    let mut out = String::new();
    for section in monte_carlo_sections() {
        section.render(summary, scenario, &mut out);
    }
    out
}

/// Ordered list of Monte Carlo sections. The order here defines the
/// order they appear in the rendered report. Adding a new section is
/// a matter of adding one entry; reordering is a matter of moving one
/// entry. No part of the composer needs to change.
fn monte_carlo_sections() -> [&'static dyn ReportSection; 18] {
    [
        &header::Header,
        &win_rates::WinRates,
        &continuous_metrics::ContinuousMetrics,
        &feasibility::Feasibility,
        &phase_breakdown::PhaseBreakdown,
        &time_dynamics::TimeDynamics,
        &pareto_frontier::ParetoFrontier,
        &correlation::CorrelationMatrix,
        &defender_capacity::DefenderCapacity,
        &network_resilience::NetworkResilience,
        &seam_analysis::SeamAnalysis,
        &regional_control::RegionalControl,
        &low_confidence::LowConfidence,
        &policy_implications::PolicyImplications,
        &countermeasure::Countermeasure,
        &environment_schedule::EnvironmentSchedule,
        &leadership_disruption::LeadershipDisruption,
        &methodology::Methodology,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::campaign::{CampaignPhase, KillChain, PhaseCost};
    use faultline_types::ids::{FactionId, KillChainId, PhaseId};
    use faultline_types::stats::{ConfidenceInterval, ConfidenceLevel};

    use test_support::{empty_summary, minimal_scenario};

    #[test]
    fn report_always_includes_methodology() {
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let md = render_markdown(&summary, &scenario);
        assert!(
            md.contains("## Methodology & Confidence"),
            "methodology section should always be present; got:\n{md}"
        );
        assert!(
            md.contains("Wilson score interval"),
            "methodology should mention Wilson; got:\n{md}"
        );
    }

    #[test]
    fn report_shows_win_rate_cis_when_present() {
        let mut summary = empty_summary();
        let fid = FactionId::from("gov");
        summary.total_runs = 100;
        summary.win_rates.insert(fid.clone(), 0.62);
        summary
            .win_rate_cis
            .insert(fid, ConfidenceInterval::new(0.62, 0.52, 0.71, 100));
        let md = render_markdown(&summary, &minimal_scenario());
        assert!(
            md.contains("95% CI"),
            "win-rate table should have CI column"
        );
        assert!(
            md.contains("52.0% – 71.0%"),
            "formatted CI should be present, got:\n{md}"
        );
    }

    #[test]
    fn report_lists_author_flagged_parameters() {
        let mut scenario = minimal_scenario();
        let chain_id = KillChainId::from("alpha");
        let phase_id = PhaseId::from("recon");
        let mut phases: BTreeMap<PhaseId, CampaignPhase> = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Recon".into(),
                description: "".into(),
                prerequisites: vec![],
                base_success_probability: 0.8,
                min_duration: 1,
                max_duration: 2,
                detection_probability_per_tick: 0.1,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 1_000.0,
                    defender_dollars: 10_000.0,
                    attacker_resources: 0.0,
                    confidence: Some(ConfidenceLevel::Low),
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![],
                parameter_confidence: Some(ConfidenceLevel::Low),
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Alpha Campaign".into(),
                description: "".into(),
                attacker: FactionId::from("red"),
                target: FactionId::from("blue"),
                entry_phase: phase_id,
                phases,
            },
        );
        let md = render_markdown(&empty_summary(), &scenario);
        assert!(
            md.contains("Author-Flagged Low-Confidence Parameters"),
            "should include low-confidence section"
        );
        assert!(md.contains("Alpha Campaign"), "should reference chain name");
        assert!(
            md.contains("phase parameters") && md.contains("phase cost"),
            "should describe both flag kinds; got:\n{md}"
        );
    }

    #[test]
    fn report_omits_flagged_section_when_no_flags() {
        let md = render_markdown(&empty_summary(), &minimal_scenario());
        assert!(
            !md.contains("Author-Flagged Low-Confidence Parameters"),
            "section should be elided when nothing is flagged"
        );
    }

    #[test]
    fn section_array_length_matches_active_sections() {
        // Pinned to catch a mistake where someone bumps the array
        // capacity without adding the corresponding entry — the
        // compiler will catch a count mismatch on the array-literal
        // type, but a stray uninitialized slot is harder to spot
        // by code review alone. Touching this number means you've
        // added or removed a section and updated `monte_carlo_sections`
        // accordingly.
        assert_eq!(monte_carlo_sections().len(), 18);
    }
}
