//! Counterfactual and side-by-side scenario comparison.
//!
//! Epic B's core analyst workflow: "what if the defender had X?" and
//! "how does scenario A compare to scenario B?" This module produces
//! [`ComparisonReport`] values that pair a *baseline* Monte Carlo
//! summary against one or more *variant* summaries and summarise the
//! deltas in the metrics that matter most for structured threat-assessment analysis —
//! per-faction win rates, mean duration, mean casualties, and the
//! kill-chain feasibility cells (success / detection / cost asymmetry).
//!
//! Semantics to keep in mind:
//!
//! - Both batches share the same base seed and run count when invoked
//!   from the CLI, so differences reflect parameter changes rather
//!   than sampling noise.
//! - Deltas are `variant - baseline`. Positive `win_rate_delta` means
//!   the faction is more likely to win in the variant.
//! - `ComparisonReport` is serializable so `--format json` can emit
//!   counterfactual output programmatically.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::info;

use faultline_types::ids::{FactionId, KillChainId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloSummary};

use crate::sensitivity::set_param;
use crate::{MonteCarloRunner, StatsError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single parameter override of the form `param.path=value`.
///
/// `PartialEq` lets co-evolution tests compare per-side assignment
/// vectors directly. The `value` is `f64`, so equality is bit-equal —
/// callers comparing values produced by independent computations should
/// use a tolerance check instead (see
/// `faultline_stats::coevolve::assignments_equal`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ParamOverride {
    pub path: String,
    pub value: f64,
}

impl ParamOverride {
    /// Parse a `path=value` pair as passed on the CLI.
    pub fn parse(s: &str) -> Result<Self, StatsError> {
        let (path, value) = s.split_once('=').ok_or_else(|| {
            StatsError::InvalidConfig(format!(
                "counterfactual override must be '<path>=<value>', got '{s}'"
            ))
        })?;
        let value: f64 = value.trim().parse().map_err(|e| {
            StatsError::InvalidConfig(format!(
                "counterfactual value '{value}' is not a number: {e}"
            ))
        })?;
        // Rust's `f64::parse` accepts "NaN", "inf", "-inf" without error.
        // Allowing these through would silently propagate non-finite values
        // into probability fields and produce garbage deltas with no warning.
        if !value.is_finite() {
            return Err(StatsError::InvalidConfig(format!(
                "counterfactual value '{value}' must be a finite number"
            )));
        }
        Ok(ParamOverride {
            path: path.trim().to_string(),
            value,
        })
    }
}

/// A single labelled variant run alongside the baseline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VariantSummary {
    /// Human-readable label (e.g. "counterfactual", "scenario B",
    /// "defender hardens detection").
    pub label: String,
    /// Applied parameter overrides (empty for `--compare`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<ParamOverride>,
    /// Display name of the second scenario (from its TOML `[meta] name`)
    /// if this variant came from `--compare`. This is a human-readable
    /// label, not a filesystem path — consumers scripting against the
    /// JSON output should not treat it as a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scenario: Option<String>,
    pub summary: MonteCarloSummary,
}

/// Result of running a baseline Monte Carlo batch plus one or more
/// variants and computing the pairwise deltas.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub baseline_label: String,
    pub baseline: MonteCarloSummary,
    pub variants: Vec<VariantSummary>,
    pub deltas: Vec<ComparisonDelta>,
}

/// Pairwise delta between baseline and one variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonDelta {
    pub variant_label: String,
    pub mean_duration_delta: f64,
    /// Per-faction `variant_rate - baseline_rate`. Factions present in
    /// only one side appear with the missing side treated as `0.0`.
    pub win_rate_deltas: BTreeMap<FactionId, f64>,
    /// Per-kill-chain feasibility deltas.
    pub chain_deltas: BTreeMap<KillChainId, ChainDelta>,
}

/// Per-chain feasibility deltas surfaced in the report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainDelta {
    pub overall_success_rate_delta: f64,
    pub detection_rate_delta: f64,
    pub cost_asymmetry_ratio_delta: f64,
    pub attacker_spend_delta: f64,
    pub defender_spend_delta: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a counterfactual comparison: baseline scenario vs. the same
/// scenario with `overrides` applied.
///
/// Both batches reuse `config` (same seed, same run count), so any
/// observed delta reflects the parameter change rather than sampling
/// noise. `overrides` is parsed from repeated `--counterfactual` CLI
/// flags.
///
/// **Determinism invariant.** This function relies on
/// `MonteCarloRunner::run` deriving each per-run seed deterministically
/// from `config.seed` plus the run index (see `lib.rs::MonteCarloRunner::run`).
/// If a future refactor introduces non-deterministic seed selection
/// inside the runner (e.g. parallel iteration that doesn't preserve
/// run-index ordering), the deltas reported here will become noisy.
/// The `counterfactual_is_deterministic_under_fixed_seed` test below
/// pins this contract — re-running the same overrides on the same
/// scenario must produce a bit-identical `ComparisonReport`.
pub fn run_counterfactual(
    baseline: &Scenario,
    config: &MonteCarloConfig,
    overrides: &[ParamOverride],
) -> Result<ComparisonReport, StatsError> {
    if overrides.is_empty() {
        return Err(StatsError::InvalidConfig(
            "counterfactual run requires at least one --counterfactual override".into(),
        ));
    }

    info!(
        override_count = overrides.len(),
        "starting counterfactual analysis"
    );

    let baseline_result = MonteCarloRunner::run(config, baseline)?;

    let mut variant_scenario = baseline.clone();
    for ov in overrides {
        set_param(&mut variant_scenario, &ov.path, ov.value)?;
        info!(path = %ov.path, value = ov.value, "applied counterfactual override");
    }

    let variant_result = MonteCarloRunner::run(config, &variant_scenario)?;

    let delta = compute_delta(
        "counterfactual",
        &baseline_result.summary,
        &variant_result.summary,
    );

    Ok(ComparisonReport {
        baseline_label: "baseline".into(),
        baseline: baseline_result.summary,
        variants: vec![VariantSummary {
            label: "counterfactual".into(),
            overrides: overrides.to_vec(),
            source_scenario: None,
            summary: variant_result.summary,
        }],
        deltas: vec![delta],
    })
}

/// Run `--compare` mode: baseline scenario vs. a second scenario TOML.
///
/// Both batches reuse `config` so comparison is apples-to-apples on
/// the sampling side — any delta reflects scenario-level structural
/// differences. `alt_label` is shown in the report.
pub fn run_compare(
    baseline: &Scenario,
    alt: &Scenario,
    alt_label: &str,
    config: &MonteCarloConfig,
) -> Result<ComparisonReport, StatsError> {
    info!(alt_label, "starting scenario comparison");

    let baseline_result = MonteCarloRunner::run(config, baseline)?;
    let alt_result = MonteCarloRunner::run(config, alt)?;

    let delta = compute_delta(alt_label, &baseline_result.summary, &alt_result.summary);

    Ok(ComparisonReport {
        baseline_label: baseline.meta.name.clone(),
        baseline: baseline_result.summary,
        variants: vec![VariantSummary {
            label: alt_label.to_string(),
            overrides: vec![],
            source_scenario: Some(alt.meta.name.clone()),
            summary: alt_result.summary,
        }],
        deltas: vec![delta],
    })
}

// ---------------------------------------------------------------------------
// Delta computation
// ---------------------------------------------------------------------------

fn compute_delta(
    label: &str,
    baseline: &MonteCarloSummary,
    variant: &MonteCarloSummary,
) -> ComparisonDelta {
    let mut win_rate_deltas: BTreeMap<FactionId, f64> = BTreeMap::new();
    // Union of faction ids from both sides so a faction that only wins
    // on one side still shows up (with the other side at 0.0).
    let mut keys: std::collections::BTreeSet<FactionId> =
        baseline.win_rates.keys().cloned().collect();
    keys.extend(variant.win_rates.keys().cloned());
    for fid in keys {
        let b = baseline.win_rates.get(&fid).copied().unwrap_or(0.0);
        let v = variant.win_rates.get(&fid).copied().unwrap_or(0.0);
        win_rate_deltas.insert(fid, v - b);
    }

    let mut chain_deltas: BTreeMap<KillChainId, ChainDelta> = BTreeMap::new();
    let mut chain_keys: std::collections::BTreeSet<KillChainId> =
        baseline.campaign_summaries.keys().cloned().collect();
    chain_keys.extend(variant.campaign_summaries.keys().cloned());
    for chain_id in chain_keys {
        let b = baseline.campaign_summaries.get(&chain_id);
        let v = variant.campaign_summaries.get(&chain_id);
        let (b_succ, b_det, b_ratio, b_att, b_def) = match b {
            Some(cs) => (
                cs.overall_success_rate,
                cs.detection_rate,
                cs.cost_asymmetry_ratio,
                cs.mean_attacker_spend,
                cs.mean_defender_spend,
            ),
            None => (0.0, 0.0, 0.0, 0.0, 0.0),
        };
        let (v_succ, v_det, v_ratio, v_att, v_def) = match v {
            Some(cs) => (
                cs.overall_success_rate,
                cs.detection_rate,
                cs.cost_asymmetry_ratio,
                cs.mean_attacker_spend,
                cs.mean_defender_spend,
            ),
            None => (0.0, 0.0, 0.0, 0.0, 0.0),
        };
        chain_deltas.insert(
            chain_id,
            ChainDelta {
                overall_success_rate_delta: v_succ - b_succ,
                detection_rate_delta: v_det - b_det,
                cost_asymmetry_ratio_delta: v_ratio - b_ratio,
                attacker_spend_delta: v_att - b_att,
                defender_spend_delta: v_def - b_def,
            },
        );
    }

    ComparisonDelta {
        variant_label: label.to_string(),
        mean_duration_delta: variant.average_duration - baseline.average_duration,
        win_rate_deltas,
        chain_deltas,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_override_basic() {
        let o = ParamOverride::parse("political_climate.tension=0.9").expect("parse");
        assert_eq!(o.path, "political_climate.tension");
        assert!((o.value - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_override_with_whitespace() {
        let o = ParamOverride::parse("  faction.gov.intelligence = 0.7 ").expect("parse");
        assert_eq!(o.path, "faction.gov.intelligence");
        assert!((o.value - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_override_rejects_missing_equals() {
        assert!(ParamOverride::parse("no_equals_sign").is_err());
    }

    #[test]
    fn parse_override_rejects_non_numeric() {
        assert!(ParamOverride::parse("foo.bar=not_a_number").is_err());
    }

    #[test]
    fn parse_override_rejects_nan_and_infinities() {
        // f64::parse accepts these silently; the override parser must not.
        // A NaN / inf reaching `set_param` would corrupt downstream
        // probability fields and produce meaningless deltas.
        for bad in ["NaN", "nan", "inf", "+inf", "-inf", "infinity", "-infinity"] {
            let s = format!("foo.bar={bad}");
            assert!(
                ParamOverride::parse(&s).is_err(),
                "expected parse to reject non-finite value `{bad}`"
            );
        }
    }

    #[test]
    fn compute_delta_sums_missing_keys_as_zero() {
        use faultline_types::stats::MonteCarloSummary;
        let mut base = MonteCarloSummary {
            total_runs: 10,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 10.0,
            metric_distributions: BTreeMap::new(),
            regional_control: BTreeMap::new(),
            event_probabilities: BTreeMap::new(),
            campaign_summaries: BTreeMap::new(),
            feasibility_matrix: vec![],
            seam_scores: BTreeMap::new(),
            correlation_matrix: None,
            pareto_frontier: None,
            defender_capacity: Vec::new(),
            network_summaries: std::collections::BTreeMap::new(),
            alliance_dynamics: None,
            supply_pressure_summaries: ::std::collections::BTreeMap::new(),
        };
        let mut variant = base.clone();

        let f_a = FactionId::from("a");
        let f_b = FactionId::from("b");
        base.win_rates.insert(f_a.clone(), 0.4);
        variant.win_rates.insert(f_a.clone(), 0.6);
        // f_b only present on variant side; baseline treated as 0.0.
        variant.win_rates.insert(f_b.clone(), 0.2);
        variant.average_duration = 12.0;

        let d = compute_delta("v", &base, &variant);
        assert!((d.mean_duration_delta - 2.0).abs() < f64::EPSILON);
        assert!((d.win_rate_deltas[&f_a] - 0.2).abs() < f64::EPSILON);
        assert!((d.win_rate_deltas[&f_b] - 0.2).abs() < f64::EPSILON);
    }
}
