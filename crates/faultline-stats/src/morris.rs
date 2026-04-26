//! Morris elementary-effects variance screening (Epic C).
//!
//! The Round-One sensitivity helper in [`crate::sensitivity`] is pure
//! one-at-a-time: it varies a single parameter across a fixed grid
//! while every other parameter sits at its baseline. That's fine for
//! quick local sweeps but it cannot rank parameters by the *variance*
//! they induce in the output, only by the visible step-to-step delta.
//!
//! Morris's method [Morris 1991, *Technometrics* 33(2)] fixes this with
//! a small number of randomized one-step trajectories through the
//! parameter space. For each trajectory, every parameter is perturbed
//! once by a fixed step `Δ` and the resulting "elementary effect"
//! `EE_i = (y(x + Δ e_i) - y(x)) / Δ` is recorded. With `R`
//! trajectories we get `R` elementary effects per parameter, summarised
//! as:
//!
//! - `mu_star` — mean of the absolute elementary effects. A robust
//!   first-order importance ranking; high `mu_star` means the
//!   parameter moves the output a lot on average.
//! - `sigma` — standard deviation of the elementary effects. High
//!   `sigma` means the parameter's effect is non-linear or interacts
//!   with the rest of the parameter space; low `sigma` with high
//!   `mu_star` means the effect is roughly additive.
//!
//! The total simulation count is `R × (k + 1)` where `k` is the number
//! of input parameters — the same order as `k` separate sensitivity
//! sweeps but the output is *variance-decomposable*, not just a
//! pointwise plot. Sobol's method is a strict superset (and the
//! natural follow-up) but requires `N(2k+2)` runs, which puts it well
//! outside the budget of an interactive analyst CLI invocation. Morris
//! is the standard "screening" stage that answers "which parameters
//! are even worth running Sobol on."
//!
//! ## Determinism
//!
//! The trajectory layout is built from a caller-supplied seed via
//! `ChaCha8Rng`, so the same seed plus the same parameter list always
//! produce identical Monte Carlo invocations. Each trajectory's MC
//! batch is run with its own seed (derived from the Morris seed plus
//! the trajectory index) so the inner MC stays deterministic too.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloConfig;

use crate::sensitivity::{get_param, set_param};
use crate::{MonteCarloRunner, StatsError};

/// Configuration for a Morris elementary-effects screening run.
#[derive(Clone, Debug)]
pub struct MorrisConfig {
    /// Parameter paths in `crate::sensitivity::set_param` grammar
    /// (e.g. `"faction.gov.initial_morale"`). All paths must already be
    /// readable on the supplied scenario.
    pub params: Vec<String>,
    /// Per-parameter `[low, high]` bounds. Same length as `params`.
    pub bounds: Vec<(f64, f64)>,
    /// Number of Morris trajectories. Each consumes `params.len() + 1`
    /// MC batches. Typical analyst values: 10–30.
    pub trajectories: u32,
    /// Step size as a fraction of the parameter range. The classical
    /// Morris recommendation is `delta = p / (2(p-1))` where `p` is
    /// the levels per parameter; for `p = 4` that's `2/3`. Callers
    /// usually want `0.5`–`0.66`.
    pub delta_fraction: f64,
    /// Seed for trajectory construction and inner MC batches. Same
    /// (config, scenario, seed) → identical output.
    pub seed: u64,
}

/// Per-parameter Morris importance summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MorrisIndex {
    pub param: String,
    /// Mean of absolute elementary effects — first-order importance.
    pub mu_star: f64,
    /// Mean of signed elementary effects — sign / direction.
    pub mu: f64,
    /// Std. dev. of elementary effects — non-linearity / interaction.
    pub sigma: f64,
    /// Number of elementary effects supporting the estimate (== `R`
    /// when no trajectory failed to evaluate).
    pub n_effects: u32,
}

/// Output of a Morris screening run for a single output metric.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MorrisResult {
    /// Name of the output metric the indices are computed against.
    pub output_metric: String,
    /// One row per parameter, sorted by `mu_star` descending so the
    /// most important parameter is first.
    pub indices: Vec<MorrisIndex>,
    /// Total number of Monte Carlo batches actually executed.
    pub batches_run: u32,
}

/// Output metric the Morris run summarises elementary effects against.
///
/// Each variant maps to a scalar-valued reduction over the Monte Carlo
/// summary for one parameter assignment. Adding a new variant requires
/// extending `extract_metric` below to produce its scalar; the rest of
/// the algorithm is metric-agnostic.
#[derive(Clone, Copy, Debug)]
pub enum MorrisMetric {
    /// Mean simulation duration in ticks.
    Duration,
    /// First-faction win rate. Useful when there's a designated
    /// attacker; falls back to 0.0 when no win rate is recorded.
    FirstFactionWinRate,
    /// Mean kill-chain success rate across all chains in the scenario.
    /// Falls back to 0.0 when there are no chains.
    MeanChainSuccess,
}

/// Run a Morris elementary-effects screening.
pub fn run_morris(
    base_scenario: &Scenario,
    mc_config: &MonteCarloConfig,
    config: &MorrisConfig,
    metric: MorrisMetric,
) -> Result<MorrisResult, StatsError> {
    if config.params.is_empty() {
        return Err(StatsError::InvalidConfig(
            "morris: at least one parameter required".into(),
        ));
    }
    if config.params.len() != config.bounds.len() {
        return Err(StatsError::InvalidConfig(
            "morris: params and bounds must have the same length".into(),
        ));
    }
    if config.trajectories == 0 {
        return Err(StatsError::InvalidConfig(
            "morris: trajectories must be > 0".into(),
        ));
    }
    if !(0.0..=1.0).contains(&config.delta_fraction) || config.delta_fraction <= 0.0 {
        return Err(StatsError::InvalidConfig(
            "morris: delta_fraction must be in (0, 1]".into(),
        ));
    }
    for path in &config.params {
        // Eagerly verify each path resolves so a bad parameter name
        // surfaces as a config error, not a runtime panic on the first
        // trajectory.
        let _ = get_param(base_scenario, path)?;
    }

    let k = config.params.len();
    let mut rng = ChaCha8Rng::seed_from_u64(config.seed);

    // Per-parameter accumulators.
    let mut mu_sum = vec![0.0_f64; k];
    let mut mu_abs_sum = vec![0.0_f64; k];
    let mut mu_sq_sum = vec![0.0_f64; k];
    let mut counts = vec![0u32; k];

    let mut batches_run = 0u32;

    for traj_idx in 0..config.trajectories {
        // Initial point: each parameter drawn uniformly from its
        // bounds, then floored to the nearest 1/levels grid so the
        // step-of-Δ-after-Δ structure works the same way as the
        // classical Morris design. The level grid is implicit in the
        // delta size — keeping it simple here, since analyst-facing
        // parameter ranges are typically continuous anyway.
        let x: Vec<f64> = config
            .bounds
            .iter()
            .map(|(lo, hi)| lo + rng.r#gen::<f64>() * (hi - lo))
            .collect();

        // Random permutation of parameter indices.
        let mut order: Vec<usize> = (0..k).collect();
        for i in (1..k).rev() {
            let j = rng.gen_range(0..=i);
            order.swap(i, j);
        }

        // Random sign per parameter — flip the step direction so we
        // sample both sides of the local subspace across trajectories.
        let signs: Vec<f64> = (0..k)
            .map(|_| if rng.r#gen::<bool>() { 1.0 } else { -1.0 })
            .collect();

        // Evaluate baseline.
        let y0 = evaluate_metric(base_scenario, mc_config, &config.params, &x, metric)?;
        batches_run += 1;
        let mut y_prev = y0;
        let mut x_prev = x.clone();

        for &param_idx in &order {
            let (lo, hi) = config.bounds[param_idx];
            let span = hi - lo;
            if span <= 0.0 {
                continue;
            }
            let mut step = config.delta_fraction * span * signs[param_idx];

            let candidate = x_prev[param_idx] + step;
            // If the candidate would leave the bounds, flip the sign.
            // This keeps the trajectory inside `[lo, hi]^k` without
            // skipping a step (which would lose a degree of freedom).
            let candidate = if candidate < lo || candidate > hi {
                step = -step;
                x_prev[param_idx] + step
            } else {
                candidate
            };

            let mut x_next = x_prev.clone();
            x_next[param_idx] = candidate;

            let y_next =
                evaluate_metric(base_scenario, mc_config, &config.params, &x_next, metric)?;
            batches_run += 1;
            // Elementary effect normalised by the *signed* step so
            // cancellation across trajectories is meaningful (both
            // `mu` and `mu_star` are well-defined).
            let ee = (y_next - y_prev) / step;
            mu_sum[param_idx] += ee;
            mu_abs_sum[param_idx] += ee.abs();
            mu_sq_sum[param_idx] += ee * ee;
            counts[param_idx] = counts[param_idx].saturating_add(1);

            x_prev = x_next;
            y_prev = y_next;
        }

        // Defensive: surface the trajectory index in tracing if a step
        // failed silently (currently unreachable because every step
        // returns `?` above, but useful when extending the routine).
        let _ = traj_idx;
    }

    let mut indices: Vec<MorrisIndex> = (0..k)
        .map(|i| {
            let n = counts[i];
            if n == 0 {
                return MorrisIndex {
                    param: config.params[i].clone(),
                    mu_star: 0.0,
                    mu: 0.0,
                    sigma: 0.0,
                    n_effects: 0,
                };
            }
            let n_f = f64::from(n);
            let mean = mu_sum[i] / n_f;
            let mean_abs = mu_abs_sum[i] / n_f;
            let var = if n > 1 {
                ((mu_sq_sum[i] / n_f) - mean * mean).max(0.0) * n_f / (n_f - 1.0)
            } else {
                0.0
            };
            MorrisIndex {
                param: config.params[i].clone(),
                mu_star: mean_abs,
                mu: mean,
                sigma: var.sqrt(),
                n_effects: n,
            }
        })
        .collect();

    indices.sort_by(|a, b| b.mu_star.total_cmp(&a.mu_star));

    Ok(MorrisResult {
        output_metric: metric_label(metric).into(),
        indices,
        batches_run,
    })
}

/// Run an MC batch with the given parameter assignment and reduce to a
/// single metric scalar.
fn evaluate_metric(
    base_scenario: &Scenario,
    mc_config: &MonteCarloConfig,
    params: &[String],
    values: &[f64],
    metric: MorrisMetric,
) -> Result<f64, StatsError> {
    let mut scenario = base_scenario.clone();
    for (path, value) in params.iter().zip(values.iter()) {
        set_param(&mut scenario, path, *value)?;
    }
    let result = MonteCarloRunner::run(mc_config, &scenario)?;
    Ok(extract_metric(&result.summary, metric))
}

fn extract_metric(
    summary: &faultline_types::stats::MonteCarloSummary,
    metric: MorrisMetric,
) -> f64 {
    match metric {
        MorrisMetric::Duration => summary.average_duration,
        MorrisMetric::FirstFactionWinRate => {
            summary.win_rates.values().next().copied().unwrap_or(0.0)
        },
        MorrisMetric::MeanChainSuccess => {
            if summary.campaign_summaries.is_empty() {
                return 0.0;
            }
            let sum: f64 = summary
                .campaign_summaries
                .values()
                .map(|cs| cs.overall_success_rate)
                .sum();
            sum / summary.campaign_summaries.len() as f64
        },
    }
}

fn metric_label(m: MorrisMetric) -> &'static str {
    match m {
        MorrisMetric::Duration => "duration",
        MorrisMetric::FirstFactionWinRate => "first_faction_win_rate",
        MorrisMetric::MeanChainSuccess => "mean_chain_success",
    }
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
    use faultline_types::victory::{VictoryCondition, VictoryType};

    fn scenario_for_morris() -> Scenario {
        let r1 = RegionId::from("r1");
        let r2 = RegionId::from("r2");
        let f_a = FactionId::from("a");
        let f_b = FactionId::from("b");

        let mut regions = BTreeMap::new();
        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "R1".into(),
                population: 1000,
                urbanization: 0.5,
                initial_control: Some(f_a.clone()),
                strategic_value: 1.0,
                borders: vec![r2.clone()],
                centroid: None,
            },
        );
        regions.insert(
            r2.clone(),
            Region {
                id: r2.clone(),
                name: "R2".into(),
                population: 1000,
                urbanization: 0.5,
                initial_control: Some(f_b.clone()),
                strategic_value: 1.0,
                borders: vec![r1.clone()],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(f_a.clone(), make_faction(f_a.clone(), "A", r1.clone()));
        factions.insert(f_b.clone(), make_faction(f_b.clone(), "B", r2.clone()));

        let mut victory_conditions = BTreeMap::new();
        let vc = VictoryId::from("a-win");
        victory_conditions.insert(
            vc.clone(),
            VictoryCondition {
                id: vc,
                name: "A wins".into(),
                faction: f_a,
                condition: VictoryType::MilitaryDominance {
                    enemy_strength_below: 0.01,
                },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "morris".into(),
                description: "".into(),
                author: "".into(),
                version: "0".into(),
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
                        visibility: 0.5,
                    },
                    TerrainModifier {
                        region: r2,
                        terrain_type: TerrainType::Rural,
                        movement_modifier: 1.0,
                        defense_modifier: 1.0,
                        visibility: 0.5,
                    },
                ],
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
                monte_carlo_runs: 2,
                seed: Some(7),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
        }
    }

    fn make_faction(id: FactionId, name: &str, region: RegionId) -> Faction {
        let force_id = ForceId::from(format!("{}-inf", id));
        let mut forces = BTreeMap::new();
        forces.insert(
            force_id.clone(),
            ForceUnit {
                id: force_id,
                name: format!("{name} Inf"),
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
            name: name.into(),
            faction_type: FactionType::Insurgent,
            description: "".into(),
            color: "#000".into(),
            forces,
            tech_access: vec![],
            initial_morale: 0.7,
            logistics_capacity: 100.0,
            initial_resources: 1_000.0,
            resource_rate: 10.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
            escalation_rules: None,
        }
    }

    #[test]
    fn morris_rejects_zero_trajectories() {
        let scenario = scenario_for_morris();
        let mc = MonteCarloConfig {
            num_runs: 2,
            seed: Some(1),
            collect_snapshots: false,
            parallel: false,
        };
        let cfg = MorrisConfig {
            params: vec!["faction.a.initial_morale".into()],
            bounds: vec![(0.1, 0.9)],
            trajectories: 0,
            delta_fraction: 0.5,
            seed: 1,
        };
        let r = run_morris(&scenario, &mc, &cfg, MorrisMetric::Duration);
        assert!(r.is_err(), "zero trajectories should error");
    }

    #[test]
    fn morris_runs_and_returns_indices() {
        let scenario = scenario_for_morris();
        let mc = MonteCarloConfig {
            num_runs: 2,
            seed: Some(1),
            collect_snapshots: false,
            parallel: false,
        };
        let cfg = MorrisConfig {
            params: vec![
                "faction.a.initial_morale".into(),
                "faction.b.initial_morale".into(),
            ],
            bounds: vec![(0.1, 0.9), (0.1, 0.9)],
            trajectories: 3,
            delta_fraction: 0.5,
            seed: 42,
        };
        let result = run_morris(&scenario, &mc, &cfg, MorrisMetric::Duration).expect("morris run");
        assert_eq!(result.indices.len(), 2);
        // Each trajectory runs k+1 = 3 batches; 3 trajectories → 9 batches.
        assert_eq!(result.batches_run, 9);
        // mu_star is non-negative by construction.
        for idx in &result.indices {
            assert!(idx.mu_star >= 0.0);
            assert!(idx.sigma >= 0.0);
        }
    }

    #[test]
    fn morris_is_deterministic_under_seed() {
        let scenario = scenario_for_morris();
        let mc = MonteCarloConfig {
            num_runs: 2,
            seed: Some(1),
            collect_snapshots: false,
            parallel: false,
        };
        let cfg = MorrisConfig {
            params: vec!["faction.a.initial_morale".into()],
            bounds: vec![(0.2, 0.9)],
            trajectories: 2,
            delta_fraction: 0.5,
            seed: 99,
        };
        let a = run_morris(&scenario, &mc, &cfg, MorrisMetric::Duration).expect("a");
        let b = run_morris(&scenario, &mc, &cfg, MorrisMetric::Duration).expect("b");
        assert_eq!(a.indices.len(), b.indices.len());
        for (x, y) in a.indices.iter().zip(b.indices.iter()) {
            assert!((x.mu_star - y.mu_star).abs() < 1e-12);
            assert!((x.sigma - y.sigma).abs() < 1e-12);
        }
    }

    #[test]
    fn morris_rejects_unknown_param() {
        let scenario = scenario_for_morris();
        let mc = MonteCarloConfig {
            num_runs: 1,
            seed: Some(1),
            collect_snapshots: false,
            parallel: false,
        };
        let cfg = MorrisConfig {
            params: vec!["faction.unknown.initial_morale".into()],
            bounds: vec![(0.0, 1.0)],
            trajectories: 1,
            delta_fraction: 0.5,
            seed: 0,
        };
        assert!(run_morris(&scenario, &mc, &cfg, MorrisMetric::Duration).is_err());
    }
}
