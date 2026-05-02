//! Integration tests for adversarial co-evolution against the bundled
//! `coevolution_demo` scenario.
//!
//! Each round runs `trials * inner_runs` engine passes plus one final
//! joint-evaluation Monte Carlo batch, so the per-test budget scales as
//! `rounds * trials * inner_runs`. Inner counts are kept tiny
//! (4 trials × 8 runs) — the contracts being tested are determinism +
//! convergence behaviour, not the underlying simulator's convergence.

use faultline_stats::coevolve::{
    CoevolveConfig, CoevolveSide, CoevolveSideConfig, CoevolveStatus, run_coevolution,
};
use faultline_stats::manifest;
use faultline_stats::search::SearchMethod;
use faultline_types::ids::FactionId;
use faultline_types::migration::load_scenario_str;
use faultline_types::stats::MonteCarloConfig;
use faultline_types::strategy_space::SearchObjective;

const DEMO_SCENARIO: &str = include_str!("../../../scenarios/coevolution_demo.toml");

fn load_demo() -> faultline_types::scenario::Scenario {
    load_scenario_str(DEMO_SCENARIO)
        .expect("bundled coevolution_demo must load")
        .scenario
}

fn small_coevolve_config() -> CoevolveConfig {
    CoevolveConfig {
        max_rounds: 6,
        initial_mover: CoevolveSide::Defender,
        attacker: CoevolveSideConfig {
            faction: FactionId::from("red"),
            objective: SearchObjective::MaximizeWinRate {
                faction: FactionId::from("red"),
            },
            method: SearchMethod::Grid,
            trials: 4,
        },
        defender: CoevolveSideConfig {
            faction: FactionId::from("blue"),
            objective: SearchObjective::MinimizeMaxChainSuccess,
            method: SearchMethod::Grid,
            trials: 4,
        },
        mc_config: MonteCarloConfig {
            num_runs: 8,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        },
        coevolve_seed: 1,
        assignment_tolerance: 1e-9,
    }
}

#[test]
fn coevolve_emits_stable_output_hash_under_fixed_seeds() {
    // The manifest-replay precondition for `--coevolve --verify`: two
    // consecutive runs against identical inputs must produce identical
    // CoevolveResult hashes. Without this, the co-evolution manifest
    // mode would silently drift across replays.
    let scenario = load_demo();
    let config = small_coevolve_config();

    let r1 = run_coevolution(&scenario, &config).expect("first coevolve");
    let r2 = run_coevolution(&scenario, &config).expect("second coevolve");

    let h1 = manifest::output_hash(&r1).expect("hash r1");
    let h2 = manifest::output_hash(&r2).expect("hash r2");
    assert_eq!(
        h1, h2,
        "co-evolve output hash must be stable across re-runs of the same config"
    );
}

#[test]
fn coevolve_demo_converges() {
    // The bundled demo's grid is small enough that a 6-round budget is
    // sufficient to reach a Nash-style equilibrium. If this test starts
    // failing — e.g. the engine's Lanchester model gains structure that
    // creates oscillation around this scenario's parameters — the right
    // fix is to expand the demo scenario rather than relax the assertion;
    // the bundled scenario's whole purpose is to demonstrate the
    // converged path end-to-end.
    let scenario = load_demo();
    let config = small_coevolve_config();
    let r = run_coevolution(&scenario, &config).expect("coevolve");
    assert!(
        matches!(r.status, CoevolveStatus::Converged),
        "demo scenario must converge with the small_coevolve_config grid; got {:?}",
        r.status
    );
    assert!(
        r.rounds.len() <= config.max_rounds as usize,
        "round count must respect max_rounds budget"
    );
    assert!(
        !r.rounds.is_empty(),
        "every coevolve run produces at least one round"
    );
}

#[test]
fn coevolve_demo_is_mc_seed_stable() {
    // Regression check on the bundled `coevolution_demo` scenario:
    // its objective landscape is steep enough that one assignment
    // dominates each round's grid by a margin wider than the MC
    // sampling noise of the small inner-run budget used here, so
    // changing `mc_config.seed` must not flip the selected best.
    //
    // This is *not* a general architectural invariant — `best_by_
    // objective` selects on MC-evaluated objective values, and on a
    // noisier scenario two MC seeds could rank grid cells
    // differently and produce different `mover_assignments`. The
    // architectural piece (the search-seed-driven *visited* trial
    // grid is identical across MC seeds) is exercised by unit tests
    // in `crates/faultline-stats/src/coevolve.rs`. This integration
    // test exists to catch regressions in the demo scenario's
    // landscape — if the engine gains structure that flattens the
    // demo's dominance margin, the right fix is to expand the demo
    // until one assignment dominates again, not to relax this
    // assertion.
    let scenario = load_demo();
    let mut a = small_coevolve_config();
    let mut b = small_coevolve_config();
    a.mc_config.seed = Some(1);
    b.mc_config.seed = Some(2);
    let ra = run_coevolution(&scenario, &a).expect("a");
    let rb = run_coevolution(&scenario, &b).expect("b");
    assert_eq!(
        ra.rounds.len(),
        rb.rounds.len(),
        "demo scenario: round count stable across MC seeds (one assignment dominates each round)"
    );
    assert_eq!(
        ra.status, rb.status,
        "demo scenario: termination status stable across MC seeds"
    );
    for (a_round, b_round) in ra.rounds.iter().zip(rb.rounds.iter()) {
        let path_pairs_a: Vec<_> = a_round
            .mover_assignments
            .iter()
            .map(|o| (o.path.clone(), o.value))
            .collect();
        let path_pairs_b: Vec<_> = b_round
            .mover_assignments
            .iter()
            .map(|o| (o.path.clone(), o.value))
            .collect();
        assert_eq!(
            path_pairs_a, path_pairs_b,
            "demo scenario round {}: mover_assignments stable across MC seeds (dominant landscape)",
            a_round.round
        );
    }
}

#[test]
fn coevolve_objective_appears_in_final_values() {
    // The final_objective_values map must carry both sides' objective
    // labels — the report's "Equilibrium objective values" table reads
    // it directly. A missing entry would render an empty table and
    // mask a regression.
    let scenario = load_demo();
    let config = small_coevolve_config();
    let r = run_coevolution(&scenario, &config).expect("coevolve");

    let attacker_label = config.attacker.objective.label();
    let defender_label = config.defender.objective.label();
    assert!(
        r.final_objective_values.contains_key(&attacker_label),
        "final_objective_values missing attacker objective `{attacker_label}`"
    );
    assert!(
        r.final_objective_values.contains_key(&defender_label),
        "final_objective_values missing defender objective `{defender_label}`"
    );
}

#[test]
fn coevolve_initial_mover_picks_first_round() {
    // Smoke-check the dispatch: round 1's mover must equal
    // `initial_mover`, regardless of which side it is. Catches a
    // regression in the alternating-mover boolean.
    let scenario = load_demo();
    for mover in [CoevolveSide::Attacker, CoevolveSide::Defender] {
        let mut cfg = small_coevolve_config();
        cfg.initial_mover = mover;
        let r = run_coevolution(&scenario, &cfg).expect("coevolve");
        assert_eq!(
            r.rounds[0].mover, mover,
            "round 1 must equal initial_mover {mover:?}"
        );
        // And round 2 (if reached) must be the opposite side.
        if r.rounds.len() >= 2 {
            assert_eq!(r.rounds[1].mover, mover.other(), "round 2 must alternate");
        }
    }
}

#[test]
fn coevolve_final_assignments_match_last_per_side_round() {
    // After the loop, the result's `final_*_assignments` must reflect
    // each side's most recent move — not, e.g., a stale baseline. The
    // simplest check: scan rounds backward and verify the most-recent
    // move of each side matches the recorded final.
    let scenario = load_demo();
    let config = small_coevolve_config();
    let r = run_coevolution(&scenario, &config).expect("coevolve");

    let last_attacker = r
        .rounds
        .iter()
        .rev()
        .find(|round| round.mover == CoevolveSide::Attacker);
    if let Some(last) = last_attacker {
        assert_eq!(
            last.mover_assignments, r.final_attacker_assignments,
            "final_attacker_assignments must match the most-recent attacker move"
        );
    }

    let last_defender = r
        .rounds
        .iter()
        .rev()
        .find(|round| round.mover == CoevolveSide::Defender);
    if let Some(last) = last_defender {
        assert_eq!(
            last.mover_assignments, r.final_defender_assignments,
            "final_defender_assignments must match the most-recent defender move"
        );
    }
}
