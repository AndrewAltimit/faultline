//! Defender-posture robustness analysis (Epic I — round two).
//!
//! Closes the deferred fourth item from Epic I round-one by adding a
//! cross-product evaluation layer: given a set of *defender postures*
//! (each one a full assignment to defender-owned decision-variable
//! paths) and a set of *attacker profiles* (named attacker-side
//! assignments declared in `[strategy_space.attacker_profiles]`),
//! evaluate every (posture, profile) cell via Monte Carlo and surface
//! per-posture rollups so an analyst can see "this posture wins under
//! profile A and C but loses under B."
//!
//! ## How this fits with `--search` and `--coevolve`
//!
//! - `--search` tells the analyst *which* defender postures are
//!   non-dominated against a single (implicit) attacker baseline.
//! - `--coevolve` tells the analyst what equilibrium emerges when both
//!   sides re-optimise.
//! - `--robustness` tells the analyst *how fragile* a fixed defender
//!   posture is against a range of attacker strategies. The expected
//!   workflow is search → robustness on the Pareto frontier: rank
//!   postures by their worst-case profile rather than their
//!   single-baseline score.
//!
//! ## Determinism contract
//!
//! Two seeds are deliberately separated, mirroring the `search` /
//! `coevolve` pattern:
//!
//! - `RobustnessConfig.mc_config.seed` — drives the inner Monte Carlo
//!   evaluation of each cell. Identical across every (posture, profile)
//!   cell so cell-to-cell deltas reflect parameter changes only, not
//!   sampling noise. This matches the `--counterfactual` invariant: the
//!   same seed under different parameters reproduces the same delta.
//! - The robustness layer itself has **no RNG**. Postures and profiles
//!   are iterated in deterministic order (postures: caller-supplied
//!   order; profiles: scenario declaration order). There is no sampling
//!   step, no shuffling. Same `(scenario, postures, profiles, mc_seed)`
//!   always produces the same `RobustnessResult`.
//!
//! ## Where postures come from
//!
//! The `RobustnessConfig` takes postures as a plain list of assignment
//! vectors so the runner stays decoupled from the search pipeline. The
//! CLI populates the list from one of three sources:
//!
//! - A saved `search.json` (`--robustness-from-search <path>`): the
//!   CLI extracts the Pareto-frontier trials and lifts each into a
//!   `DefenderPosture`. This is the typical analyst flow.
//! - A single inline posture (future extension; not wired in round
//!   two).
//! - No postures at all + `include_baseline = true`: the runner
//!   evaluates every profile against the scenario's natural state.
//!   Useful for sanity-checking that the profiles apply cleanly before
//!   running a full search.
//!
//! ## Path-collision semantics
//!
//! When a posture and a profile assign to the same parameter path, the
//! profile wins (it is applied second, after the posture). This is by
//! design: the natural authoring convention is "postures touch
//! defender-controlled parameters, profiles touch attacker-controlled
//! parameters", and a deliberate overlap typically expresses an
//! attacker action that overrides a defender investment (e.g. an
//! attacker capability that bypasses a defender's monitoring posture).
//! The schema does not enforce ownership separation because the dotted-
//! path layer doesn't carry an ownership label; if an analyst wants to
//! detect collisions they should hash both sets of paths and compare
//! at the call site. Within a *single* posture or profile, duplicate
//! paths are rejected by validation so the order-of-application is
//! never ambiguous in either direction.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloSummary};
use faultline_types::strategy_space::{AttackerProfile, SearchObjective};

use crate::counterfactual::ParamOverride;
use crate::sensitivity::set_param;
use crate::{MonteCarloRunner, StatsError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One defender posture supplied to the robustness runner.
///
/// `label` is whatever the caller wants to display; the CLI uses
/// `"posture_<trial_index>"` for postures lifted from a saved search,
/// and `"baseline"` for the natural-state row when
/// `include_baseline = true`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DefenderPosture {
    /// Display label, surfaced in the report and the JSON output.
    pub label: String,
    /// The assignments applied to the scenario before evaluation. Empty
    /// vector is allowed and represents the natural-state baseline.
    /// The runner applies these via `set_param` in declaration order;
    /// callers wanting deterministic ordering across re-runs should
    /// sort by `path` before constructing the posture.
    pub assignments: Vec<ParamOverride>,
}

/// Inputs to a robustness run.
#[derive(Clone, Debug)]
pub struct RobustnessConfig {
    /// Postures to evaluate. Must be non-empty unless
    /// `include_baseline = true` and the runner is allowed to evaluate
    /// only the baseline.
    pub postures: Vec<DefenderPosture>,
    /// Whether to evaluate the scenario's natural state as an extra
    /// "do-nothing posture" row, anchored at the front of the result.
    /// Useful as a comparison reference: every posture's worst-case
    /// profile reads against the baseline's outcome on the same profile.
    pub include_baseline: bool,
    /// Inner Monte Carlo configuration applied to every cell.
    pub mc_config: MonteCarloConfig,
    /// Objectives to evaluate per cell. Empty `objectives` is rejected
    /// — robustness ranking needs at least one metric to rank against.
    /// Unlike search, robustness does **not** consult
    /// `scenario.strategy_space.objectives` as a fallback: an analyst
    /// may want to evaluate robustness against a different metric than
    /// the search optimised for, so requiring an explicit list here
    /// makes that decision visible.
    pub objectives: Vec<SearchObjective>,
}

/// One (posture, profile) Monte Carlo evaluation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobustnessCell {
    /// Posture label, matching `DefenderPosture.label` (or
    /// `"baseline"` for the natural-state row).
    pub posture_label: String,
    /// Profile name, matching `AttackerProfile.name` (or
    /// `"baseline_attacker"` for the natural-attacker row when no
    /// profiles are declared).
    pub profile_name: String,
    /// Objective values, keyed by `SearchObjective::label()`. One entry
    /// per `RobustnessConfig.objectives` element.
    pub objective_values: BTreeMap<String, f64>,
    /// Full Monte Carlo summary the values were derived from. Lets the
    /// JSON consumer drill into a specific cell without re-running.
    pub summary: MonteCarloSummary,
}

/// Per-posture aggregate across profiles. Exists to make the report's
/// "rank by worst-case profile" question cheap to render.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PostureRollup {
    pub posture_label: String,
    /// Per-objective: the profile name (and value) that produced the
    /// **worst** outcome for this posture under that objective. Worst
    /// is direction-aware — for a `MaximizeWinRate` objective, it's the
    /// profile minimizing the value; for a `MinimizeMaxChainSuccess`
    /// objective, it's the profile maximizing the value.
    pub worst_per_objective: BTreeMap<String, NamedValue>,
    /// Per-objective: the profile / value combination producing the
    /// best outcome. Direction-aware mirror of `worst_per_objective`.
    pub best_per_objective: BTreeMap<String, NamedValue>,
    /// Per-objective: arithmetic mean across profiles. Useful when an
    /// analyst wants "average-case" rather than worst-case ranking.
    pub mean_per_objective: BTreeMap<String, f64>,
    /// Per-objective: standard deviation of the cell values across
    /// profiles. A high stdev signals the posture is sensitive to which
    /// attacker profile it faces; a low stdev signals it scales evenly.
    pub stdev_per_objective: BTreeMap<String, f64>,
}

/// (`profile_name`, `value`) tuple. Avoids a generic `BTreeMap` where
/// the key carries semantic meaning.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamedValue {
    pub profile_name: String,
    pub value: f64,
}

/// Aggregate result of a robustness run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobustnessResult {
    /// Echo of the objectives evaluated, in the order supplied.
    pub objectives: Vec<SearchObjective>,
    /// Profiles evaluated, in declaration order. The synthetic
    /// `baseline_attacker` profile (when no `[attacker_profiles]` are
    /// declared) is included here so the cell list is fully self-
    /// describing.
    pub profiles: Vec<RobustnessProfileSummary>,
    /// Postures evaluated, in input order. The baseline posture (when
    /// `include_baseline = true`) is always at index 0.
    pub postures: Vec<DefenderPosture>,
    /// Per-(posture × profile) cells, in `(posture_index, profile_index)`
    /// row-major order so consumers can index without re-scanning.
    pub cells: Vec<RobustnessCell>,
    /// Per-posture rollups, parallel to `postures`.
    pub rollups: Vec<PostureRollup>,
    /// `Some("baseline")` when `include_baseline = true` so the report
    /// renderer knows which posture to anchor deltas on; `None`
    /// otherwise.
    pub baseline_label: Option<String>,
}

/// Profile metadata included in the result so the JSON consumer
/// doesn't have to re-read the scenario to label cells.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RobustnessProfileSummary {
    pub name: String,
    pub description: String,
    pub assignment_count: u32,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a robustness analysis on the supplied scenario.
///
/// Evaluates every (posture, profile) cell via an independent Monte
/// Carlo batch and produces per-cell objective values plus per-posture
/// rollups (worst / best / mean / stdev across profiles).
///
/// # Errors
///
/// - `InvalidConfig` if `objectives` is empty, both `postures` is empty
///   and `include_baseline` is `false` (nothing to evaluate), or any
///   posture / profile assignment path fails to resolve via `set_param`
///   on the scenario.
/// - `Engine` if any inner Monte Carlo evaluation fails.
pub fn run_robustness(
    scenario: &Scenario,
    config: &RobustnessConfig,
) -> Result<RobustnessResult, StatsError> {
    if config.objectives.is_empty() {
        return Err(StatsError::InvalidConfig(
            "robustness requires at least one objective; \
             pass --robustness-objective on the CLI"
                .into(),
        ));
    }
    if config.postures.is_empty() && !config.include_baseline {
        return Err(StatsError::InvalidConfig(
            "robustness requires at least one posture or include_baseline=true; \
             with neither, there is nothing to evaluate"
                .into(),
        ));
    }

    // Validate posture paths resolve. Same probe pattern as `run_search`.
    // Also rejects within-posture duplicate paths so a hand-constructed
    // posture with `[(p, 0.1), (p, 0.5)]` doesn't silently apply only
    // the last value — symmetric with the engine-side check on
    // `AttackerProfile.assignments`.
    let mut probe = scenario.clone();
    for (i, posture) in config.postures.iter().enumerate() {
        let mut seen_paths: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for ov in &posture.assignments {
            if !seen_paths.insert(ov.path.as_str()) {
                return Err(StatsError::InvalidConfig(format!(
                    "posture #{i} (`{}`) assigns to path `{}` more than once; \
                     the second value would silently override the first",
                    posture.label, ov.path
                )));
            }
            // Read-then-write to confirm the path resolves; any failure
            // is a config error not a runtime one, so we surface it
            // before doing any expensive MC work.
            let current = crate::sensitivity::get_param(&probe, &ov.path).map_err(|e| {
                StatsError::InvalidConfig(format!(
                    "posture #{i} (`{}`) assignment to `{}` failed to resolve: {e}",
                    posture.label, ov.path
                ))
            })?;
            set_param(&mut probe, &ov.path, current).map_err(|e| {
                StatsError::InvalidConfig(format!(
                    "posture #{i} (`{}`) assignment to `{}` failed round-trip: {e}",
                    posture.label, ov.path
                ))
            })?;
        }
    }

    // Validate profile paths resolve. Profiles live on the scenario, so
    // duplicating the engine's structural validation is unnecessary —
    // the engine has already rejected empty paths, NaN values, and
    // duplicate names by the time the runner is called. Only thing the
    // engine couldn't check is whether `set_param` accepts the path.
    let profiles: Vec<&AttackerProfile> =
        scenario.strategy_space.attacker_profiles.iter().collect();
    for profile in &profiles {
        for a in &profile.assignments {
            let current = crate::sensitivity::get_param(&probe, &a.path).map_err(|e| {
                StatsError::InvalidConfig(format!(
                    "attacker profile `{}` assignment to `{}` failed to resolve: {e}",
                    profile.name, a.path
                ))
            })?;
            set_param(&mut probe, &a.path, current).map_err(|e| {
                StatsError::InvalidConfig(format!(
                    "attacker profile `{}` assignment to `{}` failed round-trip: {e}",
                    profile.name, a.path
                ))
            })?;
        }
    }

    info!(
        postures = config.postures.len(),
        profiles = profiles.len(),
        objectives = config.objectives.len(),
        include_baseline = config.include_baseline,
        "starting robustness analysis"
    );

    // Build the posture list (baseline-prepended when requested).
    let mut all_postures: Vec<DefenderPosture> = Vec::new();
    let baseline_label = if config.include_baseline {
        all_postures.push(DefenderPosture {
            label: "baseline".to_string(),
            assignments: Vec::new(),
        });
        Some("baseline".to_string())
    } else {
        None
    };
    for p in &config.postures {
        all_postures.push(p.clone());
    }

    // If no profiles are declared, synthesise a `baseline_attacker`
    // singleton so the cells matrix still has shape (postures × 1) and
    // the rollups report something sensible. Useful as a sanity check.
    let synthetic_baseline_profile;
    let effective_profiles: Vec<&AttackerProfile> = if profiles.is_empty() {
        synthetic_baseline_profile = AttackerProfile {
            name: "baseline_attacker".to_string(),
            description: "Scenario default attacker (no profile applied)".to_string(),
            faction: None,
            assignments: Vec::new(),
        };
        vec![&synthetic_baseline_profile]
    } else {
        profiles
    };

    // Evaluate every cell. The two-loop order is (posture, profile) so
    // a partial failure surfaces with a deterministic cell index in the
    // error context.
    let mut cells = Vec::with_capacity(all_postures.len() * effective_profiles.len());
    for posture in &all_postures {
        for profile in &effective_profiles {
            debug!(
                posture = %posture.label,
                profile = %profile.name,
                "evaluating robustness cell"
            );

            let mut variant = scenario.clone();
            for ov in &posture.assignments {
                set_param(&mut variant, &ov.path, ov.value).map_err(|e| {
                    StatsError::InvalidConfig(format!(
                        "robustness cell ({}, {}): posture assignment failed: {e}",
                        posture.label, profile.name,
                    ))
                })?;
            }
            for a in &profile.assignments {
                set_param(&mut variant, &a.path, a.value).map_err(|e| {
                    StatsError::InvalidConfig(format!(
                        "robustness cell ({}, {}): profile assignment failed: {e}",
                        posture.label, profile.name,
                    ))
                })?;
            }

            let mc = MonteCarloRunner::run(&config.mc_config, &variant)?;
            let mut objective_values = BTreeMap::new();
            for objective in &config.objectives {
                let v = crate::search::evaluate_objective_public(objective, &mc.summary);
                objective_values.insert(objective.label(), v);
            }
            cells.push(RobustnessCell {
                posture_label: posture.label.clone(),
                profile_name: profile.name.clone(),
                objective_values,
                summary: mc.summary,
            });
        }
    }

    let rollups = compute_rollups(
        &all_postures,
        &effective_profiles,
        &cells,
        &config.objectives,
    );

    let profile_summaries: Vec<RobustnessProfileSummary> = effective_profiles
        .iter()
        .map(|p| RobustnessProfileSummary {
            name: p.name.clone(),
            description: p.description.clone(),
            assignment_count: u32::try_from(p.assignments.len())
                .expect("profile assignment count fits u32"),
        })
        .collect();

    info!(
        cells = cells.len(),
        rollups = rollups.len(),
        "robustness analysis complete"
    );

    Ok(RobustnessResult {
        objectives: config.objectives.clone(),
        profiles: profile_summaries,
        postures: all_postures,
        cells,
        rollups,
        baseline_label,
    })
}

// ---------------------------------------------------------------------------
// Rollups
// ---------------------------------------------------------------------------

fn compute_rollups(
    postures: &[DefenderPosture],
    profiles: &[&AttackerProfile],
    cells: &[RobustnessCell],
    objectives: &[SearchObjective],
) -> Vec<PostureRollup> {
    let n_profiles = profiles.len();
    if n_profiles == 0 {
        return postures
            .iter()
            .map(|p| PostureRollup {
                posture_label: p.label.clone(),
                worst_per_objective: BTreeMap::new(),
                best_per_objective: BTreeMap::new(),
                mean_per_objective: BTreeMap::new(),
                stdev_per_objective: BTreeMap::new(),
            })
            .collect();
    }
    let mut out = Vec::with_capacity(postures.len());
    for (pi, posture) in postures.iter().enumerate() {
        let row_start = pi * n_profiles;
        let row = &cells[row_start..row_start + n_profiles];

        let mut worst_per_objective = BTreeMap::new();
        let mut best_per_objective = BTreeMap::new();
        let mut mean_per_objective = BTreeMap::new();
        let mut stdev_per_objective = BTreeMap::new();
        for objective in objectives {
            let label = objective.label();
            let values: Vec<(String, f64)> = row
                .iter()
                .map(|c| {
                    (
                        c.profile_name.clone(),
                        c.objective_values.get(&label).copied().unwrap_or(f64::NAN),
                    )
                })
                .collect();

            // Direction-aware worst/best. For a maximize-direction
            // objective (e.g. MaximizeWinRate), "worst" is the
            // smallest value (defender wins least often). For a
            // minimize-direction objective (e.g. MinimizeMaxChainSuccess),
            // "worst" is the largest value (chain succeeds most).
            // `pick_extreme` returns the value that sorts greatest
            // under its comparator: `a.total_cmp(b)` finds the max,
            // `b.total_cmp(a)` finds the min. NaN propagates via
            // total_cmp so a misconfigured objective surfaces as a
            // NaN pick rather than silent suppression.
            let maximize = objective.maximize();
            let (worst_name, worst_val) = if maximize {
                // maximize=true → worst is min (use reverse cmp)
                pick_extreme(&values, |a, b| b.total_cmp(a))
            } else {
                // maximize=false → worst is max (use normal cmp)
                pick_extreme(&values, |a, b| a.total_cmp(b))
            };
            let (best_name, best_val) = if maximize {
                // maximize=true → best is max
                pick_extreme(&values, |a, b| a.total_cmp(b))
            } else {
                // maximize=false → best is min
                pick_extreme(&values, |a, b| b.total_cmp(a))
            };
            worst_per_objective.insert(
                label.clone(),
                NamedValue {
                    profile_name: worst_name,
                    value: worst_val,
                },
            );
            best_per_objective.insert(
                label.clone(),
                NamedValue {
                    profile_name: best_name,
                    value: best_val,
                },
            );

            // Mean / stdev across profiles. Population stdev (`n` in
            // the denominator) — the profile set is the whole population
            // the analyst is asking about, not a sample of some larger
            // distribution. This matches how a feasibility table reads
            // "across the profiles the analyst declared, the variance
            // is X" rather than "estimate of an underlying distribution."
            let nums: Vec<f64> = values.iter().map(|(_, v)| *v).collect();
            let n = nums.len() as f64;
            let mean = nums.iter().copied().sum::<f64>() / n;
            let var = nums.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
            let stdev = var.sqrt();
            mean_per_objective.insert(label.clone(), mean);
            stdev_per_objective.insert(label, stdev);
        }
        out.push(PostureRollup {
            posture_label: posture.label.clone(),
            worst_per_objective,
            best_per_objective,
            mean_per_objective,
            stdev_per_objective,
        });
    }
    out
}

/// Pick the (name, value) pair sorting last under `cmp_values` —
/// i.e. the profile maximizing the comparator's "greater than"
/// relation. Ties resolve by lowest declaration index since the
/// `values` vector preserves the input order. NaN handling is
/// total_cmp's: NaN sorts highest, so a misconfigured objective that
/// returned NaN for one cell will surface as that cell winning the
/// "worst" bucket (visible failure rather than silent suppression).
fn pick_extreme<F>(values: &[(String, f64)], cmp_values: F) -> (String, f64)
where
    F: Fn(&f64, &f64) -> std::cmp::Ordering,
{
    // `values` is non-empty here — caller iterates over a non-empty
    // profile slice. The fold's seed is the first entry.
    values
        .iter()
        .skip(1)
        .fold(values[0].clone(), |acc, current| {
            match cmp_values(&current.1, &acc.1) {
                std::cmp::Ordering::Greater => current.clone(),
                _ => acc,
            }
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_extreme_returns_max_under_total_cmp() {
        let v = vec![
            ("a".to_string(), 1.0),
            ("b".to_string(), 3.0),
            ("c".to_string(), 2.0),
        ];
        let (name, val) = pick_extreme(&v, |a, b| a.total_cmp(b));
        assert_eq!(name, "b");
        assert!((val - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pick_extreme_returns_min_under_reversed_cmp() {
        let v = vec![
            ("a".to_string(), 1.0),
            ("b".to_string(), 3.0),
            ("c".to_string(), 2.0),
        ];
        let (name, val) = pick_extreme(&v, |a, b| b.total_cmp(a));
        assert_eq!(name, "a");
        assert!((val - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pick_extreme_breaks_ties_by_first_occurrence() {
        // Two profiles tie at the extreme; the earlier one (declaration
        // order) must win so output is reproducible across runs.
        let v = vec![
            ("first".to_string(), 5.0),
            ("middle".to_string(), 3.0),
            ("second".to_string(), 5.0),
        ];
        let (name, _) = pick_extreme(&v, |a, b| a.total_cmp(b));
        assert_eq!(
            name, "first",
            "earlier-declared profile must win an extreme tie"
        );
    }

    #[test]
    fn config_rejects_empty_objectives() {
        let scenario = build_minimal_scenario();
        let config = RobustnessConfig {
            postures: Vec::new(),
            include_baseline: true,
            mc_config: MonteCarloConfig {
                num_runs: 1,
                seed: Some(0),
                collect_snapshots: false,
                parallel: false,
            },
            objectives: Vec::new(),
        };
        let err = run_robustness(&scenario, &config).expect_err("empty objectives must error");
        assert!(format!("{err}").contains("at least one objective"));
    }

    #[test]
    fn config_rejects_no_postures_no_baseline() {
        let scenario = build_minimal_scenario();
        let config = RobustnessConfig {
            postures: Vec::new(),
            include_baseline: false,
            mc_config: MonteCarloConfig {
                num_runs: 1,
                seed: Some(0),
                collect_snapshots: false,
                parallel: false,
            },
            objectives: vec![SearchObjective::MinimizeDuration],
        };
        let err =
            run_robustness(&scenario, &config).expect_err("no postures + no baseline must error");
        assert!(format!("{err}").contains("nothing to evaluate"));
    }

    fn build_minimal_scenario() -> Scenario {
        // Reuse the test helpers from the lib module via an indirect
        // route: re-create a tiny scenario in-line. The fuller
        // integration tests live in `tests/epic_i_robustness.rs`.
        use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
        use faultline_types::ids::{FactionId, ForceId, RegionId, VictoryId};
        use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
        use faultline_types::politics::{MediaLandscape, PoliticalClimate};
        use faultline_types::scenario::ScenarioMeta;
        use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
        use faultline_types::strategy::Doctrine;
        use faultline_types::victory::{VictoryCondition, VictoryType};

        let r = RegionId::from("r1");
        let f = FactionId::from("blue");

        let mut regions = BTreeMap::new();
        regions.insert(
            r.clone(),
            Region {
                id: r.clone(),
                name: "R1".into(),
                population: 100_000,
                urbanization: 0.5,
                initial_control: Some(f.clone()),
                strategic_value: 1.0,
                borders: vec![],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        let force_id = ForceId::from("blue-inf");
        let mut forces = BTreeMap::new();
        forces.insert(
            force_id.clone(),
            ForceUnit {
                id: force_id,
                name: "Blue Inf".into(),
                unit_type: UnitType::Infantry,
                region: r.clone(),
                strength: 100.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );
        factions.insert(
            f.clone(),
            Faction {
                id: f.clone(),
                name: "Blue".into(),
                description: "test".into(),
                color: "#000".into(),
                faction_type: FactionType::Insurgent,
                forces,
                tech_access: vec![],
                initial_morale: 0.5,
                logistics_capacity: 1.0,
                initial_resources: 100.0,
                resource_rate: 1.0,
                recruitment: None,
                command_resilience: 0.5,
                intelligence: 0.5,
                diplomacy: vec![],
                doctrine: Doctrine::Conventional,
                escalation_rules: None,
                defender_capacities: BTreeMap::new(),
                leadership: None,
            },
        );

        let mut victory_conditions = BTreeMap::new();
        victory_conditions.insert(
            VictoryId::from("vc"),
            VictoryCondition {
                id: VictoryId::from("vc"),
                name: "Win".into(),
                faction: f.clone(),
                condition: VictoryType::MilitaryDominance {
                    enemy_strength_below: 0.01,
                },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "tiny".into(),
                description: "tiny".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![TerrainModifier {
                    region: r,
                    terrain_type: TerrainType::Urban,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                }],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.5,
                institutional_trust: 0.5,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.5,
                    internet_availability: 0.5,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 5,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(42),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: faultline_types::strategy_space::StrategySpace::default(),
            networks: std::collections::BTreeMap::new(),
        }
    }
}
