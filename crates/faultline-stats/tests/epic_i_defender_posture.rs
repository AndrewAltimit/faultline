//! Integration tests for defender-posture optimization.
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
fn counter_recommendation_elides_when_no_owner_set() {
    // Gating condition #2: even with a baseline, if no decision
    // variable carries an `owner` the section is suppressed — without
    // ownership the analyst can't read "whose posture this is" off the
    // table and the section adds noise. Pin the legacy contract for
    // the strategy_search_demo scenario and equivalents that don't
    // tag their variables.
    let mut scenario = load_posture();
    // Strip every owner; keep paths and domains intact.
    for var in &mut scenario.strategy_space.variables {
        var.owner = None;
    }
    let config = posture_config(SearchMethod::Grid, true);
    let result = run_search(&scenario, &config).expect("search");
    let md = report::render_search_markdown(&result, &scenario);
    assert!(
        !md.contains("## Counter-Recommendation"),
        "section must not render when no decision variable carries an owner"
    );
}

#[test]
fn counter_recommendation_elides_when_pareto_frontier_empty() {
    // Gating condition #3: empty Pareto frontier → skip section. We
    // can't easily make the engine produce an empty frontier (the
    // dominance check always picks up at least one trial when the
    // search ran), so this test exercises the report renderer
    // directly with a hand-built `SearchResult` that has zero
    // pareto_indices but a baseline + owner-tagged variable.
    use faultline_stats::search::{SearchResult, SearchTrial};
    use faultline_types::stats::MonteCarloSummary;
    use std::collections::BTreeMap;

    let scenario = load_posture();
    let mut summary = MonteCarloSummary {
        total_runs: 4,
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
        alliance_dynamics: None,
        supply_pressure_summaries: ::std::collections::BTreeMap::new(),
        civilian_activation_summaries: ::std::collections::BTreeMap::new(),
        tech_cost_summaries: ::std::collections::BTreeMap::new(),
        calibration: None,
    };
    summary.win_rates.insert(FactionId::from("blue"), 0.5);

    let baseline = SearchTrial {
        trial_index: None,
        assignments: vec![],
        objective_values: BTreeMap::new(),
        summary: summary.clone(),
    };
    let result = SearchResult {
        method: SearchMethod::Grid,
        trials: vec![SearchTrial {
            trial_index: Some(0),
            assignments: vec![],
            objective_values: BTreeMap::new(),
            summary,
        }],
        // Empty Pareto: section must not render even with baseline +
        // owner.
        pareto_indices: vec![],
        best_by_objective: BTreeMap::new(),
        objectives: vec![SearchObjective::MaximizeWinRate {
            faction: FactionId::from("blue"),
        }],
        baseline: Some(baseline),
    };

    let md = report::render_search_markdown(&result, &scenario);
    assert!(
        !md.contains("## Counter-Recommendation"),
        "section must not render with empty Pareto frontier"
    );
}

#[test]
fn counter_recommendation_groups_decision_variables_when_multiple_owners() {
    // The "Decision variables by owner" subsection only renders when
    // the strategy_space has >1 distinct owner — otherwise the grouping
    // is redundant. Pin both branches: bundled scenario (one owner) →
    // no subsection; mutated copy with two owners → subsection appears.
    let single_owner = load_posture();
    let config = posture_config(SearchMethod::Grid, true);
    let result_single = run_search(&single_owner, &config).expect("single-owner search");
    let md_single = report::render_search_markdown(&result_single, &single_owner);
    assert!(
        !md_single.contains("### Decision variables by owner"),
        "single-owner space must not emit the grouping subsection"
    );

    // Mutate one decision variable's owner to a synthetic second
    // faction so the renderer sees two distinct owners. (Path
    // resolution is not affected — the owner is informational only.)
    let mut multi_owner = single_owner.clone();
    if let Some(var) = multi_owner.strategy_space.variables.get_mut(0) {
        var.owner = Some(FactionId::from("red"));
    }
    let result_multi = run_search(&multi_owner, &config).expect("multi-owner search");
    let md_multi = report::render_search_markdown(&result_multi, &multi_owner);
    assert!(
        md_multi.contains("### Decision variables by owner"),
        "multi-owner space must emit the grouping subsection"
    );
    // Both owner labels must appear in the subsection.
    assert!(md_multi.contains("`blue`"));
    assert!(md_multi.contains("`red`"));
}

#[test]
fn counter_recommendation_renders_per_objective_delta_table() {
    // The Counter-Recommendation section emits a delta table per
    // Pareto-frontier trial. For each objective we expect:
    // - the objective label as a row,
    // - "max" / "min" direction tag,
    // - a baseline value,
    // - a trial value,
    // - a signed delta with one of "+" / "·" / "−" glyphs,
    // - a yes/no improvement flag.
    let scenario = load_posture();
    let config = posture_config(SearchMethod::Grid, true);
    let result = run_search(&scenario, &config).expect("search");
    let md = report::render_search_markdown(&result, &scenario);

    // Header row of the per-trial delta table.
    assert!(
        md.contains("| Objective | Direction | Baseline | Trial | Δ | Improvement? |"),
        "delta table header must appear in Counter-Rec section"
    );
    // Direction tags appear (covers max-aligned MaximizeWinRate +
    // MaximizeDetection and the min-aligned MinimizeMaxChainSuccess
    // from the bundled scenario's objective list).
    assert!(md.contains("| max |"));
    assert!(md.contains("| min |"));
    // Improvement column carries at least one "no" or "yes" cell —
    // baseline-vs-trial deltas are always one or the other in the
    // bundled scenario.
    assert!(
        md.contains("| yes |") || md.contains("| no |"),
        "improvement column must surface yes/no cells"
    );
}

#[test]
fn counter_recommendation_wilson_ci_panel_renders_for_win_rate() {
    // The Wilson CI panel only renders when the objectives contain a
    // `MaximizeWinRate` variant — the only currently rate-valued
    // objective with a Wilson formula on the search summary. Pin
    // both branches: present (panel renders) and absent (no panel).
    let scenario = load_posture();
    let mut config = posture_config(SearchMethod::Grid, true);

    // Branch 1: with MaximizeWinRate, the panel must render.
    let result_with_winrate = run_search(&scenario, &config).expect("search with winrate");
    let md_with = report::render_search_markdown(&result_with_winrate, &scenario);
    assert!(
        md_with.contains("Win-rate Wilson 95% CIs:"),
        "Wilson CI panel must render when MaximizeWinRate is in the objective list"
    );

    // Branch 2: drop MaximizeWinRate, keep the defender-aligned
    // objectives. The Wilson panel must disappear; the rest of the
    // section stays intact.
    config.objectives = vec![
        SearchObjective::MinimizeMaxChainSuccess,
        SearchObjective::MaximizeDetection,
    ];
    let result_without_winrate = run_search(&scenario, &config).expect("search without winrate");
    let md_without = report::render_search_markdown(&result_without_winrate, &scenario);
    assert!(
        md_without.contains("## Counter-Recommendation"),
        "section must still render with non-rate objectives"
    );
    assert!(
        !md_without.contains("Win-rate Wilson 95% CIs:"),
        "Wilson CI panel must not render when no MaximizeWinRate objective is present"
    );
}

#[test]
fn manifest_search_mode_backward_compat_default_compute_baseline() {
    // Older manifests predating the baseline-trial feature lacked
    // the `compute_baseline`
    // field. `#[serde(default)]` on the new field must let those
    // manifests deserialize cleanly with `compute_baseline = false`,
    // matching the SearchResult shape they were originally hashed
    // under (no baseline trial).
    use faultline_stats::manifest::ManifestMode;

    // Hand-craft the JSON that an older manifest would have
    // produced — note the absence of `compute_baseline`.
    let legacy_json = r#"{
        "kind": "search",
        "method": "grid",
        "trials": 4,
        "search_seed": 99,
        "objectives": ["maximize_win_rate:alpha", "minimize_duration"]
    }"#;

    let parsed: ManifestMode =
        serde_json::from_str(legacy_json).expect("legacy manifest must deserialize");
    match parsed {
        ManifestMode::Search {
            compute_baseline, ..
        } => {
            assert!(
                !compute_baseline,
                "legacy manifest must default compute_baseline to false"
            );
        },
        other => panic!("expected ManifestMode::Search, got {other:?}"),
    }
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
