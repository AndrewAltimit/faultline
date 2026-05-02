//! Property tests for `faultline_stats::search`.
//!
//! Two invariants worth pinning per the May 2026 refresh:
//!
//! 1. **Same `search_seed` ⇒ bit-identical search result.** Already
//!    pinned by `search_then_evaluate_is_deterministic` for one fixed
//!    seed; the property version randomizes the seed so a refactor
//!    that introduced HashMap-style nondeterminism (e.g. picking up a
//!    `std::collections::HashMap` somewhere in the trial pipeline)
//!    would surface here under random seeds even if it slipped past
//!    the fixed-seed test. Manifest replay (`--verify`) depends on
//!    this contract.
//!
//! 2. **Every trial's assignments stay within declared bounds.** The
//!    public report would silently surface out-of-range parameter
//!    values if `sample_random_value` or `enumerate_levels` regressed
//!    (e.g. inclusive vs. exclusive bound errors at the high end).
//!
//! The property suite uses a tiny scenario built from scratch (not the
//! bundled `strategy_search_demo` whose 50 MC runs × 80 ticks per
//! trial would slow proptest's 256-case budget to a crawl). The
//! engine path is still exercised — `run_search` always runs the
//! engine — but only with `num_runs = 2` and `max_ticks = 30`. The
//! proptest `Config { cases: 24, .. }` keeps total wall time under a
//! couple of seconds.

use std::collections::BTreeMap;

use faultline_stats::search::{SearchConfig, SearchMethod, run_search};
use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
use faultline_types::ids::{FactionId, ForceId, RegionId, VictoryId};
use faultline_types::map::{
    EnvironmentSchedule, MapConfig, MapSource, Region, TerrainModifier, TerrainType,
};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::stats::MonteCarloConfig;
use faultline_types::strategy::Doctrine;
use faultline_types::strategy_space::{DecisionVariable, Domain, SearchObjective, StrategySpace};
use faultline_types::victory::{VictoryCondition, VictoryType};
use proptest::prelude::*;

/// Steps for the lone continuous decision variable in the property-test
/// fixture. Centralized here so `minimal_search_scenario` and the grid
/// assertion in `grid_trial_assignments_match_enumerated_levels` cannot
/// drift apart — if this changes, both sites update together.
const FIXTURE_STEPS: u32 = 4;

/// Minimal scenario with two factions and a single decision variable
/// over `faction.alpha.initial_morale`. Tuned for property-test speed:
/// 30 ticks max, two-region grid, no kill chains or networks. Exposed
/// here (not reused from `search.rs`'s private fixture) so this test
/// file stays self-contained — depending on `pub(crate)` test fixtures
/// would silently couple to internal layout.
fn minimal_search_scenario(low: f64, high: f64) -> Scenario {
    let f_alpha = FactionId::from("alpha");
    let f_bravo = FactionId::from("bravo");
    let r1 = RegionId::from("region-a");
    let r2 = RegionId::from("region-b");

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

    let make_faction = |id: FactionId, name: &str, region: RegionId| -> Faction {
        let force_id = ForceId::from(format!("{}-inf", id).as_str());
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
                move_progress: 0.0,
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
            alliance_fracture: None,
        }
    };

    let mut factions = BTreeMap::new();
    factions.insert(
        f_alpha.clone(),
        make_faction(f_alpha.clone(), "Alpha", r1.clone()),
    );
    factions.insert(f_bravo.clone(), make_faction(f_bravo, "Bravo", r2.clone()));

    let mut victory_conditions = BTreeMap::new();
    let vc_id = VictoryId::from("alpha-win");
    victory_conditions.insert(
        vc_id.clone(),
        VictoryCondition {
            id: vc_id,
            name: "Alpha Dominance".into(),
            faction: f_alpha.clone(),
            condition: VictoryType::MilitaryDominance {
                enemy_strength_below: 0.01,
            },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Property Search Scenario".into(),
            description: "Property-test fixture".into(),
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
            max_ticks: 30,
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
        environment: EnvironmentSchedule::default(),
        strategy_space: StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.alpha.initial_morale".into(),
                owner: Some(FactionId::from("alpha")),
                domain: Domain::Continuous {
                    low,
                    high,
                    steps: FIXTURE_STEPS,
                },
            }],
            objectives: vec![],
            attacker_profiles: Vec::new(),
        },
        networks: BTreeMap::new(),
    }
}

fn search_config(method: SearchMethod, search_seed: u64, trials: u32) -> SearchConfig {
    SearchConfig {
        trials,
        method,
        search_seed,
        mc_config: MonteCarloConfig {
            num_runs: 2,
            seed: Some(0xBEEF),
            collect_snapshots: false,
            parallel: false,
        },
        objectives: vec![SearchObjective::MaximizeWinRate {
            faction: FactionId::from("alpha"),
        }],
        compute_baseline: false,
    }
}

proptest! {
    // 24 cases × ~3 engine trials × 2 MC runs ≈ 144 short engine runs;
    // measured at ~1.5s wall time on a developer laptop.
    #![proptest_config(ProptestConfig {
        cases: 24,
        .. ProptestConfig::default()
    })]

    /// **Invariant: same `search_seed` ⇒ bit-identical SearchResult.**
    /// Manifest replay (`--verify`) and CI's `verify-bundled` /
    /// `verify-robustness` stages depend on this; a regression here
    /// would silently break replay.
    #[test]
    fn search_is_deterministic_under_fixed_seeds(
        search_seed in any::<u64>(),
        trials in 2u32..=4,
    ) {
        let scenario = minimal_search_scenario(0.3, 0.9);
        let cfg = search_config(SearchMethod::Random, search_seed, trials);
        let r1 = run_search(&scenario, &cfg).expect("first run");
        let r2 = run_search(&scenario, &cfg).expect("second run");
        let j1 = serde_json::to_string(&r1).expect("serialize r1");
        let j2 = serde_json::to_string(&r2).expect("serialize r2");
        prop_assert_eq!(j1, j2);
    }

    /// **Invariant: every trial's assignments stay within declared
    /// continuous bounds.** Random sampling uses `gen_range(low..high)`
    /// which is half-open; grid sampling enumerates inclusive endpoints.
    /// Both must keep every emitted value in `[low, high]`.
    #[test]
    fn random_trial_assignments_stay_within_continuous_bounds(
        search_seed in any::<u64>(),
        low_thousandths in 0u32..=400,
        span_thousandths in 100u32..=600,
    ) {
        let low = f64::from(low_thousandths) / 1_000.0;
        let high = low + f64::from(span_thousandths) / 1_000.0;
        let scenario = minimal_search_scenario(low, high);
        let cfg = search_config(SearchMethod::Random, search_seed, 3);
        let result = run_search(&scenario, &cfg).expect("search runs");
        for trial in &result.trials {
            for ov in &trial.assignments {
                prop_assert!(
                    ov.value >= low - 1e-12,
                    "assignment {} below low {}",
                    ov.value,
                    low
                );
                // Random mode is half-open: gen_range(low..high) draws
                // strictly below `high`. Allow strict <= high (with a
                // tiny tolerance to absorb fp drift) so we don't reject
                // legitimate edge samples.
                prop_assert!(
                    ov.value <= high + 1e-12,
                    "assignment {} above high {}",
                    ov.value,
                    high
                );
            }
        }
    }

    /// **Invariant: grid trial assignments hit only enumerated levels.**
    /// `enumerate_levels` produces `steps` evenly-spaced values
    /// inclusive of both endpoints. A regression that drifted off-grid
    /// would surface here under randomized bounds.
    #[test]
    fn grid_trial_assignments_match_enumerated_levels(
        search_seed in any::<u64>(),
        low_thousandths in 0u32..=400,
        span_thousandths in 100u32..=600,
    ) {
        let low = f64::from(low_thousandths) / 1_000.0;
        let high = low + f64::from(span_thousandths) / 1_000.0;
        let scenario = minimal_search_scenario(low, high);
        let cfg = search_config(SearchMethod::Grid, search_seed, FIXTURE_STEPS);
        let result = run_search(&scenario, &cfg).expect("search runs");
        // The fixture declares `steps: FIXTURE_STEPS`, so the
        // enumerated levels are at t in {i/(steps-1)} for
        // i in 0..steps inclusive of both endpoints. Build the same
        // set here and require every assignment to match one to
        // within fp drift.
        let denom = f64::from(FIXTURE_STEPS - 1);
        let levels: Vec<f64> = (0..FIXTURE_STEPS)
            .map(|i| {
                let t = f64::from(i) / denom;
                low + (high - low) * t
            })
            .collect();
        for trial in &result.trials {
            for ov in &trial.assignments {
                let on_grid = levels
                    .iter()
                    .any(|l| (l - ov.value).abs() < 1e-9);
                prop_assert!(
                    on_grid,
                    "grid assignment {} not on any of {:?}",
                    ov.value,
                    levels
                );
            }
        }
    }

    /// **Invariant: Pareto-frontier indices are valid and strictly
    /// ascending.** Sorted-ascending is the contract of the
    /// `pareto_indices` field; if it ever degenerates to unsorted or
    /// duplicate-laden output, downstream report rendering produces
    /// wrong tables.
    #[test]
    fn pareto_indices_valid_and_ascending(
        search_seed in any::<u64>(),
        trials in 2u32..=5,
    ) {
        let scenario = minimal_search_scenario(0.3, 0.9);
        let cfg = search_config(SearchMethod::Random, search_seed, trials);
        let result = run_search(&scenario, &cfg).expect("search runs");
        let n = result.trials.len() as u32;
        for w in result.pareto_indices.windows(2) {
            prop_assert!(
                w[0] < w[1],
                "pareto indices not strictly ascending: {} then {}",
                w[0],
                w[1]
            );
        }
        for &idx in &result.pareto_indices {
            prop_assert!(idx < n, "pareto index {idx} out of range (n={n})");
        }
    }
}
