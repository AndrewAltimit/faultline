//! Strategy-search runner (Epic H — round one).
//!
//! Given a [`StrategySpace`](faultline_types::strategy_space::StrategySpace)
//! declaration on a scenario plus a [`SearchConfig`], evaluate `trials`
//! candidate assignments and surface:
//!
//! - the best assignment per objective,
//! - the non-dominated (Pareto) frontier across all objectives,
//! - the per-trial Monte Carlo summaries so an analyst can drill into
//!   any single point in the strategy space without re-running.
//!
//! ## Determinism contract
//!
//! Two seeds are deliberately separated:
//!
//! - `SearchConfig.search_seed` — drives random sampling of decision-
//!   variable assignments. Identical inputs (space, method, search_seed)
//!   always produce the same trial list.
//! - `mc_config.seed` — drives the inner Monte Carlo evaluation of each
//!   trial. The inner seed is **identical across trials**, so trial-to-
//!   trial deltas are pure parameter-change effects and not sampling
//!   noise. This mirrors the Epic B counterfactual contract (same seed,
//!   different parameters → reproducible delta).
//!
//! Search-then-evaluate is bit-identical: the `search_then_evaluate_is_deterministic`
//! test below pins this behaviour.
//!
//! ## Round-one scope
//!
//! - Random and grid sampling of continuous and discrete domains.
//! - Single-side optimization (one space against fixed opponents). The
//!   space's `variables` may name parameters from any faction; the
//!   runner does not enforce a single-owner constraint, so an analyst
//!   can also use this layer to evaluate joint-optimal postures by
//!   listing both sides' decision variables.
//! - Adversarial co-evolution (alternating best-response loop) and a
//!   first-class "defender posture" specialization (Epic I) are
//!   deferred to follow-up rounds.

use std::collections::BTreeMap;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloSummary};
use faultline_types::strategy_space::{DecisionVariable, Domain, SearchObjective, StrategySpace};

use crate::counterfactual::ParamOverride;
use crate::sensitivity::set_param;
use crate::{MonteCarloRunner, StatsError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Sampling strategy for a search run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMethod {
    /// Uniform random sampling. For continuous domains, draw uniformly
    /// from `[low, high)` (the high endpoint is approached arbitrarily
    /// closely but never sampled exactly — this is `rand::Rng::gen_range`
    /// semantics; use `Grid` if endpoint coverage matters). For discrete
    /// domains, pick uniformly from `values`. Trial count ==
    /// `SearchConfig.trials`.
    Random,
    /// Cartesian-product grid. Continuous variables expand into `steps`
    /// evenly-spaced values inclusive of both endpoints. Discrete
    /// variables enumerate their `values`. The first
    /// `SearchConfig.trials` cells of the product are evaluated, in the
    /// natural odometer order over the variable list. When the product
    /// is smaller than `trials`, only the available cells run; when it
    /// is larger, the truncated head is sampled deterministically (the
    /// last variable cycles fastest).
    Grid,
}

/// Inputs to a strategy-search run.
#[derive(Clone, Debug)]
pub struct SearchConfig {
    /// Number of trials to evaluate.
    pub trials: u32,
    /// Sampling strategy.
    pub method: SearchMethod,
    /// Seed for the search-only RNG. Independent of `mc_config.seed`.
    pub search_seed: u64,
    /// Inner Monte Carlo configuration applied to every trial.
    pub mc_config: MonteCarloConfig,
    /// Objectives to evaluate. The runner computes each objective's
    /// value on every trial; `best_by_objective` and `pareto_indices`
    /// are derived afterwards. Empty `objectives` is rejected — a
    /// search with no objectives produces no ranking.
    pub objectives: Vec<SearchObjective>,
    /// Compute a "do nothing" baseline trial (no decision-variable
    /// assignments applied) alongside the search trials. Used by the
    /// Counter-Recommendation report (Epic I) to anchor deltas.
    /// Defaults to `true` for caller convenience; flip to `false` when
    /// the extra MC batch is unwanted.
    pub compute_baseline: bool,
}

impl SearchConfig {
    /// Construct a `SearchConfig` with `compute_baseline = true`. Use
    /// `SearchConfig { compute_baseline: false, ..config }` when an
    /// existing instance needs the baseline disabled.
    pub fn new(
        trials: u32,
        method: SearchMethod,
        search_seed: u64,
        mc_config: MonteCarloConfig,
        objectives: Vec<SearchObjective>,
    ) -> Self {
        Self {
            trials,
            method,
            search_seed,
            mc_config,
            objectives,
            compute_baseline: true,
        }
    }
}

/// One trial's worth of decisions and their evaluated objectives.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchTrial {
    /// 0-based index in [`SearchResult::trials`]. `None` for the
    /// baseline trial in [`SearchResult::baseline`], which is not part
    /// of the indexed trial sequence.
    pub trial_index: Option<u32>,
    /// The variable assignments applied for this trial. Stored as
    /// `ParamOverride`s so an analyst can copy any single trial back as
    /// a `--counterfactual` invocation to reproduce it standalone.
    pub assignments: Vec<ParamOverride>,
    /// Objective values keyed by `SearchObjective::label()`. Always one
    /// entry per `SearchConfig.objectives` element.
    pub objective_values: BTreeMap<String, f64>,
    /// The Monte Carlo summary the objective values were derived from.
    pub summary: MonteCarloSummary,
}

/// Aggregate result of a strategy-search run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    /// Method used, surfaced for the report.
    pub method: SearchMethod,
    /// Trials in the order they were sampled.
    pub trials: Vec<SearchTrial>,
    /// Indices into `trials` for the non-dominated frontier across all
    /// objectives. Sorted ascending. A trial is dominated if some other
    /// trial is at least as good on every objective and strictly better
    /// on at least one. Returned indices respect the maximize/minimize
    /// direction declared by each `SearchObjective`.
    pub pareto_indices: Vec<u32>,
    /// Best trial index per objective label. Ties resolve by lowest
    /// trial index (i.e. the assignment that appears first in trial
    /// order wins) so output is reproducible across re-runs.
    pub best_by_objective: BTreeMap<String, u32>,
    /// Echo of the objectives evaluated, in the order supplied. Lets
    /// the report renderer iterate without redeclaring the order.
    pub objectives: Vec<SearchObjective>,
    /// "Do nothing" reference run — the scenario evaluated with no
    /// decision-variable assignment applied (Epic I).
    ///
    /// The Counter-Recommendation report section uses this as the
    /// comparison anchor: every Pareto-frontier trial is reported with
    /// `(objective_value - baseline_value)` deltas so an analyst sees
    /// what the posture investment buys vs. status quo. Reuses the
    /// inner Monte Carlo seed so the baseline is bit-identical to a
    /// `--single-run` of the same scenario at the same seed.
    ///
    /// `None` when `SearchConfig.compute_baseline = false` so legacy
    /// scenarios that opted out can still produce a search result
    /// without paying the extra MC batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<SearchTrial>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a strategy-search batch on the supplied scenario.
///
/// Returns `Err(StatsError::InvalidConfig)` if the scenario declares no
/// strategy space, the requested objectives list is empty, the trial
/// count is zero, any decision-variable domain is structurally
/// malformed (empty discrete values, inverted continuous bounds,
/// non-finite bounds, zero `steps`, NaN values), or any decision-
/// variable path fails to resolve via `set_param` on a clone of the
/// scenario.
///
/// Direct callers should typically have already passed the scenario
/// through `faultline_engine::validate_scenario`, which performs the
/// same structural checks at scenario load time. This function
/// re-validates anyway so a programmer wiring `run_search` against a
/// hand-constructed `Scenario` (e.g. in a test or a custom workflow)
/// gets the same guarantees as the CLI path.
pub fn run_search(scenario: &Scenario, config: &SearchConfig) -> Result<SearchResult, StatsError> {
    let space = &scenario.strategy_space;
    // Check `variables` directly rather than `StrategySpace::is_empty`:
    // the latter returns false when only objectives are declared, but
    // search needs at least one variable to have anything to sample.
    if space.variables.is_empty() {
        return Err(StatsError::InvalidConfig(
            "scenario has no [strategy_space] declaration; \
             add a [strategy_space] block with at least one variable to use --search"
                .into(),
        ));
    }
    if config.trials == 0 {
        return Err(StatsError::InvalidConfig(
            "search trials must be > 0".into(),
        ));
    }
    if config.objectives.is_empty() {
        return Err(StatsError::InvalidConfig(
            "search requires at least one objective; \
             pass --search-objective on the CLI or list `objectives` in [strategy_space]"
                .into(),
        ));
    }

    // Structural validation of every decision variable's domain.
    // Mirrors the engine-side `validate_scenario` checks so direct
    // callers (tests, custom workflows) that bypassed the engine
    // validator get the same guarantees. Cheap to repeat — the work
    // is bounded by `space.variables.len()`.
    validate_search_inputs(space)?;

    // Path-resolution sanity check: refuse the run if any variable's
    // path doesn't resolve. We try a no-op assignment (read the current
    // value via get_param, then set it back) on a scenario clone so the
    // caller's scenario stays untouched. Catching this here turns a
    // mid-run "trial 17 failed" surprise into an up-front validation
    // error.
    let mut probe = scenario.clone();
    for var in &space.variables {
        let current = crate::sensitivity::get_param(&probe, &var.path).map_err(|e| {
            StatsError::InvalidConfig(format!("strategy_space variable `{}`: {e}", var.path))
        })?;
        set_param(&mut probe, &var.path, current).map_err(|e| {
            StatsError::InvalidConfig(format!(
                "strategy_space variable `{}` failed to round-trip via set_param: {e}",
                var.path
            ))
        })?;
    }

    info!(
        method = ?config.method,
        trials = config.trials,
        variables = space.variables.len(),
        objectives = config.objectives.len(),
        "starting strategy search"
    );

    let assignments = sample_assignments(space, config);
    let mut trials = Vec::with_capacity(assignments.len());
    for (i, assignment) in assignments.into_iter().enumerate() {
        let trial_index = u32::try_from(i).expect("trial count fits u32");
        debug!(trial_index, "evaluating trial");

        let mut variant = scenario.clone();
        for ov in &assignment {
            set_param(&mut variant, &ov.path, ov.value)?;
        }

        let mc = MonteCarloRunner::run(&config.mc_config, &variant)?;

        let mut objective_values = BTreeMap::new();
        for objective in &config.objectives {
            let v = evaluate_objective(objective, &mc.summary);
            objective_values.insert(objective.label(), v);
        }

        trials.push(SearchTrial {
            trial_index: Some(trial_index),
            assignments: assignment,
            objective_values,
            summary: mc.summary,
        });
    }

    let pareto_indices = compute_pareto_frontier(&trials, &config.objectives);
    let best_by_objective = compute_best_by_objective(&trials, &config.objectives);

    let baseline = if config.compute_baseline {
        debug!("evaluating baseline (no decision-variable assignment)");
        let mc = MonteCarloRunner::run(&config.mc_config, scenario)?;
        let mut objective_values = BTreeMap::new();
        for objective in &config.objectives {
            let v = evaluate_objective(objective, &mc.summary);
            objective_values.insert(objective.label(), v);
        }
        // The baseline reuses the trial schema so the report renderer
        // can read its objective values like a normal trial. Its
        // `trial_index` is `None` because it isn't part of the indexed
        // trial sequence — the baseline lives in its own `baseline`
        // field on `SearchResult`.
        Some(SearchTrial {
            trial_index: None,
            assignments: Vec::new(),
            objective_values,
            summary: mc.summary,
        })
    } else {
        None
    };

    info!(
        completed = trials.len(),
        pareto = pareto_indices.len(),
        baseline = baseline.is_some(),
        "strategy search complete"
    );

    Ok(SearchResult {
        method: config.method,
        trials,
        pareto_indices,
        best_by_objective,
        objectives: config.objectives.clone(),
        baseline,
    })
}

// ---------------------------------------------------------------------------
// Structural validation
// ---------------------------------------------------------------------------

/// Re-validate domain shapes for direct callers who skipped
/// `validate_scenario`. Same invariants as the engine validator.
fn validate_search_inputs(space: &StrategySpace) -> Result<(), StatsError> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for var in &space.variables {
        if var.path.is_empty() {
            return Err(StatsError::InvalidConfig(
                "strategy_space variable has empty path".into(),
            ));
        }
        if !seen.insert(var.path.as_str()) {
            return Err(StatsError::InvalidConfig(format!(
                "strategy_space variable path `{}` declared more than once",
                var.path
            )));
        }
        match &var.domain {
            Domain::Continuous { low, high, steps } => {
                if !low.is_finite() || !high.is_finite() {
                    return Err(StatsError::InvalidConfig(format!(
                        "strategy_space variable `{}` has non-finite continuous bounds",
                        var.path
                    )));
                }
                if low > high {
                    return Err(StatsError::InvalidConfig(format!(
                        "strategy_space variable `{}` has low ({low}) > high ({high})",
                        var.path
                    )));
                }
                if *steps == 0 {
                    return Err(StatsError::InvalidConfig(format!(
                        "strategy_space variable `{}` has steps == 0",
                        var.path
                    )));
                }
            },
            Domain::Discrete { values } => {
                if values.is_empty() {
                    return Err(StatsError::InvalidConfig(format!(
                        "strategy_space variable `{}` has empty discrete values",
                        var.path
                    )));
                }
                for v in values {
                    if !v.is_finite() {
                        return Err(StatsError::InvalidConfig(format!(
                            "strategy_space variable `{}` has non-finite discrete value {v}",
                            var.path
                        )));
                    }
                }
            },
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

/// Build the list of trial assignments according to method/seed.
///
/// Each entry is a list of `ParamOverride`s — one per declared decision
/// variable, in declaration order so the trial layout is deterministic
/// even when adding a new variable shifts later trials.
fn sample_assignments(space: &StrategySpace, config: &SearchConfig) -> Vec<Vec<ParamOverride>> {
    match config.method {
        SearchMethod::Random => sample_random(space, config),
        SearchMethod::Grid => sample_grid(space, config),
    }
}

fn sample_random(space: &StrategySpace, config: &SearchConfig) -> Vec<Vec<ParamOverride>> {
    let mut rng = ChaCha8Rng::seed_from_u64(config.search_seed);
    let mut out = Vec::with_capacity(config.trials as usize);
    for _ in 0..config.trials {
        let mut assignment = Vec::with_capacity(space.variables.len());
        for var in &space.variables {
            let value = sample_random_value(&mut rng, var);
            assignment.push(ParamOverride {
                path: var.path.clone(),
                value,
            });
        }
        out.push(assignment);
    }
    out
}

fn sample_random_value(rng: &mut ChaCha8Rng, var: &DecisionVariable) -> f64 {
    match &var.domain {
        Domain::Continuous { low, high, .. } => {
            // Half-open `[low, high)` from `gen_range` is fine for our
            // purposes — the high endpoint is approached arbitrarily
            // closely and grid mode separately enumerates exactly the
            // endpoints. If `low == high`, return `low` (degenerate).
            if low >= high {
                *low
            } else {
                rng.gen_range(*low..*high)
            }
        },
        Domain::Discrete { values } => {
            // `validate_search_inputs` rejects empty `values` before
            // sampling starts, so the empty branch here is unreachable
            // under the public API. Keep the defensive `0.0` so an
            // internal caller that bypasses validation can't panic
            // (workspace lints deny `unwrap`).
            if values.is_empty() {
                0.0
            } else {
                let idx = rng.gen_range(0..values.len());
                values[idx]
            }
        },
    }
}

fn sample_grid(space: &StrategySpace, config: &SearchConfig) -> Vec<Vec<ParamOverride>> {
    // Pre-compute each variable's discrete enumeration. Continuous with
    // `steps == 1` collapses to a single midpoint — that lets an author
    // pin a continuous variable while exploring others.
    let levels: Vec<Vec<f64>> = space
        .variables
        .iter()
        .map(|v| enumerate_levels(&v.domain))
        .collect();
    if levels.iter().any(|l| l.is_empty()) {
        // Shouldn't happen post-validation, but stay defensive.
        return Vec::new();
    }
    // Saturate the Cartesian-product size at usize::MAX. With no upper
    // bound on per-variable `steps`, multiplying a few large counts can
    // overflow usize and silently produce far fewer cells than the
    // analyst requested (or panic in debug). The cap is min'd against
    // `config.trials` immediately below, so a saturated total just means
    // "trials wins" — which is already the correct truncation behaviour.
    let total: usize = levels
        .iter()
        .map(|l| l.len())
        .try_fold(1usize, usize::checked_mul)
        .unwrap_or(usize::MAX);
    let cap = (config.trials as usize).min(total);

    let mut out = Vec::with_capacity(cap);
    for cell in 0..cap {
        let mut idx = cell;
        let mut assignment = Vec::with_capacity(space.variables.len());
        // Last variable cycles fastest (odometer over the levels).
        // Iterate from last to first, then reverse the assignment to
        // restore declaration order so the JSON output reads naturally.
        let mut reverse = Vec::with_capacity(space.variables.len());
        for (var, lvl) in space.variables.iter().rev().zip(levels.iter().rev()) {
            let pick = idx % lvl.len();
            idx /= lvl.len();
            reverse.push(ParamOverride {
                path: var.path.clone(),
                value: lvl[pick],
            });
        }
        reverse.reverse();
        assignment.extend(reverse);
        out.push(assignment);
    }
    out
}

fn enumerate_levels(domain: &Domain) -> Vec<f64> {
    match domain {
        Domain::Continuous { low, high, steps } => {
            let s = *steps as usize;
            if s == 0 {
                Vec::new()
            } else if s == 1 {
                // Midpoint: lets a one-step grid pin the variable at the
                // centre of its declared range.
                vec![(low + high) / 2.0]
            } else {
                let span = high - low;
                (0..s)
                    .map(|i| {
                        let t = (i as f64) / ((s - 1) as f64);
                        low + span * t
                    })
                    .collect()
            }
        },
        Domain::Discrete { values } => values.clone(),
    }
}

// ---------------------------------------------------------------------------
// Objective evaluation
// ---------------------------------------------------------------------------

/// Public projection of the internal objective evaluator. Round-two
/// callers (Epic H co-evolution) need to score a hand-built
/// [`MonteCarloSummary`] against an arbitrary [`SearchObjective`]
/// without re-running [`run_search`]. The internal `evaluate_objective`
/// stays private so its signature can churn freely; this thin wrapper
/// is the stable public API.
pub fn evaluate_objective_public(objective: &SearchObjective, summary: &MonteCarloSummary) -> f64 {
    evaluate_objective(objective, summary)
}

fn evaluate_objective(objective: &SearchObjective, summary: &MonteCarloSummary) -> f64 {
    use SearchObjective::*;
    match objective {
        MaximizeWinRate { faction } => summary.win_rates.get(faction).copied().unwrap_or(0.0),
        MinimizeDetection => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.detection_rate)
            .fold(0.0_f64, f64::max),
        MinimizeAttackerCost => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.mean_attacker_spend)
            .sum(),
        MaximizeCostAsymmetry => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.cost_asymmetry_ratio)
            .fold(f64::NEG_INFINITY, f64::max)
            .max(0.0),
        MinimizeDuration => summary.average_duration,
        // Defender-aligned objectives. These read the same underlying
        // CampaignSummary fields as their attacker-aligned mirrors but
        // flip the optimization direction (see `maximize()` on the
        // enum). Chains-empty cases stay sane: an empty fold over
        // `0.0` initial returns 0 (no chains → no detection / cost to
        // worry about).
        MaximizeAttackerCost => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.mean_attacker_spend)
            .sum(),
        MaximizeDetection => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.detection_rate)
            .fold(0.0_f64, f64::max),
        MinimizeDefenderCost => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.mean_defender_spend)
            .sum(),
        MinimizeMaxChainSuccess => summary
            .campaign_summaries
            .values()
            .map(|cs| cs.overall_success_rate)
            .fold(0.0_f64, f64::max),
    }
}

// ---------------------------------------------------------------------------
// Pareto frontier + best-by-objective
// ---------------------------------------------------------------------------

/// Direction-aware "is `a` at least as good as `b` on this objective"
/// relation. Used by the dominance check below.
fn weakly_better(a: f64, b: f64, maximize: bool) -> bool {
    if maximize { a >= b } else { a <= b }
}

/// Direction-aware strict-better.
fn strictly_better(a: f64, b: f64, maximize: bool) -> bool {
    if maximize { a > b } else { a < b }
}

fn compute_pareto_frontier(trials: &[SearchTrial], objectives: &[SearchObjective]) -> Vec<u32> {
    let n = trials.len();
    let mut frontier = Vec::new();
    for i in 0..n {
        let mut dominated = false;
        for j in 0..n {
            if i == j {
                continue;
            }
            if dominates(&trials[j], &trials[i], objectives) {
                dominated = true;
                break;
            }
        }
        if !dominated {
            frontier.push(u32::try_from(i).expect("trial count fits u32"));
        }
    }
    frontier
}

fn dominates(a: &SearchTrial, b: &SearchTrial, objectives: &[SearchObjective]) -> bool {
    let mut strictly_any = false;
    for obj in objectives {
        let label = obj.label();
        let av = a.objective_values.get(&label).copied().unwrap_or(0.0);
        let bv = b.objective_values.get(&label).copied().unwrap_or(0.0);
        let max = obj.maximize();
        if !weakly_better(av, bv, max) {
            return false;
        }
        if strictly_better(av, bv, max) {
            strictly_any = true;
        }
    }
    strictly_any
}

fn compute_best_by_objective(
    trials: &[SearchTrial],
    objectives: &[SearchObjective],
) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for obj in objectives {
        let label = obj.label();
        let max = obj.maximize();
        let mut best_idx: Option<usize> = None;
        let mut best_val: Option<f64> = None;
        for (i, t) in trials.iter().enumerate() {
            let v = t.objective_values.get(&label).copied().unwrap_or(0.0);
            let take = match best_val {
                None => true,
                Some(bv) => strictly_better(v, bv, max),
            };
            if take {
                best_idx = Some(i);
                best_val = Some(v);
            }
        }
        if let Some(i) = best_idx {
            out.insert(label, u32::try_from(i).expect("trial count fits u32"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
    use faultline_types::ids::{FactionId, ForceId, RegionId, VictoryId};
    use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::strategy::Doctrine;
    use faultline_types::strategy_space::{
        DecisionVariable, Domain, SearchObjective, StrategySpace,
    };
    use faultline_types::victory::{VictoryCondition, VictoryType};

    fn minimal_scenario() -> Scenario {
        let r1 = RegionId::from("region-a");
        let r2 = RegionId::from("region-b");
        let f_alpha = FactionId::from("alpha");
        let f_bravo = FactionId::from("bravo");

        let mut regions = BTreeMap::new();
        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "Region A".into(),
                population: 100_000,
                urbanization: 0.5,
                initial_control: Some(f_alpha.clone()),
                strategic_value: 5.0,
                borders: vec![r2.clone()],
                centroid: None,
            },
        );
        regions.insert(
            r2.clone(),
            Region {
                id: r2.clone(),
                name: "Region B".into(),
                population: 50_000,
                urbanization: 0.3,
                initial_control: Some(f_bravo.clone()),
                strategic_value: 3.0,
                borders: vec![r1.clone()],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(
            f_alpha.clone(),
            make_faction(f_alpha.clone(), "Alpha", r1.clone()),
        );
        factions.insert(
            f_bravo.clone(),
            make_faction(f_bravo.clone(), "Bravo", r2.clone()),
        );

        let mut victory_conditions = BTreeMap::new();
        let vc_id = VictoryId::from("alpha-win");
        victory_conditions.insert(
            vc_id.clone(),
            VictoryCondition {
                id: vc_id,
                name: "Alpha Dominance".into(),
                faction: f_alpha,
                condition: VictoryType::MilitaryDominance {
                    enemy_strength_below: 0.01,
                },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "Search Test Scenario".into(),
                description: "Minimal scenario for search tests".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 2,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![
                    TerrainModifier {
                        region: r1,
                        terrain_type: TerrainType::Urban,
                        movement_modifier: 1.0,
                        defense_modifier: 1.0,
                        visibility: 0.8,
                    },
                    TerrainModifier {
                        region: r2,
                        terrain_type: TerrainType::Rural,
                        movement_modifier: 1.0,
                        defense_modifier: 1.0,
                        visibility: 1.0,
                    },
                ],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.4,
                institutional_trust: 0.5,
                population_segments: vec![],
                global_modifiers: vec![],
                media_landscape: MediaLandscape {
                    fragmentation: 0.3,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.2,
                    social_media_penetration: 0.5,
                    internet_availability: 0.8,
                },
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 50,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(0xCAFE),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: StrategySpace::default(),
            networks: std::collections::BTreeMap::new(),
        }
    }

    fn make_faction(id: FactionId, name: &str, region: RegionId) -> Faction {
        let force_id = ForceId::from(format!("{}-inf", id));
        let mut forces = BTreeMap::new();
        forces.insert(
            force_id.clone(),
            ForceUnit {
                id: force_id,
                name: format!("{name} Infantry"),
                unit_type: UnitType::Infantry,
                region,
                strength: 100.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );
        Faction {
            id,
            name: name.to_string(),
            description: String::new(),
            color: "#000000".into(),
            faction_type: FactionType::Insurgent,
            forces,
            tech_access: vec![],
            initial_morale: 0.7,
            logistics_capacity: 10.0,
            initial_resources: 100.0,
            resource_rate: 5.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
            leadership: None,
        }
    }

    fn search_scenario() -> Scenario {
        let mut s = minimal_scenario();
        s.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.alpha.initial_morale".into(),
                owner: Some(FactionId::from("alpha")),
                domain: Domain::Continuous {
                    low: 0.3,
                    high: 0.9,
                    steps: 4,
                },
            }],
            objectives: vec![],
            attacker_profiles: Vec::new(),
        };
        s
    }

    fn config(method: SearchMethod, trials: u32) -> SearchConfig {
        SearchConfig {
            trials,
            method,
            search_seed: 12345,
            mc_config: MonteCarloConfig {
                num_runs: 4,
                seed: Some(0xBEEF),
                collect_snapshots: false,
                parallel: false,
            },
            objectives: vec![SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            }],
            // Most existing tests don't care about the baseline trial
            // and benefit from skipping the extra MC batch. The
            // baseline-specific tests below explicitly flip this on.
            compute_baseline: false,
        }
    }

    #[test]
    fn search_rejects_empty_strategy_space() {
        let s = minimal_scenario();
        let err =
            run_search(&s, &config(SearchMethod::Random, 4)).expect_err("empty space must reject");
        match err {
            StatsError::InvalidConfig(msg) => assert!(msg.contains("strategy_space")),
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn search_rejects_zero_trials() {
        let s = search_scenario();
        let err =
            run_search(&s, &config(SearchMethod::Random, 0)).expect_err("zero trials must reject");
        assert!(matches!(err, StatsError::InvalidConfig(_)));
    }

    #[test]
    fn search_rejects_empty_objectives() {
        let s = search_scenario();
        let mut c = config(SearchMethod::Random, 4);
        c.objectives.clear();
        let err = run_search(&s, &c).expect_err("no objectives must reject");
        assert!(matches!(err, StatsError::InvalidConfig(_)));
    }

    #[test]
    fn search_then_evaluate_is_deterministic() {
        // Two consecutive runs against the same scenario+config must
        // produce bit-identical SearchResult JSON. This is the core
        // determinism contract — without it the manifest replay path
        // for --search would be unreachable.
        let s = search_scenario();
        let c = config(SearchMethod::Random, 6);
        let r1 = run_search(&s, &c).expect("first search");
        let r2 = run_search(&s, &c).expect("second search");
        let j1 = serde_json::to_string(&r1).expect("serialize r1");
        let j2 = serde_json::to_string(&r2).expect("serialize r2");
        assert_eq!(j1, j2, "search must be deterministic under fixed seeds");
    }

    #[test]
    fn search_seed_independent_of_mc_seed() {
        // Changing the inner MC seed must change trial outcomes (the
        // summary will be different) but must NOT change the trial
        // *assignments* — the search seed is the sole driver of the
        // assignment list.
        let s = search_scenario();
        let mut a = config(SearchMethod::Random, 4);
        let mut b = config(SearchMethod::Random, 4);
        a.mc_config.seed = Some(1);
        b.mc_config.seed = Some(2);
        let ra = run_search(&s, &a).expect("a");
        let rb = run_search(&s, &b).expect("b");

        let assignments_a: Vec<_> = ra
            .trials
            .iter()
            .map(|t| {
                t.assignments
                    .iter()
                    .map(|a| (a.path.clone(), a.value))
                    .collect::<Vec<_>>()
            })
            .collect();
        let assignments_b: Vec<_> = rb
            .trials
            .iter()
            .map(|t| {
                t.assignments
                    .iter()
                    .map(|a| (a.path.clone(), a.value))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            assignments_a, assignments_b,
            "search assignments must be independent of mc_config.seed"
        );
    }

    #[test]
    fn grid_method_enumerates_endpoints() {
        let s = search_scenario();
        let c = config(SearchMethod::Grid, 8);
        let r = run_search(&s, &c).expect("grid search");
        // Variable has steps=4 over [0.3, 0.9] → expects {0.3, 0.5,
        // 0.7, 0.9}. Only one variable, so trials = 4 (the cap clamps
        // to product size).
        assert_eq!(r.trials.len(), 4);
        let values: Vec<f64> = r.trials.iter().map(|t| t.assignments[0].value).collect();
        for &expected in &[0.3, 0.5, 0.7, 0.9] {
            assert!(
                values.iter().any(|v| (v - expected).abs() < 1e-9),
                "grid must include endpoint {expected}, got {values:?}"
            );
        }
    }

    #[test]
    fn pareto_frontier_drops_dominated_trials() {
        // Hand-construct trials with two objectives so we exercise the
        // dominance check without re-running a full MC batch.
        use faultline_types::stats::MonteCarloSummary;
        let mk = |idx: u32, win: f64, det: f64| SearchTrial {
            trial_index: Some(idx),
            assignments: vec![],
            objective_values: {
                let mut m = BTreeMap::new();
                m.insert(
                    SearchObjective::MaximizeWinRate {
                        faction: FactionId::from("alpha"),
                    }
                    .label(),
                    win,
                );
                m.insert(SearchObjective::MinimizeDetection.label(), det);
                m
            },
            summary: MonteCarloSummary {
                total_runs: 1,
                win_rates: BTreeMap::new(),
                win_rate_cis: BTreeMap::new(),
                average_duration: 0.0,
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
            },
        };
        // Trial 0 dominates trial 1 (better win, equal detection).
        // Trial 2 is non-dominated (better detection, lower win).
        let trials = vec![mk(0, 0.8, 0.4), mk(1, 0.6, 0.4), mk(2, 0.5, 0.1)];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let frontier = compute_pareto_frontier(&trials, &objectives);
        assert_eq!(frontier, vec![0, 2]);
    }

    #[test]
    fn best_by_objective_picks_correct_direction() {
        use faultline_types::stats::MonteCarloSummary;
        let mk = |idx: u32, win: f64, det: f64| SearchTrial {
            trial_index: Some(idx),
            assignments: vec![],
            objective_values: {
                let mut m = BTreeMap::new();
                m.insert(
                    SearchObjective::MaximizeWinRate {
                        faction: FactionId::from("alpha"),
                    }
                    .label(),
                    win,
                );
                m.insert(SearchObjective::MinimizeDetection.label(), det);
                m
            },
            summary: MonteCarloSummary {
                total_runs: 1,
                win_rates: BTreeMap::new(),
                win_rate_cis: BTreeMap::new(),
                average_duration: 0.0,
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
            },
        };
        let trials = vec![mk(0, 0.8, 0.4), mk(1, 0.6, 0.1), mk(2, 0.9, 0.5)];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let best = compute_best_by_objective(&trials, &objectives);
        assert_eq!(
            best.get(
                &SearchObjective::MaximizeWinRate {
                    faction: FactionId::from("alpha")
                }
                .label()
            ),
            Some(&2u32),
            "max-win should pick trial 2 (highest win)"
        );
        assert_eq!(
            best.get(&SearchObjective::MinimizeDetection.label()),
            Some(&1u32),
            "min-detection should pick trial 1 (lowest detection)"
        );
    }

    // ----- Helper for hand-constructed trials in invariant tests -----

    fn empty_summary() -> faultline_types::stats::MonteCarloSummary {
        faultline_types::stats::MonteCarloSummary {
            total_runs: 1,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 0.0,
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
        }
    }

    fn trial_with_values(idx: u32, kvs: &[(&str, f64)]) -> SearchTrial {
        let mut m = BTreeMap::new();
        for (k, v) in kvs {
            m.insert((*k).to_string(), *v);
        }
        SearchTrial {
            trial_index: Some(idx),
            assignments: vec![],
            objective_values: m,
            summary: empty_summary(),
        }
    }

    // ----- Pareto invariant tests -----

    #[test]
    fn pareto_frontier_is_idempotent_under_recomputation() {
        // Recomputing the frontier on the same input always returns
        // the same indices in the same order.
        let trials = vec![
            trial_with_values(
                0,
                &[
                    ("maximize_win_rate:alpha", 0.8),
                    ("minimize_detection", 0.4),
                ],
            ),
            trial_with_values(
                1,
                &[
                    ("maximize_win_rate:alpha", 0.4),
                    ("minimize_detection", 0.1),
                ],
            ),
            trial_with_values(
                2,
                &[
                    ("maximize_win_rate:alpha", 0.6),
                    ("minimize_detection", 0.2),
                ],
            ),
        ];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let a = compute_pareto_frontier(&trials, &objectives);
        let b = compute_pareto_frontier(&trials, &objectives);
        assert_eq!(a, b, "frontier must be deterministic across recomputation");
    }

    #[test]
    fn pareto_frontier_single_trial_includes_self() {
        let trials = vec![trial_with_values(
            0,
            &[
                ("maximize_win_rate:alpha", 0.5),
                ("minimize_detection", 0.5),
            ],
        )];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let frontier = compute_pareto_frontier(&trials, &objectives);
        assert_eq!(
            frontier,
            vec![0],
            "the only trial must be on its own frontier"
        );
    }

    #[test]
    fn pareto_frontier_keeps_identical_objective_values() {
        // Two trials with identical objective vectors: neither
        // strictly-dominates the other, both survive on the frontier.
        let trials = vec![
            trial_with_values(
                0,
                &[
                    ("maximize_win_rate:alpha", 0.7),
                    ("minimize_detection", 0.3),
                ],
            ),
            trial_with_values(
                1,
                &[
                    ("maximize_win_rate:alpha", 0.7),
                    ("minimize_detection", 0.3),
                ],
            ),
        ];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let frontier = compute_pareto_frontier(&trials, &objectives);
        assert_eq!(frontier, vec![0, 1]);
    }

    #[test]
    fn pareto_frontier_all_dominated_by_one() {
        // Trial 0 strictly dominates 1 and 2. Frontier is just [0].
        let trials = vec![
            trial_with_values(
                0,
                &[
                    ("maximize_win_rate:alpha", 0.9),
                    ("minimize_detection", 0.1),
                ],
            ),
            trial_with_values(
                1,
                &[
                    ("maximize_win_rate:alpha", 0.4),
                    ("minimize_detection", 0.4),
                ],
            ),
            trial_with_values(
                2,
                &[
                    ("maximize_win_rate:alpha", 0.2),
                    ("minimize_detection", 0.5),
                ],
            ),
        ];
        let objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDetection,
        ];
        let frontier = compute_pareto_frontier(&trials, &objectives);
        assert_eq!(frontier, vec![0]);
    }

    #[test]
    fn best_by_objective_ties_resolve_to_lowest_index() {
        // Two trials tied on the objective value: the lower-index
        // trial wins so output is reproducible.
        let trials = vec![
            trial_with_values(0, &[("minimize_duration", 30.0)]),
            trial_with_values(1, &[("minimize_duration", 30.0)]),
            trial_with_values(2, &[("minimize_duration", 60.0)]),
        ];
        let objectives = vec![SearchObjective::MinimizeDuration];
        let best = compute_best_by_objective(&trials, &objectives);
        assert_eq!(
            best.get(&SearchObjective::MinimizeDuration.label()),
            Some(&0u32),
            "ties must resolve to lowest index"
        );
    }

    // ----- Domain enumeration tests -----

    #[test]
    fn enumerate_levels_continuous_steps_one_uses_midpoint() {
        let levels = enumerate_levels(&Domain::Continuous {
            low: 0.2,
            high: 0.8,
            steps: 1,
        });
        assert_eq!(levels.len(), 1);
        assert!((levels[0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn enumerate_levels_continuous_includes_endpoints() {
        let levels = enumerate_levels(&Domain::Continuous {
            low: 0.0,
            high: 1.0,
            steps: 5,
        });
        assert_eq!(levels.len(), 5);
        assert!((levels[0] - 0.0).abs() < 1e-9);
        assert!((levels[4] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn enumerate_levels_discrete_passes_through() {
        let levels = enumerate_levels(&Domain::Discrete {
            values: vec![0.1, 0.5, 0.9],
        });
        assert_eq!(levels, vec![0.1, 0.5, 0.9]);
    }

    #[test]
    fn sample_grid_saturates_on_overflowing_product() {
        // Regression: the Cartesian-product size used to be computed
        // with `iter().map(...).product()`, which overflows usize
        // silently in release mode (or panics in debug). With nine
        // discrete variables of 256 values each, the product is
        // 256^9 = 2^72, which exceeds usize::MAX on a 64-bit target.
        // The fix saturates the product at usize::MAX so `cap` falls
        // back to `trials` and sampling proceeds normally.
        let levels: Vec<f64> = (0..256).map(|i| i as f64).collect();
        let variables: Vec<DecisionVariable> = (0..9)
            .map(|i| DecisionVariable {
                path: format!("synthetic.var.{i}"),
                owner: None,
                domain: Domain::Discrete {
                    values: levels.clone(),
                },
            })
            .collect();
        let space = StrategySpace {
            variables,
            objectives: vec![],
            attacker_profiles: Vec::new(),
        };
        let cfg = SearchConfig {
            trials: 4,
            method: SearchMethod::Grid,
            search_seed: 0,
            mc_config: MonteCarloConfig {
                num_runs: 1,
                seed: Some(0),
                collect_snapshots: false,
                parallel: false,
            },
            objectives: vec![SearchObjective::MinimizeDuration],
            compute_baseline: false,
        };
        // Direct call to sample_grid bypasses run_search's path-resolution
        // probe (the synthetic paths don't exist in any real scenario).
        let cells = sample_grid(&space, &cfg);
        assert_eq!(
            cells.len(),
            4,
            "trial cap must be honored even when the product overflows"
        );
        // Each cell carries one value per variable.
        for cell in &cells {
            assert_eq!(cell.len(), 9);
        }
    }

    // ----- Objective evaluation edge cases -----

    #[test]
    fn evaluate_minimize_detection_empty_chains_returns_zero() {
        // No campaign_summaries → fold() over empty iterator with 0.0
        // initial value returns 0.0. Document and pin the behaviour.
        let summary = empty_summary();
        let v = evaluate_objective(&SearchObjective::MinimizeDetection, &summary);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn evaluate_maximize_cost_asymmetry_empty_chains_clamped_to_zero() {
        // Empty fold seed is NEG_INFINITY; the .max(0.0) post-fold
        // clamps to a sane "no chains, no asymmetry" value.
        let summary = empty_summary();
        let v = evaluate_objective(&SearchObjective::MaximizeCostAsymmetry, &summary);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn evaluate_defender_objectives_empty_chains_return_zero() {
        // Mirror of the attacker-aligned empty-chains tests above for
        // the four Epic I defender objectives. All four sum / fold over
        // `summary.campaign_summaries`; an empty map must produce a
        // sane 0.0 rather than a NaN, NEG_INFINITY, or panic. Pinning
        // this behaviour means a future renderer can read the value
        // without a special "no chains" branch.
        let summary = empty_summary();
        for obj in [
            SearchObjective::MaximizeAttackerCost,
            SearchObjective::MaximizeDetection,
            SearchObjective::MinimizeDefenderCost,
            SearchObjective::MinimizeMaxChainSuccess,
        ] {
            let v = evaluate_objective(&obj, &summary);
            assert_eq!(
                v,
                0.0,
                "empty-chains evaluation of {} must be 0.0, got {v}",
                obj.label()
            );
        }
    }

    #[test]
    fn evaluate_defender_objectives_read_expected_summary_fields() {
        // Each defender-aligned objective reads a specific field on
        // `CampaignSummary`. Build a synthetic summary with two chains
        // and verify the evaluator sees the values it should — sums for
        // the cost-style objectives, max-fold for the rate-style ones.
        use faultline_types::ids::KillChainId;
        use faultline_types::stats::{CampaignSummary, MonteCarloSummary};

        let mut campaigns = BTreeMap::new();
        let mk = |id: &str, succ: f64, det: f64, atk: f64, def: f64| CampaignSummary {
            chain_id: KillChainId::from(id),
            phase_stats: BTreeMap::new(),
            overall_success_rate: succ,
            detection_rate: det,
            mean_attacker_spend: atk,
            mean_defender_spend: def,
            cost_asymmetry_ratio: 0.0,
            mean_attribution_confidence: 0.0,
            time_to_first_detection: None,
            defender_reaction_time: None,
            phase_survival: BTreeMap::new(),
        };
        campaigns.insert(KillChainId::from("c1"), mk("c1", 0.4, 0.5, 100.0, 50.0));
        campaigns.insert(KillChainId::from("c2"), mk("c2", 0.7, 0.2, 30.0, 80.0));

        let summary = MonteCarloSummary {
            total_runs: 10,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 0.0,
            metric_distributions: BTreeMap::new(),
            regional_control: BTreeMap::new(),
            event_probabilities: BTreeMap::new(),
            campaign_summaries: campaigns,
            feasibility_matrix: vec![],
            seam_scores: BTreeMap::new(),
            correlation_matrix: None,
            pareto_frontier: None,
            defender_capacity: Vec::new(),
            network_summaries: std::collections::BTreeMap::new(),
        };

        // Cost-style objectives sum across chains.
        assert_eq!(
            evaluate_objective(&SearchObjective::MaximizeAttackerCost, &summary),
            130.0,
            "MaximizeAttackerCost must sum mean_attacker_spend across chains"
        );
        assert_eq!(
            evaluate_objective(&SearchObjective::MinimizeDefenderCost, &summary),
            130.0,
            "MinimizeDefenderCost must sum mean_defender_spend across chains"
        );

        // Rate-style objectives take the max across chains.
        assert!(
            (evaluate_objective(&SearchObjective::MaximizeDetection, &summary) - 0.5).abs() < 1e-9,
            "MaximizeDetection must take the max detection_rate"
        );
        assert!(
            (evaluate_objective(&SearchObjective::MinimizeMaxChainSuccess, &summary) - 0.7).abs()
                < 1e-9,
            "MinimizeMaxChainSuccess must take the max overall_success_rate"
        );
    }

    // ----- Structural validation -----

    #[test]
    fn run_search_validates_empty_discrete_directly() {
        // Validator catches this at scenario load, but the runner must
        // also catch it for direct callers who hand-built a Scenario.
        let mut s = search_scenario();
        s.strategy_space.variables = vec![DecisionVariable {
            path: "faction.alpha.initial_morale".into(),
            owner: None,
            domain: Domain::Discrete { values: vec![] },
        }];
        let err = run_search(&s, &config(SearchMethod::Random, 4))
            .expect_err("empty discrete must reject");
        match err {
            StatsError::InvalidConfig(msg) => {
                assert!(msg.contains("empty discrete"), "unexpected message: {msg}");
            },
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn run_search_validates_inverted_continuous_directly() {
        let mut s = search_scenario();
        s.strategy_space.variables = vec![DecisionVariable {
            path: "faction.alpha.initial_morale".into(),
            owner: None,
            domain: Domain::Continuous {
                low: 0.9,
                high: 0.1,
                steps: 2,
            },
        }];
        let err =
            run_search(&s, &config(SearchMethod::Random, 4)).expect_err("low > high must reject");
        match err {
            StatsError::InvalidConfig(msg) => assert!(msg.contains("low")),
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn run_search_validates_duplicate_paths_directly() {
        let mut s = search_scenario();
        let dup = DecisionVariable {
            path: "faction.alpha.initial_morale".into(),
            owner: None,
            domain: Domain::Continuous {
                low: 0.1,
                high: 0.9,
                steps: 4,
            },
        };
        s.strategy_space.variables = vec![dup.clone(), dup];
        let err = run_search(&s, &config(SearchMethod::Random, 4))
            .expect_err("duplicate path must reject");
        match err {
            StatsError::InvalidConfig(msg) => assert!(msg.contains("declared more than once")),
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    // ----- Discrete domain sampling -----

    #[test]
    fn random_discrete_only_samples_declared_values() {
        // Every drawn value must appear in the declared `values`.
        let mut s = search_scenario();
        s.strategy_space.variables = vec![DecisionVariable {
            path: "faction.alpha.initial_morale".into(),
            owner: None,
            domain: Domain::Discrete {
                values: vec![0.3, 0.6, 0.9],
            },
        }];
        let result = run_search(&s, &config(SearchMethod::Random, 32)).expect("random search");
        for trial in &result.trials {
            let v = trial.assignments[0].value;
            assert!(
                [0.3, 0.6, 0.9].iter().any(|d| (d - v).abs() < 1e-9),
                "drawn value {v} not in declared discrete set"
            );
        }
    }

    // ----- TOML / JSON round-trips -----

    #[test]
    fn strategy_space_round_trips_through_toml() {
        // Serialize a Scenario with a strategy_space, parse it back,
        // and assert the strategy_space is byte-identical.
        let mut s = search_scenario();
        s.strategy_space = StrategySpace {
            variables: vec![
                DecisionVariable {
                    path: "faction.alpha.initial_morale".into(),
                    owner: Some(FactionId::from("alpha")),
                    domain: Domain::Continuous {
                        low: 0.3,
                        high: 0.9,
                        steps: 4,
                    },
                },
                DecisionVariable {
                    path: "political_climate.tension".into(),
                    owner: None,
                    domain: Domain::Discrete {
                        values: vec![0.2, 0.5, 0.8],
                    },
                },
            ],
            objectives: vec![
                SearchObjective::MaximizeWinRate {
                    faction: FactionId::from("alpha"),
                },
                SearchObjective::MinimizeDuration,
            ],
            attacker_profiles: Vec::new(),
        };
        let toml_str = toml::to_string(&s).expect("serialize TOML");
        let parsed: Scenario = toml::from_str(&toml_str).expect("parse TOML");
        assert_eq!(
            parsed.strategy_space, s.strategy_space,
            "strategy_space must round-trip through TOML"
        );
    }

    #[test]
    fn search_result_round_trips_through_json() {
        // The full SearchResult shape (including SearchMethod and
        // objective enum variants) must round-trip via serde_json so
        // the search.json artifact can be reloaded without loss.
        let s = search_scenario();
        let result = run_search(&s, &config(SearchMethod::Grid, 4)).expect("grid search");
        let json = serde_json::to_string(&result).expect("serialize JSON");
        let parsed: SearchResult = serde_json::from_str(&json).expect("parse JSON");
        let json2 = serde_json::to_string(&parsed).expect("re-serialize JSON");
        assert_eq!(json, json2, "SearchResult must round-trip via JSON");
    }

    #[test]
    fn baseline_runs_when_enabled_and_carries_objective_values() {
        // With `compute_baseline = true`, the runner emits a baseline
        // trial holding the scenario's natural objective values (no
        // decision-variable assignment). Without the toggle, no
        // baseline is emitted (back-compat).
        let s = search_scenario();

        let mut c = config(SearchMethod::Random, 3);
        c.compute_baseline = true;
        c.objectives = vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDuration,
        ];
        let result = run_search(&s, &c).expect("search with baseline");
        let baseline = result.baseline.as_ref().expect("baseline present");
        assert!(
            baseline.trial_index.is_none(),
            "baseline carries no trial index"
        );
        assert!(
            baseline.assignments.is_empty(),
            "baseline carries no assignments"
        );
        for obj in &c.objectives {
            assert!(
                baseline.objective_values.contains_key(&obj.label()),
                "baseline missing objective {}",
                obj.label()
            );
        }

        let mut c_off = c.clone();
        c_off.compute_baseline = false;
        let r_off = run_search(&s, &c_off).expect("search without baseline");
        assert!(r_off.baseline.is_none(), "compute_baseline=false → None");
    }

    #[test]
    fn baseline_objective_matches_a_zero_assignment_run() {
        // The baseline must produce the same objective values as if we
        // had run a Monte Carlo batch on the unmodified scenario at the
        // same seed. This is the determinism contract that lets the
        // Counter-Recommendation deltas be reproducible.
        let s = search_scenario();
        let mut c = config(SearchMethod::Random, 1);
        c.compute_baseline = true;
        let r = run_search(&s, &c).expect("search");
        let baseline = r.baseline.expect("baseline");

        // Re-run the same MC config independently; objective values
        // must match exactly (same seed → same outcome distribution).
        let mc = MonteCarloRunner::run(&c.mc_config, &s).expect("standalone MC");
        for obj in &c.objectives {
            let label = obj.label();
            let bv = baseline
                .objective_values
                .get(&label)
                .copied()
                .unwrap_or(0.0);
            let direct = evaluate_objective(obj, &mc.summary);
            assert!(
                (bv - direct).abs() < 1e-12,
                "baseline {label} = {bv} must match direct evaluation {direct}",
            );
        }
    }

    #[test]
    fn search_method_serde_round_trip_snake_case() {
        // Pin the wire format: variants are serialized as
        // snake_case strings in JSON.
        let m = SearchMethod::Random;
        let s = serde_json::to_string(&m).expect("serialize");
        assert_eq!(s, r#""random""#);
        let m: SearchMethod = serde_json::from_str(r#""grid""#).expect("parse");
        assert_eq!(m, SearchMethod::Grid);
    }
}
