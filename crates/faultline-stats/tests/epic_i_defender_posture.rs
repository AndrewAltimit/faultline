//! Integration tests for Epic I — defender-posture optimization.
//!
//! Exercises the full pipeline against the bundled
//! `defender_posture_optimization` scenario: search-mode determinism
//! with a baseline trial, Counter-Recommendation report rendering,
//! and the round-trip determinism contract for the new manifest field.

use faultline_stats::manifest;
use faultline_stats::report;
use faultline_stats::search::{SearchConfig, SearchMethod, run_search};
use faultline_types::ids::FactionId;
use faultline_types::migration::load_scenario_str;
use faultline_types::stats::MonteCarloConfig;
use faultline_types::strategy_space::SearchObjective;

const POSTURE_SCENARIO: &str =
    include_str!("../../../scenarios/defender_posture_optimization.toml");

fn load_posture() -> faultline_types::scenario::Scenario {
    load_scenario_str(POSTURE_SCENARIO)
        .expect("bundled defender_posture_optimization must load")
        .scenario
}

fn posture_config(method: SearchMethod, compute_baseline: bool) -> SearchConfig {
    // Tiny batch — search emits 8 cells over 8 inner runs each → 64
    // engine passes per test, well under the integration-test budget.
    SearchConfig {
        trials: 8,
        method,
        search_seed: 17,
        mc_config: MonteCarloConfig {
            num_runs: 8,
            seed: Some(7),
            collect_snapshots: false,
            parallel: false,
        },
        objectives: vec![
            SearchObjective::MaximizeWinRate {
                faction: FactionId::from("blue"),
            },
            SearchObjective::MinimizeMaxChainSuccess,
            SearchObjective::MaximizeDetection,
        ],
        compute_baseline,
    }
}

#[test]
fn defender_posture_search_emits_baseline_under_fixed_seeds() {
    // The baseline trial is part of the determinism contract — same
    // search_seed + mc_seed must produce the same baseline objective
    // values across re-runs.
    let scenario = load_posture();
    let config = posture_config(SearchMethod::Grid, true);

    let r1 = run_search(&scenario, &config).expect("first run");
    let r2 = run_search(&scenario, &config).expect("second run");

    let b1 = r1.baseline.as_ref().expect("baseline 1");
    let b2 = r2.baseline.as_ref().expect("baseline 2");
    assert_eq!(
        b1.objective_values, b2.objective_values,
        "baseline objective values must be deterministic"
    );

    let h1 = manifest::output_hash(&r1).expect("hash 1");
    let h2 = manifest::output_hash(&r2).expect("hash 2");
    assert_eq!(
        h1, h2,
        "search output hash must be stable when baseline is included"
    );
}

#[test]
fn baseline_changes_output_hash_when_toggled() {
    // Whether the baseline is computed must affect the output hash:
    // the baseline trial is part of the SearchResult shape and the
    // manifest's `compute_baseline` field is what tells the verifier
    // which form to replay.
    let scenario = load_posture();
    let on = run_search(&scenario, &posture_config(SearchMethod::Grid, true)).expect("on");
    let off = run_search(&scenario, &posture_config(SearchMethod::Grid, false)).expect("off");

    let h_on = manifest::output_hash(&on).expect("hash on");
    let h_off = manifest::output_hash(&off).expect("hash off");
    assert_ne!(
        h_on, h_off,
        "output hash must differ when baseline presence differs"
    );
    assert!(on.baseline.is_some());
    assert!(off.baseline.is_none());
}

#[test]
fn counter_recommendation_section_renders_when_owner_present() {
    // The Counter-Recommendation section is gated on:
    // 1. baseline present,
    // 2. at least one decision variable with `owner` set,
    // 3. non-empty Pareto frontier.
    // The bundled scenario satisfies all three.
    let scenario = load_posture();
    let config = posture_config(SearchMethod::Grid, true);
    let result = run_search(&scenario, &config).expect("search");
    let md = report::render_search_markdown(&result, &scenario);
    assert!(
        md.contains("## Counter-Recommendation"),
        "Counter-Recommendation header must appear; got:\n{md}"
    );
    // The section must mention the baseline as the comparison anchor.
    assert!(
        md.contains("do-nothing baseline"),
        "section must explain the baseline anchor"
    );
}

#[test]
fn counter_recommendation_elides_when_no_baseline() {
    // Without a baseline (compute_baseline=false), the section must
    // disappear — there's nothing to compare against. The rest of the
    // search report stays intact.
    let scenario = load_posture();
    let config = posture_config(SearchMethod::Grid, false);
    let result = run_search(&scenario, &config).expect("search");
    let md = report::render_search_markdown(&result, &scenario);
    assert!(
        !md.contains("## Counter-Recommendation"),
        "section must not render without a baseline"
    );
    // The rest of the search report must still appear.
    assert!(md.contains("## Pareto Frontier"));
    assert!(md.contains("## Trial Detail"));
}

#[test]
fn defender_objectives_move_under_search() {
    // Sanity check that the bundled scenario's decision variables
    // actually push the new defender-aligned objectives. If a future
    // refactor accidentally inerts these variables (the trap that
    // caught the first draft of this scenario), at least one
    // objective should still differ across the 8 grid cells.
    let scenario = load_posture();
    let config = posture_config(SearchMethod::Grid, false);
    let result = run_search(&scenario, &config).expect("search");

    let detection_label = SearchObjective::MaximizeDetection.label();
    let chain_success_label = SearchObjective::MinimizeMaxChainSuccess.label();

    let detections: Vec<f64> = result
        .trials
        .iter()
        .map(|t| {
            t.objective_values
                .get(&detection_label)
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    let chain_success: Vec<f64> = result
        .trials
        .iter()
        .map(|t| {
            t.objective_values
                .get(&chain_success_label)
                .copied()
                .unwrap_or(0.0)
        })
        .collect();

    let det_min = detections.iter().copied().fold(f64::INFINITY, f64::min);
    let det_max = detections.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let cs_min = chain_success.iter().copied().fold(f64::INFINITY, f64::min);
    let cs_max = chain_success
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    assert!(
        (det_max - det_min) > 1e-6 || (cs_max - cs_min) > 1e-6,
        "decision variables must move at least one defender objective; \
         detection range = [{det_min}, {det_max}], \
         chain_success range = [{cs_min}, {cs_max}]"
    );
}
