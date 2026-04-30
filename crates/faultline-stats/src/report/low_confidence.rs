//! Author-Flagged Low-Confidence Parameters section: lists every
//! kill-chain phase the author has tagged with `parameter_confidence`
//! or `cost.confidence = Low`.
//!
//! Elided when the scenario contains no flagged parameters.

use std::fmt::Write;

use faultline_types::campaign::{CampaignPhase, KillChain};
use faultline_types::ids::PhaseId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{ConfidenceLevel, MonteCarloSummary};

use super::ReportSection;

pub(super) struct LowConfidence;

impl ReportSection for LowConfidence {
    fn render(&self, _summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let author_flagged = collect_author_flagged(scenario);
        if author_flagged.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Author-Flagged Low-Confidence Parameters");
        let _ = writeln!(
            out,
            "The following scenario parameters are tagged `Low` confidence by the scenario author. Results that depend on them should be interpreted with correspondingly wider uncertainty than the Wilson CIs alone suggest."
        );
        let _ = writeln!(out);
        for (chain_name, phase_id, kind) in &author_flagged {
            let _ = writeln!(out, "- **{}** / `{}` — {}", chain_name, phase_id, kind);
        }
        let _ = writeln!(out);
    }
}

fn collect_author_flagged(scenario: &Scenario) -> Vec<(String, PhaseId, String)> {
    let mut out = Vec::new();
    for chain in scenario.kill_chains.values() {
        collect_flagged_from_chain(chain, &mut out);
    }
    out
}

fn collect_flagged_from_chain(chain: &KillChain, out: &mut Vec<(String, PhaseId, String)>) {
    let chain_name = chain.name.clone();
    for (pid, phase) in &chain.phases {
        if let Some(kind) = describe_flag(phase) {
            out.push((chain_name.clone(), pid.clone(), kind));
        }
    }
}

fn describe_flag(phase: &CampaignPhase) -> Option<String> {
    let mut parts: Vec<&'static str> = Vec::new();
    if matches!(phase.parameter_confidence, Some(ConfidenceLevel::Low)) {
        parts.push("phase parameters");
    }
    if matches!(phase.cost.confidence, Some(ConfidenceLevel::Low)) {
        parts.push("phase cost");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}
