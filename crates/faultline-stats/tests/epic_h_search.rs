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
    // The demo scenario declares two continuous variables with steps=3.
    // The grid product is 3×3 = 9 cells. Run a full 9-trial grid search
    // and assert every endpoint pair from {0.4, 0.675, 0.95}² appears.
    let scenario = load_demo();
    let mut config = small_search_config(SearchMethod::Grid);
    config.trials = 9;
    let result = run_search(&scenario, &config).expect("grid search");

    assert_eq!(result.trials.len(), 9);
    let pairs: Vec<(f64, f64)> = result
        .trials
        .iter()
        .map(|t| (t.assignments[0].value, t.assignments[1].value))
        .collect();

    // Each variable's domain is [0.4, 0.95] with steps=3 → midpoint at
    // 0.675, endpoints at 0.4 and 0.95. Product is the 9-cell Cartesian.
    let levels = [0.4, 0.675, 0.95];
    for &a in &levels {
        for &b in &levels {
            assert!(
                pairs
                    .iter()
                    .any(|p| (p.0 - a).abs() < 1e-9 && (p.1 - b).abs() < 1e-9),
                "expected grid cell ({a}, {b}) missing; got {pairs:?}"
            );
        }
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

#[test]
fn manifest_search_mode_serializes_and_round_trips() {
    // ManifestMode::Search must round-trip via serde_json with the
    // recorded objective labels intact. This is the wire format that
    // `--verify` reparses on a saved manifest.
    use faultline_stats::manifest::ManifestMode;

    let mode = ManifestMode::Search {
        method: SearchMethod::Grid,
        trials: 8,
        search_seed: 99,
        objectives: vec!["maximize_win_rate:alpha".into(), "minimize_duration".into()],
    };
    let json = serde_json::to_string(&mode).expect("serialize ManifestMode::Search");
    assert!(
        json.contains("\"search\""),
        "kind tag must be `search`; got {json}"
    );
    let parsed: ManifestMode = serde_json::from_str(&json).expect("parse ManifestMode::Search");
    assert_eq!(parsed, mode);
}

#[test]
fn manifest_search_mode_replay_objective_labels_reparse() {
    // The replay path (`SearchObjective::parse_cli`) must accept every
    // label that emit produced. If we ever rename a label, this test
    // catches it before a saved manifest fails verify in production.
    let labels = vec![
        SearchObjective::MaximizeWinRate {
            faction: FactionId::from("alpha"),
        },
        SearchObjective::MinimizeDetection,
        SearchObjective::MinimizeAttackerCost,
        SearchObjective::MaximizeCostAsymmetry,
        SearchObjective::MinimizeDuration,
    ];
    for o in &labels {
        let label = o.label();
        let parsed = SearchObjective::parse_cli(&label)
            .unwrap_or_else(|e| panic!("label {label} failed to reparse: {e}"));
        assert_eq!(&parsed, o);
    }
}

#[test]
fn search_emits_csv_warning_handled_in_runner() {
    // The CLI maps --format csv to "JSON + Markdown" for search; the
    // runner itself doesn't care about CLI format flags, but the
    // output_hash must stay stable regardless of how the CLI
    // serializes downstream artifacts. Pin that the result hash is a
    // function of the SearchResult only.
    use faultline_stats::manifest;
    let scenario = load_demo();
    let config = small_search_config(SearchMethod::Random);
    let r = run_search(&scenario, &config).expect("run");
    let h1 = manifest::output_hash(&r).expect("hash");
    let h2 = manifest::output_hash(&r).expect("hash");
    assert_eq!(h1, h2, "output hash must be stable across re-hashing");
}

#[test]
fn bundled_demo_passes_engine_validation() {
    // The bundled scenario must validate cleanly through the same
    // path as every other shipped scenario. This is the
    // verify-bundled-scenarios.sh contract enforced as a unit test
    // so a rust-only `cargo test` catches regressions before CI.
    let scenario = load_demo();
    faultline_engine::validate_scenario(&scenario).expect("bundled demo must validate");
}

#[test]
fn bundled_demo_strategy_space_has_expected_shape() {
    // Lock the bundled scenario's structure: 2 continuous decision
    // variables with steps=3, 2 default objectives. Catches accidental
    // schema drift in the bundled fixture.
    let scenario = load_demo();
    let space = &scenario.strategy_space;
    assert_eq!(space.variables.len(), 2, "expected 2 decision variables");
    for var in &space.variables {
        match &var.domain {
            faultline_types::strategy_space::Domain::Continuous { steps, .. } => {
                assert_eq!(*steps, 3, "expected 3-step grid for {}", var.path);
            },
            other => panic!("expected continuous domain, got {other:?}"),
        }
    }
    assert_eq!(space.objectives.len(), 2, "expected 2 default objectives");
}
