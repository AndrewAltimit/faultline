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
    /// Named attacker strategies for robustness analysis (Epic I,
    /// round-two). Each profile is a full assignment to attacker-owned
    /// parameter paths; the robustness runner evaluates every defender
    /// posture against every profile to surface where a posture is
    /// fragile ("posture P wins under profile A and C, loses under B").
    /// Empty by default so legacy scenarios load unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attacker_profiles: Vec<AttackerProfile>,
}

impl StrategySpace {
    /// `true` when nothing useful is declared — neither variables,
    /// objectives, nor attacker profiles. Used as the
    /// `skip_serializing_if` predicate on `Scenario.strategy_space`, so
    /// a programmatically-constructed space with embedded `objectives`
    /// but no `variables` still round-trips through TOML/JSON instead
    /// of being silently dropped (the runner's "no variables to search"
    /// error is the right place to reject that shape, not the serializer).
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty() && self.objectives.is_empty() && self.attacker_profiles.is_empty()
    }
}

/// A named attacker strategy. The `assignments` apply to specific
/// parameter paths in the scenario (same dotted form as
/// `DecisionVariable.path`); the robustness runner clones the scenario,
/// applies the profile's assignments via `set_param`, then evaluates
/// each defender posture under that fixed attacker.
///
/// Profiles do not carry a `domain` — they are point assignments, not
/// search variables. The robustness runner accepts any `path` that
/// resolves via `set_param`, so a profile may name a parameter that
/// isn't in `StrategySpace.variables` (e.g. an attacker-side
/// `phase.attribution_difficulty` that the search loop wasn't sweeping).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttackerProfile {
    /// Display name, shown in the report. Must be unique within
    /// `StrategySpace.attacker_profiles`; duplicates are rejected at
    /// scenario-load time so a renamed profile doesn't silently shadow
    /// the original.
    pub name: String,
    /// Optional human-readable description. Surfaces in the report
    /// header for the per-profile column so an analyst reading
    /// `Profile: opportunist` sees what that profile represents
    /// without consulting the TOML.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Optional faction tag. Informational only — surfaces in the
    /// report. Validation does NOT require profiles' assignments to
    /// only touch this faction's parameters; an analyst may want to
    /// model a profile that includes environmental conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<FactionId>,
    /// The actual parameter overrides. Each entry is a (path, value)
    /// pair applied via `set_param`. Empty `assignments` is rejected
    /// at validation — a "no-op profile" should be expressed as the
    /// `--robustness-include-baseline` flag, not as an empty profile.
    pub assignments: Vec<ProfileAssignment>,
}

/// One (path, value) override inside an [`AttackerProfile`]. A separate
/// type from `ParamOverride` (which lives in `faultline-stats`) so the
/// `faultline-types` crate doesn't pull a stats dependency just for the
/// schema declaration; the robustness runner re-projects these into
/// `ParamOverride` at runtime.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProfileAssignment {
    /// Parameter path. Same dotted form as `DecisionVariable.path`.
    pub path: String,
    /// Value to assign. Must be finite — NaN / inf is rejected at
    /// validation time.
    pub value: f64,
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
    /// Minimize the maximum per-chain detection rate. Attacker-aligned
    /// (the attacker wants to stay invisible).
    MinimizeDetection,
    /// Minimize mean attacker spend, summed across kill chains.
    /// Attacker-aligned (the attacker wants to spend less).
    MinimizeAttackerCost,
    /// Maximize the maximum per-chain cost-asymmetry ratio
    /// (attacker spend / defender spend). Higher = attacker pays less
    /// per dollar of defender spend.
    MaximizeCostAsymmetry,
    /// Minimize average run duration.
    MinimizeDuration,
    /// Defender-aligned mirror of `MinimizeAttackerCost`: maximize the
    /// total cost the attacker has to bear to attempt the campaign.
    /// Sums `mean_attacker_spend` across all kill chains.
    MaximizeAttackerCost,
    /// Defender-aligned mirror of `MinimizeDetection`: maximize the
    /// maximum per-chain detection rate so attacker activity is visible.
    MaximizeDetection,
    /// Defender-aligned: minimize the total cost the defender has to
    /// spend across kill chains. Sums `mean_defender_spend` across the
    /// `campaign_summaries` map. Pair with `MaximizeWinRate` to surface
    /// cost-effective postures.
    MinimizeDefenderCost,
    /// Defender-aligned: minimize the maximum per-chain success rate.
    /// Reads `overall_success_rate` from `CampaignSummary` (the
    /// fraction of runs where the chain reached its terminal phase
    /// with at least one kinetic output delivered) so a defender
    /// posture that blocks every chain scores 0 and is best.
    MinimizeMaxChainSuccess,
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
            Self::MaximizeAttackerCost => "maximize_attacker_cost".to_string(),
            Self::MaximizeDetection => "maximize_detection".to_string(),
            Self::MinimizeDefenderCost => "minimize_defender_cost".to_string(),
            Self::MinimizeMaxChainSuccess => "minimize_max_chain_success".to_string(),
        }
    }

    /// Direction of optimization. `true` = larger is better.
    pub fn maximize(&self) -> bool {
        matches!(
            self,
            Self::MaximizeWinRate { .. }
                | Self::MaximizeCostAsymmetry
                | Self::MaximizeAttackerCost
                | Self::MaximizeDetection
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
            "maximize_attacker_cost" => Ok(Self::MaximizeAttackerCost),
            "maximize_detection" => Ok(Self::MaximizeDetection),
            "minimize_defender_cost" => Ok(Self::MinimizeDefenderCost),
            "minimize_max_chain_success" => Ok(Self::MinimizeMaxChainSuccess),
            other => Err(format!(
                "unknown search objective `{other}`. Supported: \
                 maximize_win_rate:<faction>, minimize_detection, \
                 minimize_attacker_cost, maximize_cost_asymmetry, \
                 minimize_duration, maximize_attacker_cost, \
                 maximize_detection, minimize_defender_cost, \
                 minimize_max_chain_success"
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
            SearchObjective::MaximizeAttackerCost,
            SearchObjective::MaximizeDetection,
            SearchObjective::MinimizeDefenderCost,
            SearchObjective::MinimizeMaxChainSuccess,
        ] {
            let parsed = SearchObjective::parse_cli(&o.label())
                .expect("label round-trip must parse for argument-less variants");
            assert_eq!(parsed, o);
        }
    }

    #[test]
    fn defender_objectives_have_correct_direction() {
        // Pin the maximize/minimize direction so a future refactor of
        // `maximize()` doesn't silently flip a defender objective.
        assert!(SearchObjective::MaximizeAttackerCost.maximize());
        assert!(SearchObjective::MaximizeDetection.maximize());
        assert!(!SearchObjective::MinimizeDefenderCost.maximize());
        assert!(!SearchObjective::MinimizeMaxChainSuccess.maximize());
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

    #[test]
    fn space_with_only_objectives_is_not_empty() {
        // Regression: previously `is_empty` only checked `variables`,
        // which meant a programmatically-built space with embedded
        // objectives but no variables would be skipped on serialize.
        // The serializer skips only when nothing useful is declared.
        let s = StrategySpace {
            variables: Vec::new(),
            objectives: vec![SearchObjective::MinimizeDuration],
            attacker_profiles: Vec::new(),
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn space_with_only_objectives_round_trips_through_json() {
        // Pin the round-trip: serialize → deserialize must preserve
        // the objectives list even with no variables.
        let s = StrategySpace {
            variables: Vec::new(),
            objectives: vec![SearchObjective::MinimizeDuration],
            attacker_profiles: Vec::new(),
        };
        let json = serde_json::to_string(&s).expect("serialize");
        let parsed: StrategySpace = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, s);
    }
}
