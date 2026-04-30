//! Integration tests for Epic I — round two: robustness analysis.
//!
//! Exercises the full pipeline against the bundled
//! `defender_robustness_demo` scenario: cell determinism, per-posture
//! rollup correctness (worst/best direction-awareness), and validation
//! rejections for misshapen profiles.

use faultline_stats::counterfactual::ParamOverride;
use faultline_stats::robustness::{DefenderPosture, RobustnessConfig, run_robustness};
use faultline_stats::{StatsError, manifest, report};
use faultline_types::ids::FactionId;
use faultline_types::migration::load_scenario_str;
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloConfig;
use faultline_types::strategy_space::SearchObjective;

const ROBUSTNESS_SCENARIO: &str = include_str!("../../../scenarios/defender_robustness_demo.toml");

fn load_scenario() -> Scenario {
    load_scenario_str(ROBUSTNESS_SCENARIO)
        .expect("bundled defender_robustness_demo must load")
        .scenario
}

fn small_config(postures: Vec<DefenderPosture>, include_baseline: bool) -> RobustnessConfig {
    RobustnessConfig {
        postures,
        include_baseline,
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
        ],
    }
}

fn pareto_postures() -> Vec<DefenderPosture> {
    // Two synthetic postures, deliberately different so cell deltas
    // are observable. Reuses paths from the bundled scenario's
    // strategy space so they resolve via `set_param`.
    vec![
        DefenderPosture {
            label: "low_detect".to_string(),
            assignments: vec![
                ParamOverride {
                    path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
                    value: 0.04,
                },
                ParamOverride {
                    path: "kill_chain.red_op.phase.infiltrate.detection_probability_per_tick"
                        .into(),
                    value: 0.08,
                },
            ],
        },
        DefenderPosture {
            label: "high_detect".to_string(),
            assignments: vec![
                ParamOverride {
                    path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
                    value: 0.18,
                },
                ParamOverride {
                    path: "kill_chain.red_op.phase.infiltrate.detection_probability_per_tick"
                        .into(),
                    value: 0.28,
                },
            ],
        },
    ]
}

#[test]
fn robustness_run_is_deterministic_under_fixed_seeds() {
    // Two runs with identical config must produce identical
    // cells / rollups / output_hash. This is the core determinism
    // contract — the robustness layer has no RNG, so any drift would
    // come from MonteCarloRunner::run alone.
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);

    let r1 = run_robustness(&scenario, &config).expect("first run");
    let r2 = run_robustness(&scenario, &config).expect("second run");

    let h1 = manifest::output_hash(&r1).expect("hash 1");
    let h2 = manifest::output_hash(&r2).expect("hash 2");
    assert_eq!(h1, h2, "robustness output_hash must match across re-runs");
    assert_eq!(
        r1.cells.len(),
        r2.cells.len(),
        "cells count must match: {} vs {}",
        r1.cells.len(),
        r2.cells.len()
    );
    for (c1, c2) in r1.cells.iter().zip(r2.cells.iter()) {
        assert_eq!(c1.posture_label, c2.posture_label);
        assert_eq!(c1.profile_name, c2.profile_name);
        assert_eq!(
            c1.objective_values, c2.objective_values,
            "cell objective values must match"
        );
    }
}

#[test]
fn baseline_prepended_when_include_baseline_true() {
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    assert!(!result.postures.is_empty());
    assert_eq!(
        result.postures[0].label, "baseline",
        "baseline posture must be at index 0 when include_baseline=true"
    );
    assert_eq!(
        result.baseline_label,
        Some("baseline".to_string()),
        "baseline_label must surface to the report renderer"
    );
}

#[test]
fn baseline_omitted_when_include_baseline_false() {
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), false);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    assert!(
        result.postures.iter().all(|p| p.label != "baseline"),
        "no posture should be labeled 'baseline' when the flag is off"
    );
    assert!(result.baseline_label.is_none());
}

#[test]
fn cell_count_matches_postures_times_profiles() {
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    let expected = result.postures.len() * result.profiles.len();
    assert_eq!(
        result.cells.len(),
        expected,
        "cell count must equal postures × profiles ({} × {} = {})",
        result.postures.len(),
        result.profiles.len(),
        expected
    );
}

#[test]
fn worst_per_objective_is_direction_aware() {
    // For a `MinimizeMaxChainSuccess` objective (minimize-direction),
    // worst is the LARGEST observed value — the profile under which
    // the chain is most likely to succeed against that defender. The
    // rollup must surface that direction, not the smallest value.
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    let label = SearchObjective::MinimizeMaxChainSuccess.label();

    for (rollup, posture) in result.rollups.iter().zip(result.postures.iter()) {
        let worst = rollup
            .worst_per_objective
            .get(&label)
            .expect("worst entry must exist for declared objective");
        let best = rollup
            .best_per_objective
            .get(&label)
            .expect("best entry must exist for declared objective");

        // Find max across the row's cells.
        let row_start = result
            .postures
            .iter()
            .position(|p| p.label == posture.label)
            .expect("posture index lookup")
            * result.profiles.len();
        let row = &result.cells[row_start..row_start + result.profiles.len()];
        let max_val = row
            .iter()
            .map(|c| c.objective_values.get(&label).copied().unwrap_or(f64::NAN))
            .fold(f64::NEG_INFINITY, f64::max);
        let min_val = row
            .iter()
            .map(|c| c.objective_values.get(&label).copied().unwrap_or(f64::NAN))
            .fold(f64::INFINITY, f64::min);

        assert!(
            (worst.value - max_val).abs() < 1e-9,
            "minimize-direction worst must be the row max: posture={}, worst.value={}, max={}",
            posture.label,
            worst.value,
            max_val
        );
        assert!(
            (best.value - min_val).abs() < 1e-9,
            "minimize-direction best must be the row min: posture={}, best.value={}, min={}",
            posture.label,
            best.value,
            min_val
        );
    }
}

#[test]
fn baseline_only_run_evaluates_natural_state_per_profile() {
    // No postures + include_baseline=true: every profile should
    // evaluate against the natural-state defender. Cell count =
    // 1 (baseline) × profiles_count.
    let scenario = load_scenario();
    let config = small_config(Vec::new(), true);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    assert_eq!(result.postures.len(), 1);
    assert_eq!(result.postures[0].label, "baseline");
    assert_eq!(result.cells.len(), result.profiles.len());
}

#[test]
fn rejects_no_postures_no_baseline() {
    let scenario = load_scenario();
    let config = small_config(Vec::new(), false);
    let err = run_robustness(&scenario, &config).expect_err("must reject empty input");
    assert!(matches!(err, StatsError::InvalidConfig(_)));
}

#[test]
fn rejects_empty_objectives() {
    let scenario = load_scenario();
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
    assert!(matches!(err, StatsError::InvalidConfig(_)));
}

#[test]
fn rejects_invalid_posture_path() {
    let scenario = load_scenario();
    let bad_postures = vec![DefenderPosture {
        label: "bogus".to_string(),
        assignments: vec![ParamOverride {
            path: "totally.fake.path".into(),
            value: 0.5,
        }],
    }];
    let config = small_config(bad_postures, false);
    let err =
        run_robustness(&scenario, &config).expect_err("must reject unresolvable posture path");
    let msg = format!("{err}");
    assert!(
        msg.contains("totally.fake.path"),
        "error must name the bad path; got: {msg}"
    );
}

#[test]
fn rendered_report_contains_required_sections() {
    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);
    let result = run_robustness(&scenario, &config).expect("must succeed");
    let md = report::render_robustness_markdown(&result, &scenario);

    assert!(md.contains("# Faultline Robustness Report"));
    assert!(
        md.contains("## Attacker profiles"),
        "must list profile metadata"
    );
    assert!(
        md.contains("## Per-posture rollup"),
        "must include rollup section"
    );
    assert!(
        md.contains("## Cell matrix"),
        "must include cell matrix for small N"
    );
    assert!(
        md.contains("## Reproducibility"),
        "must include reproducibility footer"
    );
}

#[test]
fn engine_validation_rejects_duplicate_profile_names() {
    use faultline_engine::validate_scenario;
    use faultline_types::strategy_space::{AttackerProfile, ProfileAssignment};

    let mut scenario = load_scenario();
    let dup = AttackerProfile {
        name: "opportunist".into(), // already declared in the bundled scenario
        description: "fake".into(),
        faction: None,
        assignments: vec![ProfileAssignment {
            path: "kill_chain.red_op.phase.recon.base_success_probability".into(),
            value: 0.5,
        }],
    };
    scenario.strategy_space.attacker_profiles.push(dup);
    let err = validate_scenario(&scenario).expect_err("duplicate profile name must reject");
    assert!(
        format!("{err}").contains("declared more than once"),
        "error must call out duplication"
    );
}

#[test]
fn engine_validation_rejects_empty_profile_assignments() {
    use faultline_engine::validate_scenario;
    use faultline_types::strategy_space::AttackerProfile;

    let mut scenario = load_scenario();
    scenario
        .strategy_space
        .attacker_profiles
        .push(AttackerProfile {
            name: "noop".into(),
            description: String::new(),
            faction: None,
            assignments: Vec::new(),
        });
    let err = validate_scenario(&scenario).expect_err("empty assignments must reject");
    assert!(
        format!("{err}").contains("no assignments"),
        "error must call out empty assignments"
    );
}

#[test]
fn engine_validation_rejects_unknown_profile_faction() {
    use faultline_engine::validate_scenario;
    use faultline_types::strategy_space::{AttackerProfile, ProfileAssignment};

    let mut scenario = load_scenario();
    scenario
        .strategy_space
        .attacker_profiles
        .push(AttackerProfile {
            name: "ghosted".into(),
            description: String::new(),
            faction: Some(FactionId::from("ghost_faction")),
            assignments: vec![ProfileAssignment {
                path: "kill_chain.red_op.phase.recon.base_success_probability".into(),
                value: 0.5,
            }],
        });
    let err = validate_scenario(&scenario).expect_err("unknown faction must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("ghost_faction"),
        "error must name the unknown faction; got: {msg}"
    );
}

#[test]
fn profiles_actually_produce_different_cell_values() {
    // Smoke test: if the profile's assignments weren't being applied,
    // every cell in a row would be identical. This regression catches
    // a future refactor that silently drops the profile-application
    // loop in `run_robustness`. We assert the row is NOT trivially
    // constant for at least one objective.
    let scenario = load_scenario();
    // Inflate run count so MC noise can't make three different
    // parameter regimes look identical by chance.
    let config = RobustnessConfig {
        postures: Vec::new(),
        include_baseline: true,
        mc_config: MonteCarloConfig {
            num_runs: 32,
            seed: Some(7),
            collect_snapshots: false,
            parallel: false,
        },
        objectives: vec![SearchObjective::MinimizeMaxChainSuccess],
    };
    let result = run_robustness(&scenario, &config).expect("must succeed");
    let label = SearchObjective::MinimizeMaxChainSuccess.label();

    // One posture row × N profile cells. The bundled scenario has
    // three profiles with deliberately different attacker rates; the
    // resulting per-cell `minimize_max_chain_success` values must
    // not all be identical.
    let row: Vec<f64> = result
        .cells
        .iter()
        .map(|c| c.objective_values.get(&label).copied().unwrap_or(f64::NAN))
        .collect();
    assert!(row.len() >= 2, "need at least 2 profiles for this check");
    let first = row[0];
    let any_differs = row.iter().skip(1).any(|v| (v - first).abs() > 1e-9);
    assert!(
        any_differs,
        "profiles must produce distinguishable cell values; got identical values: {row:?}",
    );
}

#[test]
fn engine_validation_rejects_within_profile_duplicate_path() {
    use faultline_engine::validate_scenario;
    use faultline_types::strategy_space::{AttackerProfile, ProfileAssignment};

    let mut scenario = load_scenario();
    scenario
        .strategy_space
        .attacker_profiles
        .push(AttackerProfile {
            name: "double_assign".into(),
            description: String::new(),
            faction: None,
            assignments: vec![
                ProfileAssignment {
                    path: "kill_chain.red_op.phase.recon.base_success_probability".into(),
                    value: 0.5,
                },
                ProfileAssignment {
                    path: "kill_chain.red_op.phase.recon.base_success_probability".into(),
                    value: 0.7,
                },
            ],
        });
    let err = validate_scenario(&scenario).expect_err("duplicate path within profile must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("more than once"),
        "error must call out duplication; got: {msg}"
    );
}

#[test]
fn engine_validation_rejects_nan_profile_value() {
    use faultline_engine::validate_scenario;
    use faultline_types::strategy_space::{AttackerProfile, ProfileAssignment};

    let mut scenario = load_scenario();
    scenario
        .strategy_space
        .attacker_profiles
        .push(AttackerProfile {
            name: "nan_value".into(),
            description: String::new(),
            faction: None,
            assignments: vec![ProfileAssignment {
                path: "kill_chain.red_op.phase.recon.base_success_probability".into(),
                value: f64::NAN,
            }],
        });
    let err = validate_scenario(&scenario).expect_err("NaN value must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("finite"),
        "error must mention the finiteness requirement; got: {msg}"
    );
}

#[test]
fn rejects_within_posture_duplicate_path() {
    let scenario = load_scenario();
    let bad = vec![DefenderPosture {
        label: "double_assign".to_string(),
        assignments: vec![
            ParamOverride {
                path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
                value: 0.04,
            },
            ParamOverride {
                path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
                value: 0.18,
            },
        ],
    }];
    let config = small_config(bad, false);
    let err =
        run_robustness(&scenario, &config).expect_err("duplicate path within posture must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("more than once"),
        "error must call out duplication; got: {msg}"
    );
}

#[test]
fn posture_and_profile_path_collision_lets_profile_win() {
    // Documented contract: when a posture and a profile both assign to
    // the same path, the profile wins (applied second). This test pins
    // that behavior so a future change to the order-of-application
    // surfaces as a deliberate test edit, not a silent semantic flip.
    //
    // We construct a posture that sets the recon detection probability
    // to a defender-friendly 0.18, plus a synthetic profile that
    // *also* assigns to that same path with an attacker-friendly 0.02.
    // The profile's assignment should be the value the engine sees.
    let mut scenario = load_scenario();
    scenario.strategy_space.attacker_profiles.clear();
    scenario.strategy_space.attacker_profiles.push(
        faultline_types::strategy_space::AttackerProfile {
            name: "overrides_posture".into(),
            description: "Profile that intentionally collides with the posture path".into(),
            faction: Some(FactionId::from("red")),
            assignments: vec![faultline_types::strategy_space::ProfileAssignment {
                path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
                value: 0.02, // attacker-friendly; should override posture's 0.18
            }],
        },
    );

    // Posture that disagrees with the profile on the same path.
    let posture = vec![DefenderPosture {
        label: "conflicted".to_string(),
        assignments: vec![ParamOverride {
            path: "kill_chain.red_op.phase.recon.detection_probability_per_tick".into(),
            value: 0.18,
        }],
    }];

    // Run the conflicted (posture, profile) cell and a second run with
    // the posture disabled but the profile kept. Same MC seed, same
    // scenario otherwise — the two cells must produce equal values
    // because the profile's 0.02 overrides the posture's 0.18 in
    // both runs.
    let config_with_posture = small_config(posture, false);
    let mut config_no_posture = config_with_posture.clone();
    config_no_posture.postures.clear();
    config_no_posture.include_baseline = true;

    let r_with = run_robustness(&scenario, &config_with_posture).expect("with posture");
    let r_without = run_robustness(&scenario, &config_no_posture).expect("baseline only");

    let label = SearchObjective::MinimizeMaxChainSuccess.label();
    let v_with = r_with.cells[0]
        .objective_values
        .get(&label)
        .copied()
        .expect("value");
    let v_without = r_without.cells[0]
        .objective_values
        .get(&label)
        .copied()
        .expect("value");

    assert!(
        (v_with - v_without).abs() < 1e-9,
        "profile must dominate posture on a colliding path; got with-posture={v_with}, baseline={v_without}",
    );
}

#[test]
fn manifest_replay_produces_identical_output_hash() {
    // The full round-trip: build a manifest, then re-execute the same
    // mode and compare the freshly computed output hash. Equivalent to
    // what `--verify` does on disk.
    use faultline_stats::manifest::{
        ManifestAssignment, ManifestMcConfig, ManifestMode, ManifestPosture,
    };

    let scenario = load_scenario();
    let config = small_config(pareto_postures(), true);
    let result_a = run_robustness(&scenario, &config).expect("first run");
    let hash_a = manifest::output_hash(&result_a).expect("hash a");

    let manifest_postures: Vec<ManifestPosture> = config
        .postures
        .iter()
        .map(|p| ManifestPosture {
            label: p.label.clone(),
            assignments: p
                .assignments
                .iter()
                .map(|a| ManifestAssignment {
                    path: a.path.clone(),
                    value: a.value,
                })
                .collect(),
        })
        .collect();
    let mode = ManifestMode::Robustness {
        objectives: config.objectives.iter().map(|o| o.label()).collect(),
        include_baseline: config.include_baseline,
        postures: manifest_postures,
        from_search_path: None,
        from_search_hash: None,
    };
    let mc = ManifestMcConfig::from_config(&config.mc_config, 7);
    let scenario_hash = manifest::scenario_hash(&scenario).expect("scenario hash");
    let m_a = manifest::build_manifest(
        "scenarios/defender_robustness_demo.toml".into(),
        scenario_hash.clone(),
        mc.clone(),
        mode.clone(),
        hash_a.clone(),
    )
    .expect("build manifest");

    // Re-execute exactly as the verify path would.
    let result_b = run_robustness(&scenario, &config).expect("replay run");
    let hash_b = manifest::output_hash(&result_b).expect("hash b");

    let m_b = manifest::build_manifest(
        "scenarios/defender_robustness_demo.toml".into(),
        scenario_hash,
        mc,
        mode,
        hash_b,
    )
    .expect("build manifest replay");

    assert_eq!(
        manifest::verify_manifest(&m_a, &m_b),
        manifest::VerifyResult::Match,
        "robustness replay must match the original manifest bit-for-bit"
    );
}
