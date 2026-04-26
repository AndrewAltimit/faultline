//! Declarative strategy search space (Epic H — round one).
//!
//! A [`StrategySpace`] is the analyst-authored declaration of *which*
//! parameters in a scenario are decision variables and *what range of
//! values* they can take. It hangs off `Scenario` as
//! `Scenario.strategy_space`, optional and `#[serde(default)]` so legacy
//! scenarios load unchanged. The search runner in `faultline-stats` reads
//! this declaration, samples assignments, evaluates each via Monte Carlo,
//! and reports the non-dominated frontier.
//!
//! ## Determinism contract
//!
//! - Sampling decision-variable assignments uses a search-only RNG seeded
//!   from `SearchConfig.search_seed`, which is independent of
//!   `MonteCarloConfig.seed`. Same `search_seed` + same space + same
//!   method always produces the same trial assignments.
//! - Per-trial Monte Carlo evaluation reuses `mc_config.seed` across
//!   trials so trial-to-trial deltas reflect parameter changes only,
//!   not sampling noise.
//!
//! ## Schema-evolution invariants
//!
//! - All fields are `#[serde(default)]` or have safe defaults. An empty
//!   `StrategySpace` (no variables) is valid and serializes/deserializes
//!   round-trip; the report renderer elides empty spaces.
//! - Adding a new `Domain` variant requires bumping the migrator (Epic O)
//!   only if existing serialised forms would mis-deserialize. Today every
//!   variant is internally tagged, so additive variants are safe.

use serde::{Deserialize, Serialize};

use crate::ids::FactionId;

/// The full strategy-search declaration on a scenario.
///
/// Empty by default — a scenario with no `strategy_space` table simply
/// produces no decision variables and rejects `--search` invocations
/// with a clear error rather than silently running zero trials.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StrategySpace {
    /// Decision variables. Each names a parameter (resolvable via the
    /// `set_param` path layer in `faultline-stats::sensitivity`) and a
    /// domain to sample from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<DecisionVariable>,
    /// Optional embedded objectives. CLI may also pass objectives at
    /// invocation time; when both are present, the CLI list wins so a
    /// pre-canned space can be reused for one-off questions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<SearchObjective>,
}

impl StrategySpace {
    /// `true` when the scenario declared no decision variables — the
    /// most common shape, since strategy search is opt-in. Used by the
    /// CLI and validator to short-circuit search-mode dispatch.
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
}

/// A single decision variable within a strategy space.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DecisionVariable {
    /// Parameter path. Same dotted form accepted by `--counterfactual`
    /// and `--sensitivity` (e.g. `faction.alpha.initial_morale`,
    /// `kill_chain.heist.phase.exfil.detection_probability_per_tick`).
    /// The search runner validates each path against `set_param` before
    /// the first trial so authoring typos surface up-front.
    pub path: String,
    /// Optional faction owner — informational, surfaced in the report
    /// so analysts can read "attacker decisions vs. defender decisions"
    /// without inferring it from the path naming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FactionId>,
    /// Domain to sample from / enumerate.
    pub domain: Domain,
}

/// What shape of values this variable can take.
///
/// `Continuous` and `Discrete` are the two shapes the round-one runner
/// supports. Categorical strings (e.g. doctrine choice) would need a
/// non-`f64` variant and a corresponding extension to `set_param` —
/// deferred to a follow-up.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Domain {
    /// Continuous interval `[low, high]`. Random search draws uniformly;
    /// grid search emits `steps` evenly-spaced values inclusive of both
    /// endpoints. `low <= high` and both finite are validation invariants.
    Continuous { low: f64, high: f64, steps: u32 },
    /// Enumerated values. Each trial picks one. Empty `values` is
    /// rejected at validation time.
    Discrete { values: Vec<f64> },
}

/// What the search optimizes. Multiple objectives are evaluated for
/// every trial; the runner reports best-by-objective and the Pareto
/// frontier across all of them.
///
/// Round-one objectives are derived from existing `MonteCarloSummary` /
/// `CampaignSummary` fields — no new analytics modules required. Adding
/// a new objective is additive (existing serialised search results
/// remain valid).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "metric", rename_all = "snake_case")]
pub enum SearchObjective {
    /// Maximize a faction's win rate.
    MaximizeWinRate { faction: FactionId },
    /// Minimize the maximum per-chain detection rate.
    MinimizeDetection,
    /// Minimize mean attacker spend, summed across kill chains.
    MinimizeAttackerCost,
    /// Maximize the maximum per-chain cost-asymmetry ratio
    /// (attacker spend / defender spend). Higher = attacker pays less
    /// per dollar of defender spend.
    MaximizeCostAsymmetry,
    /// Minimize average run duration.
    MinimizeDuration,
}

impl SearchObjective {
    /// Stable string label used as the BTreeMap key in
    /// `SearchTrial.objective_values` and the report's per-objective
    /// section headers. Format: `metric_kind[:argument]`. Adding a new
    /// objective variant must add a new label here so report rendering
    /// and JSON keys stay aligned.
    pub fn label(&self) -> String {
        match self {
            Self::MaximizeWinRate { faction } => format!("maximize_win_rate:{faction}"),
            Self::MinimizeDetection => "minimize_detection".to_string(),
            Self::MinimizeAttackerCost => "minimize_attacker_cost".to_string(),
            Self::MaximizeCostAsymmetry => "maximize_cost_asymmetry".to_string(),
            Self::MinimizeDuration => "minimize_duration".to_string(),
        }
    }

    /// Direction of optimization. `true` = larger is better.
    pub fn maximize(&self) -> bool {
        matches!(
            self,
            Self::MaximizeWinRate { .. } | Self::MaximizeCostAsymmetry
        )
    }

    /// Parse an objective from a CLI string. Format mirrors `label()`:
    /// `<kind>[:<argument>]`.
    pub fn parse_cli(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        let (kind, arg) = match trimmed.split_once(':') {
            Some((k, a)) => (k, Some(a.trim())),
            None => (trimmed, None),
        };
        match kind {
            "maximize_win_rate" => {
                let faction = arg.ok_or_else(|| {
                    "maximize_win_rate requires a faction id, e.g. \
                     'maximize_win_rate:alpha'"
                        .to_string()
                })?;
                if faction.is_empty() {
                    return Err("maximize_win_rate faction id cannot be empty".to_string());
                }
                Ok(Self::MaximizeWinRate {
                    faction: FactionId::from(faction),
                })
            },
            "minimize_detection" => Ok(Self::MinimizeDetection),
            "minimize_attacker_cost" => Ok(Self::MinimizeAttackerCost),
            "maximize_cost_asymmetry" => Ok(Self::MaximizeCostAsymmetry),
            "minimize_duration" => Ok(Self::MinimizeDuration),
            other => Err(format!(
                "unknown search objective `{other}`. Supported: \
                 maximize_win_rate:<faction>, minimize_detection, \
                 minimize_attacker_cost, maximize_cost_asymmetry, \
                 minimize_duration"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objective_label_round_trip_simple() {
        for o in [
            SearchObjective::MinimizeDetection,
            SearchObjective::MinimizeAttackerCost,
            SearchObjective::MaximizeCostAsymmetry,
            SearchObjective::MinimizeDuration,
        ] {
            let parsed = SearchObjective::parse_cli(&o.label())
                .expect("label round-trip must parse for argument-less variants");
            assert_eq!(parsed, o);
        }
    }

    #[test]
    fn objective_label_round_trip_with_faction() {
        let o = SearchObjective::MaximizeWinRate {
            faction: FactionId::from("alpha"),
        };
        let parsed = SearchObjective::parse_cli(&o.label()).expect("parses");
        assert_eq!(parsed, o);
    }

    #[test]
    fn objective_parse_rejects_unknown_kind() {
        assert!(SearchObjective::parse_cli("not_a_metric").is_err());
    }

    #[test]
    fn objective_parse_rejects_missing_faction_argument() {
        let err = SearchObjective::parse_cli("maximize_win_rate")
            .expect_err("missing faction must error");
        assert!(err.contains("requires a faction"));
    }

    #[test]
    fn objective_parse_rejects_empty_faction_argument() {
        assert!(SearchObjective::parse_cli("maximize_win_rate:").is_err());
    }

    #[test]
    fn empty_strategy_space_is_empty() {
        let s = StrategySpace::default();
        assert!(s.is_empty());
    }
}
