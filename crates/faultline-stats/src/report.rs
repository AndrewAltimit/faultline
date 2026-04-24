//! ETRA-style Markdown report generation from Monte Carlo summaries.
//!
//! Produces a structured document suitable for pasting into research
//! write-ups. Consumes only types from
//! `faultline_types` so it works against any summary source (native CLI,
//! WASM, or stored JSON).

use std::fmt::Write;

use faultline_types::campaign::{CampaignPhase, KillChain};
use faultline_types::ids::PhaseId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    ConfidenceInterval, ConfidenceLevel, FeasibilityConfidence, FeasibilityRow, MonteCarloSummary,
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
        let has_cis = !summary.win_rate_cis.is_empty();
        if has_cis {
            let _ = writeln!(out, "| Faction | Probability | 95% CI |");
            let _ = writeln!(out, "|---|---|---|");
        } else {
            let _ = writeln!(out, "| Faction | Probability |");
            let _ = writeln!(out, "|---|---|");
        }
        for (fid, rate) in &summary.win_rates {
            if let Some(ci) = summary.win_rate_cis.get(fid) {
                let _ = writeln!(
                    out,
                    "| `{}` | {:.1}% | {} |",
                    fid,
                    rate * 100.0,
                    fmt_ci_pct(ci)
                );
            } else {
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
                    row.confidence.technology_readiness.clone(),
                    None,
                ),
                fmt_cell(
                    row.operational_complexity,
                    row.confidence.operational_complexity.clone(),
                    None,
                ),
                fmt_cell(
                    row.detection_probability,
                    row.confidence.detection_probability.clone(),
                    row.ci_95.detection_probability.as_ref(),
                ),
                fmt_cell(
                    row.success_probability,
                    row.confidence.success_probability.clone(),
                    row.ci_95.success_probability.as_ref(),
                ),
                fmt_cell(
                    row.consequence_severity,
                    row.confidence.consequence_severity.clone(),
                    row.ci_95.consequence_severity.as_ref(),
                ),
                row.attribution_difficulty,
                row.cost_asymmetry_ratio,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Cell format: `value [confidence]` or `value [confidence] (lo–hi)` when a 95% Wilson CI is available. Confidence bucket is derived from the CI half-width; see Methodology._"
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

    let author_flagged = collect_author_flagged(scenario);
    if !author_flagged.is_empty() {
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

    let _ = writeln!(out, "## Methodology & Confidence");
    let _ = writeln!(out, "{}", METHODOLOGY_APPENDIX.trim_start());

    out
}

const METHODOLOGY_APPENDIX: &str = r#"
This report combines two distinct sources of uncertainty. Mixing them up is a common way to get analysis wrong, so they are reported separately:

- **Sampling uncertainty** (the Wilson CIs below). Given the scenario's specified parameters, how precisely did the Monte Carlo runs estimate the rates shown? More runs shrink these intervals.
- **Parameter uncertainty** (the author-flagged confidence tags). Are the input parameters themselves defensible? A tight Wilson CI around a success rate derived from expert-guess detection probabilities does not mean the real-world success rate is known to that precision.

### 95% confidence intervals
Win rates, phase success rates, detection rates, and the rate-valued feasibility cells use the [Wilson score interval][wilson] at `z ≈ 1.960` (the standard-normal 97.5% quantile). Wilson is used in preference to the textbook Wald approximation because Wald collapses to `[0, 0]` or `[1, 1]` when zero or all runs succeed, implying false certainty for rare events. Wilson retains well-calibrated coverage across `p ∈ [0, 1]`.

Continuous metrics (duration, casualties, resources expended) are summarised by their mean, 5th / 95th percentiles, and standard deviation. A percentile-bootstrap CI helper is available in `faultline_stats::uncertainty::percentile_bootstrap_ci` for downstream callers that need a CI on the mean; it is deterministic under a seeded `ChaCha8Rng`.

[wilson]: https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval#Wilson_score_interval

### Confidence bucket derivation
The `[H]` / `[M]` / `[L]` tag on rate-valued feasibility cells is a coarse readability aid derived from the Wilson CI half-width at the scenario's run count:

| Bucket | Wilson half-width | Interpretation |
|---|---|---|
| `H` (High) | `< 0.03` | ±3 percentage points at 95% |
| `M` (Medium) | `< 0.08` | ±8 percentage points at 95% |
| `L` (Low) | otherwise (or `n < 30`) | Wide enough that comparing two `L` values is unsafe |

The `technology_readiness` bucket is a separate diagnostic: it is `L` when fewer than two phases exist in the chain, and otherwise buckets the coefficient of variation of per-phase base-success probabilities (`<0.15` → `H`, `<0.40` → `M`, else `L`). A `L` tag here means the chain's phases vary widely in expected success and a single "readiness" number is lossy, not that the MC estimate is imprecise.

### Author-flagged parameters
Authors can annotate `CampaignPhase.parameter_confidence` and `PhaseCost.confidence` in the TOML scenario to signal how defensible the input numbers are — `High` for commodity-parts costs or published rate cards, `Low` for wide expert estimates. Any phase or cost block flagged `Low` is listed in a dedicated section above when present. This complements, and does not replace, a full sensitivity sweep.
"#;

fn fmt_cell(value: f64, conf: ConfidenceLevel, ci: Option<&ConfidenceInterval>) -> String {
    let tag = match conf {
        ConfidenceLevel::High => "H",
        ConfidenceLevel::Medium => "M",
        ConfidenceLevel::Low => "L",
    };
    match ci {
        Some(ci) => format!("{:.2} [{}] ({:.2}–{:.2})", value, tag, ci.lower, ci.upper),
        None => format!("{:.2} [{}]", value, tag),
    }
}

fn fmt_ci_pct(ci: &ConfidenceInterval) -> String {
    format!("{:.1}% – {:.1}%", ci.lower * 100.0, ci.upper * 100.0)
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

// The renderer pulls these types into the compiled output via
// `summary.feasibility_matrix`; keep an explicit anchor so that
// removing the table from the document does not silently drop the
// import.
#[allow(dead_code)]
fn _type_anchor(_r: &FeasibilityRow, _c: &FeasibilityConfidence) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::campaign::{CampaignPhase, KillChain, PhaseCost};
    use faultline_types::ids::{FactionId, KillChainId, PhaseId};
    use faultline_types::map::{MapConfig, MapSource};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::stats::MonteCarloSummary;

    fn empty_summary() -> MonteCarloSummary {
        MonteCarloSummary {
            total_runs: 0,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 0.0,
            metric_distributions: BTreeMap::new(),
            regional_control: BTreeMap::new(),
            event_probabilities: BTreeMap::new(),
            campaign_summaries: BTreeMap::new(),
            feasibility_matrix: Vec::new(),
            seam_scores: BTreeMap::new(),
        }
    }

    fn minimal_scenario() -> Scenario {
        Scenario {
            meta: ScenarioMeta {
                name: "Report Test".into(),
                description: "description".into(),
                author: "test".into(),
                version: "0.0.1".into(),
                tags: vec![],
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions: BTreeMap::new(),
                infrastructure: BTreeMap::new(),
                terrain: vec![],
            },
            factions: BTreeMap::new(),
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.0,
                institutional_trust: 0.5,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.7,
                    internet_availability: 0.8,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 1,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(0),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions: BTreeMap::new(),
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
        }
    }

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
        summary.win_rate_cis.insert(
            fid,
            ConfidenceInterval {
                point: 0.62,
                lower: 0.52,
                upper: 0.71,
                n: 100,
            },
        );
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
}
