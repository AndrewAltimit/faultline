//! Doctrinal seam scoring and feasibility matrix output.
//!
//! These operate purely over `RunResult.campaign_reports` and the
//! scenario configuration — they do not re-run the engine.

use std::collections::{BTreeMap, BTreeSet};

use faultline_types::campaign::DefensiveDomain;
use faultline_types::ids::KillChainId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    ConfidenceLevel, FeasibilityConfidence, FeasibilityRow, PhaseOutcome, RunResult, SeamScore,
};

use crate::CampaignSummary;

// ---------------------------------------------------------------------------
// Feasibility matrix (6.5)
// ---------------------------------------------------------------------------

/// Build a feasibility matrix row for each kill chain in the scenario.
pub fn compute_feasibility_matrix(
    runs: &[RunResult],
    scenario: &Scenario,
    campaign_summaries: &BTreeMap<KillChainId, CampaignSummary>,
) -> Vec<FeasibilityRow> {
    let mut rows = Vec::new();

    for (chain_id, chain) in &scenario.kill_chains {
        let summary = match campaign_summaries.get(chain_id) {
            Some(s) => s,
            None => continue,
        };

        // Technology readiness: mean of baseline success probabilities.
        let tech_readiness = if chain.phases.is_empty() {
            0.0
        } else {
            chain
                .phases
                .values()
                .map(|p| p.base_success_probability)
                .sum::<f64>()
                / chain.phases.len() as f64
        };

        // Operational complexity: the more phases and the lower the
        // end-to-end success rate, the higher the complexity.
        let op_complexity = (1.0 - summary.overall_success_rate).clamp(0.0, 1.0)
            * (chain.phases.len() as f64 / 10.0).clamp(0.1, 1.0);

        let detection_probability = summary.detection_rate;
        let success_probability = summary.overall_success_rate;

        // Consequence severity — compute from final-state tension
        // delta and average institutional erosion in campaign reports.
        let mean_tension: f64 = if runs.is_empty() {
            0.0
        } else {
            runs.iter().map(|r| r.final_state.tension).sum::<f64>() / runs.len() as f64
        };
        // Average across runs that actually produced a report for this
        // chain. Dividing by `runs.len()` would artificially depress the
        // mean when some runs have no campaign report.
        let mean_institutional_erosion: f64 = {
            let (sum, count) = runs
                .iter()
                .filter_map(|r| r.campaign_reports.get(chain_id))
                .fold((0.0_f64, 0_usize), |(s, n), r| {
                    (s + r.institutional_erosion, n + 1)
                });
            if count == 0 { 0.0 } else { sum / count as f64 }
        };
        let consequence_severity =
            (mean_tension * 0.4 + mean_institutional_erosion * 0.6).clamp(0.0, 1.0);

        let attribution_difficulty = (1.0 - summary.mean_attribution_confidence).clamp(0.0, 1.0);

        // Confidence: use the coefficient of variation of phase success
        // rates across the chain as a proxy for MC stability.
        let confidence = FeasibilityConfidence {
            technology_readiness: confidence_from_variance(
                &chain
                    .phases
                    .values()
                    .map(|p| p.base_success_probability)
                    .collect::<Vec<_>>(),
            ),
            operational_complexity: ConfidenceLevel::Medium,
            detection_probability: confidence_from_rate(detection_probability, runs.len()),
            success_probability: confidence_from_rate(success_probability, runs.len()),
            consequence_severity: confidence_from_rate(consequence_severity, runs.len()),
        };

        rows.push(FeasibilityRow {
            chain_id: chain_id.clone(),
            chain_name: chain.name.clone(),
            technology_readiness: tech_readiness,
            operational_complexity: op_complexity,
            detection_probability,
            success_probability,
            consequence_severity,
            attribution_difficulty,
            cost_asymmetry_ratio: summary.cost_asymmetry_ratio,
            confidence,
        });
    }

    rows
}

fn confidence_from_variance(values: &[f64]) -> ConfidenceLevel {
    if values.len() < 2 {
        return ConfidenceLevel::Low;
    }
    let mean = values.iter().copied().sum::<f64>() / values.len() as f64;
    if mean < 1e-6 {
        return ConfidenceLevel::Low;
    }
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let cv = variance.sqrt() / mean;
    if cv < 0.15 {
        ConfidenceLevel::High
    } else if cv < 0.4 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::Low
    }
}

fn confidence_from_rate(rate: f64, n: usize) -> ConfidenceLevel {
    // Wald 95% CI half-width as a confidence proxy.
    if n < 30 {
        return ConfidenceLevel::Low;
    }
    let p = rate.clamp(0.01, 0.99);
    let half_width = 1.96 * (p * (1.0 - p) / n as f64).sqrt();
    if half_width < 0.03 {
        ConfidenceLevel::High
    } else if half_width < 0.08 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::Low
    }
}

// ---------------------------------------------------------------------------
// Seam scoring (6.4)
// ---------------------------------------------------------------------------

/// Compute doctrinal seam scores for each kill chain.
pub fn compute_seam_scores(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<KillChainId, SeamScore> {
    let mut out = BTreeMap::new();

    for (chain_id, chain) in &scenario.kill_chains {
        let mut cross_domain_phases = 0u32;
        let mut total_domains_count = 0u64;
        let mut domain_frequency: BTreeMap<String, u32> = BTreeMap::new();

        for phase in chain.phases.values() {
            let unique: BTreeSet<&DefensiveDomain> = phase.targets_domains.iter().collect();
            if unique.len() >= 2 {
                cross_domain_phases += 1;
            }
            total_domains_count += unique.len() as u64;
            for d in &unique {
                *domain_frequency.entry(domain_label(d)).or_insert(0) += 1;
            }
        }

        let n_phases = chain.phases.len().max(1) as f64;
        let mean_domains_per_phase = total_domains_count as f64 / n_phases;

        // Seam exploitation share: for each run, what fraction of
        // successful phases were cross-domain?
        let mut share_sum = 0.0_f64;
        let mut share_count = 0u32;
        for run in runs {
            if let Some(report) = run.campaign_reports.get(chain_id) {
                let succeeded: Vec<_> = report
                    .phase_outcomes
                    .iter()
                    .filter(|(_, o)| matches!(o, PhaseOutcome::Succeeded { .. }))
                    .map(|(pid, _)| pid.clone())
                    .collect();
                if succeeded.is_empty() {
                    continue;
                }
                let cross_succeeded = succeeded
                    .iter()
                    .filter(|pid| {
                        chain
                            .phases
                            .get(pid)
                            .map(|p| {
                                let u: BTreeSet<&DefensiveDomain> =
                                    p.targets_domains.iter().collect();
                                u.len() >= 2
                            })
                            .unwrap_or(false)
                    })
                    .count();
                share_sum += cross_succeeded as f64 / succeeded.len() as f64;
                share_count += 1;
            }
        }
        let seam_exploitation_share = if share_count == 0 {
            0.0
        } else {
            share_sum / f64::from(share_count)
        };

        out.insert(
            chain_id.clone(),
            SeamScore {
                chain_id: chain_id.clone(),
                cross_domain_phase_count: cross_domain_phases,
                mean_domains_per_phase,
                domain_frequency,
                seam_exploitation_share,
            },
        );
    }

    out
}

fn domain_label(d: &DefensiveDomain) -> String {
    match d {
        DefensiveDomain::PhysicalSecurity => "PhysicalSecurity".into(),
        DefensiveDomain::NetworkSecurity => "NetworkSecurity".into(),
        DefensiveDomain::CounterUAS => "CounterUAS".into(),
        DefensiveDomain::ExecutiveProtection => "ExecutiveProtection".into(),
        DefensiveDomain::CivilianEmergency => "CivilianEmergency".into(),
        DefensiveDomain::SignalsIntelligence => "SignalsIntelligence".into(),
        DefensiveDomain::InsiderThreat => "InsiderThreat".into(),
        DefensiveDomain::SupplyChainSecurity => "SupplyChainSecurity".into(),
        DefensiveDomain::Custom(s) => format!("Custom:{s}"),
    }
}
