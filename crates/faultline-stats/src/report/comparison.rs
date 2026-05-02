//! Comparison-mode report (`--counterfactual` / `--compare`):
//! prepends a per-variant deltas block in front of the standard
//! per-scenario report so readers see the deltas first.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use crate::counterfactual::{ComparisonReport, ParamOverride};

/// Render a Markdown report for a counterfactual or `--compare` run.
///
/// Prepends a "Counterfactual Comparison" section to the usual
/// per-scenario report so readers see the deltas first. `scenario` is
/// the baseline; each variant summary is already included in `report`.
pub fn render_comparison_markdown(report: &ComparisonReport, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Counterfactual Report");
    let _ = writeln!(out, "## Baseline: {}", report.baseline_label);
    let _ = writeln!(out);
    let _ = writeln!(out, "- **Baseline runs:** {}", report.baseline.total_runs);
    let _ = writeln!(
        out,
        "- **Baseline mean duration:** {:.1} ticks",
        report.baseline.average_duration
    );
    let _ = writeln!(out);

    for (variant, delta) in report.variants.iter().zip(report.deltas.iter()) {
        render_variant_section(&mut out, variant, delta, &report.baseline);
    }

    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# Baseline Full Report");
    let _ = writeln!(out);

    out.push_str(&super::render_markdown(&report.baseline, scenario));

    out
}

fn render_variant_section(
    out: &mut String,
    variant: &crate::counterfactual::VariantSummary,
    delta: &crate::counterfactual::ComparisonDelta,
    baseline: &MonteCarloSummary,
) {
    let _ = writeln!(out, "## Variant: {}", variant.label);
    if let Some(src) = &variant.source_scenario {
        let _ = writeln!(out, "- **Source scenario:** {}", src);
    }
    if !variant.overrides.is_empty() {
        let _ = writeln!(out, "- **Applied overrides:**");
        for ov in &variant.overrides {
            render_override_line(out, ov);
        }
    }
    let _ = writeln!(
        out,
        "- **Mean duration delta:** {:+.2} ticks ({:.1} → {:.1})",
        delta.mean_duration_delta, baseline.average_duration, variant.summary.average_duration
    );
    let _ = writeln!(out);

    if !delta.win_rate_deltas.is_empty() {
        let _ = writeln!(out, "### Win-rate deltas");
        let _ = writeln!(out, "| Faction | Baseline | Variant | Δ (pp) |");
        let _ = writeln!(out, "|---|---|---|---|");
        for (fid, d) in &delta.win_rate_deltas {
            let b = baseline.win_rates.get(fid).copied().unwrap_or(0.0);
            let v = variant.summary.win_rates.get(fid).copied().unwrap_or(0.0);
            let _ = writeln!(
                out,
                "| `{}` | {:.1}% | {:.1}% | **{:+.1}** |",
                fid,
                b * 100.0,
                v * 100.0,
                d * 100.0
            );
        }
        let _ = writeln!(out);
    }

    if !delta.chain_deltas.is_empty() {
        let _ = writeln!(out, "### Kill-chain deltas");
        let _ = writeln!(
            out,
            "| Chain | Success Δ (pp) | Detection Δ (pp) | Cost-ratio Δ | Attacker spend Δ | Defender spend Δ |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for (cid, cd) in &delta.chain_deltas {
            let _ = writeln!(
                out,
                "| `{}` | **{:+.1}** | **{:+.1}** | **{:+.1}×** | **${:+.0}** | **${:+.0}** |",
                cid,
                cd.overall_success_rate_delta * 100.0,
                cd.detection_rate_delta * 100.0,
                cd.cost_asymmetry_ratio_delta,
                cd.attacker_spend_delta,
                cd.defender_spend_delta
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Positive success Δ = campaign more likely to succeed under the variant; positive detection Δ = defender more likely to catch it; positive cost-ratio Δ = defender paying more per attacker dollar. Both batches share the same seed and run count._"
        );
        let _ = writeln!(out);
    }
}

fn render_override_line(out: &mut String, ov: &ParamOverride) {
    let _ = writeln!(out, "  - `{}` = **{}**", ov.path, ov.value);
}
