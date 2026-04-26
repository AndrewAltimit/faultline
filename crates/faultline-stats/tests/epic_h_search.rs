//! Integration tests for Epic H — strategy-search manifest replay and
//! end-to-end determinism through the bundled `strategy_search_demo`
//! scenario.
//!
//! Round-one search runs a `MonteCarloRunner::run` per trial, so the
//! per-test compute budget is dominated by `trials * inner_runs *
//! engine_passes`. Inner counts are kept tiny (4 trials × 10 runs)
//! because the determinism contract being tested is a property of the
//! seeding, not the convergence of the underlying simulator.

use faultline_stats::manifest;
use faultline_stats::search::{SearchConfig, SearchMethod, run_search};
use faultline_types::ids::FactionId;
use faultline_types::migration::load_scenario_str;
use faultline_types::stats::MonteCarloConfig;
use faultline_types::strategy_space::SearchObjective;

const DEMO_SCENARIO: &str = include_str!("../../../scenarios/strategy_search_demo.toml");

fn load_demo() -> faultline_types::scenario::Scenario {
    load_scenario_str(DEMO_SCENARIO)
        .expect("bundled strategy_search_demo must load")
        .scenario
}

fn small_search_config(method: SearchMethod) -> SearchConfig {
    SearchConfig {
        trials: 4,
        method,
        search_seed: 99,
        mc_config: MonteCarloConfig {
            num_runs: 10,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        },
        objectives: vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("alpha"),
            },
            SearchObjective::MinimizeDuration,
        ],
    }
}

#[test]
fn search_emits_stable_output_hash_under_fixed_seeds() {
    // Two consecutive runs must produce identical output_hash. This is
    // the manifest-replay precondition: the verify path computes the
    // output hash of the live replay and compares it to the saved one.
    let scenario = load_demo();
    let config = small_search_config(SearchMethod::Random);

    let r1 = run_search(&scenario, &config).expect("first run");
    let r2 = run_search(&scenario, &config).expect("second run");

    let h1 = manifest::output_hash(&r1).expect("hash r1");
    let h2 = manifest::output_hash(&r2).expect("hash r2");
    assert_eq!(
        h1, h2,
        "search output hash must be stable across re-runs of the same config"
    );
}

#[test]
fn search_grid_method_covers_demo_strategy_space() {
    // The demo scenario declares two continuous variables with steps=2.
    // The grid product is 2×2 = 4 cells, so a 4-trial grid search must
    // emit exactly the four endpoint pairs in declaration order.
    let scenario = load_demo();
    let config = small_search_config(SearchMethod::Grid);
    let result = run_search(&scenario, &config).expect("grid search");

    assert_eq!(result.trials.len(), 4);
    let pairs: Vec<(f64, f64)> = result
        .trials
        .iter()
        .map(|t| (t.assignments[0].value, t.assignments[1].value))
        .collect();

    // Each variable's domain is [0.5, 0.9] with steps=2 → endpoints
    // {0.5, 0.9}. Product = {(0.5,0.5), (0.5,0.9), (0.9,0.5), (0.9,0.9)}.
    let expected: Vec<(f64, f64)> = vec![(0.5, 0.5), (0.5, 0.9), (0.9, 0.5), (0.9, 0.9)];
    for e in &expected {
        assert!(
            pairs
                .iter()
                .any(|p| (p.0 - e.0).abs() < 1e-9 && (p.1 - e.1).abs() < 1e-9),
            "expected grid cell {e:?} missing; got {pairs:?}"
        );
    }
}

#[test]
fn search_objectives_round_trip_through_label_form() {
    // ManifestMode::Search records objective *labels* (strings), not
    // structured enum variants, so the manifest schema is stable across
    // future objective additions. The verify path reparses the labels
    // back into the typed form. Pin that round-trip here.
    let scenario = load_demo();
    let config = small_search_config(SearchMethod::Random);
    let result = run_search(&scenario, &config).expect("search");

    for obj in &result.objectives {
        let label = obj.label();
        let parsed = SearchObjective::parse_cli(&label).expect("parses back");
        assert_eq!(&parsed, obj, "label {label} must round-trip");
    }
}

#[test]
fn search_rejects_path_that_does_not_resolve() {
    // Path-resolution sanity check is the search runner's
    // responsibility (engine validate_scenario can't reach set_param).
    // A bogus path must surface as InvalidConfig, not panic mid-run.
    let mut scenario = load_demo();
    scenario
        .strategy_space
        .variables
        .push(faultline_types::strategy_space::DecisionVariable {
            path: "faction.does_not_exist.initial_morale".into(),
            owner: None,
            domain: faultline_types::strategy_space::Domain::Continuous {
                low: 0.0,
                high: 1.0,
                steps: 2,
            },
        });
    let config = small_search_config(SearchMethod::Random);
    let err = run_search(&scenario, &config).expect_err("bogus path must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("does_not_exist"),
        "error must name the bad path; got: {msg}"
    );
}
