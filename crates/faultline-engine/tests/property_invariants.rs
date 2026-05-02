//! Property tests for `faultline_engine` (R3-5).
//!
//! Three invariants worth pinning per the May 2026 refresh:
//!
//! 1. **For any seed, no faction strength goes negative.** Force
//!    `strength` is clamped via `.max(0.0)` after every combat /
//!    attrition / damage update, and `FactionState.total_strength`
//!    is recomputed as a sum of those clamped values. A regression
//!    that dropped a clamp would immediately surface here under
//!    random seeds even on a tiny scenario.
//! 2. **For any seed, faction morale stays in `[0, 1]`.** Combat,
//!    leadership decapitation, casualty propagation, and event
//!    effects all `.clamp(0.0, 1.0)` morale updates. The closed-
//!    form combat math respects the bound, but a refactor that
//!    introduced a subtraction without the clamp would silently
//!    push morale negative.
//! 3. **Same `(scenario, seed)` produces bit-identical RunResult.**
//!    The seeded-RNG / `BTreeMap`-iteration determinism contract is
//!    the foundation of `--verify`, the manifest replay system, and
//!    every CI guard around it. The fixed-seed integration tests
//!    pin one seed; this property randomizes across many.
//!
//! The test loads the bundled `tutorial_symmetric.toml` scenario via
//! `include_str!` so it exercises a realistic, regression-relevant
//! engine path. To keep wall time manageable, the proptest budget is
//! tightened to 16 cases (the `max_ticks = 100` scenario takes a few
//! milliseconds per run; 16 × 4 properties stays under a second).

use faultline_engine::Engine;
use faultline_types::migration::load_scenario_str;
use faultline_types::scenario::Scenario;
use proptest::prelude::*;

/// Tutorial-symmetric scenario, loaded once and cloned per case. Picked
/// because (a) it's the smallest bundled scenario, (b) it produces a
/// definitive outcome under most seeds (one faction usually wins
/// before max_ticks), and (c) it covers combat, morale, and political
/// dynamics — the three invariants above all touch its code paths.
fn fixture_scenario() -> Scenario {
    let src = include_str!("../../../scenarios/tutorial_symmetric.toml");
    load_scenario_str(src)
        .expect("bundled tutorial_symmetric must load")
        .scenario
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        .. ProptestConfig::default()
    })]

    /// **Invariant: no faction strength goes negative across any tick
    /// for any seed.** This is the example invariant from the May 2026
    /// improvement-plan refresh.
    #[test]
    fn faction_strength_never_negative(seed in any::<u64>()) {
        let scenario = fixture_scenario();
        let mut engine = Engine::with_seed(scenario, seed)
            .expect("engine must construct");
        let result = engine.run().expect("engine must complete");

        // Final state.
        for (fid, state) in &result.final_state.faction_states {
            prop_assert!(
                state.total_strength >= 0.0,
                "{}: terminal total_strength {} < 0 for seed {}",
                fid,
                state.total_strength,
                seed
            );
            prop_assert!(
                state.total_strength.is_finite(),
                "{}: terminal total_strength non-finite for seed {}",
                fid,
                seed
            );
        }
        // Every snapshot in between (when collect_snapshots is on for
        // the bundled scenario, the run honors the snapshot_interval).
        // Assert non-empty so that if a future fixture change disables
        // snapshot collection (e.g. snapshot_interval = 0), the
        // intermediate-tick check doesn't silently degenerate to a
        // vacuous pass.
        prop_assert!(
            !result.snapshots.is_empty(),
            "expected snapshots from fixture for intermediate-tick checks"
        );
        for snap in &result.snapshots {
            for (fid, state) in &snap.faction_states {
                prop_assert!(
                    state.total_strength >= 0.0,
                    "{}: snapshot tick={} total_strength {} < 0 for seed {}",
                    fid,
                    snap.tick,
                    state.total_strength,
                    seed
                );
            }
        }
    }

    /// **Invariant: faction morale stays in `[0, 1]` for any seed.**
    /// All morale updates in the engine pass through a `.clamp(0.0, 1.0)`
    /// or equivalent; a regression that introduced an unclamped
    /// arithmetic update would surface here.
    #[test]
    fn faction_morale_in_unit_interval(seed in any::<u64>()) {
        let scenario = fixture_scenario();
        let mut engine = Engine::with_seed(scenario, seed)
            .expect("engine must construct");
        let result = engine.run().expect("engine must complete");

        for (fid, state) in &result.final_state.faction_states {
            prop_assert!(
                state.morale >= 0.0,
                "{}: terminal morale {} < 0 for seed {}",
                fid,
                state.morale,
                seed
            );
            prop_assert!(
                state.morale <= 1.0,
                "{}: terminal morale {} > 1 for seed {}",
                fid,
                state.morale,
                seed
            );
            prop_assert!(state.morale.is_finite());
        }
        for snap in &result.snapshots {
            for (fid, state) in &snap.faction_states {
                prop_assert!(
                    (0.0..=1.0).contains(&state.morale),
                    "{}: snapshot tick={} morale {} out of [0,1] for seed {}",
                    fid,
                    snap.tick,
                    state.morale,
                    seed
                );
            }
        }
    }

    /// **Invariant: tension stays in `[0, 1]` for any seed.** Every
    /// tension update site uses `.clamp(0.0, 1.0)` or one of the
    /// half-bounded `.min(1.0)` / `.max(0.0)` forms; the property
    /// guards against drift.
    #[test]
    fn tension_in_unit_interval(seed in any::<u64>()) {
        let scenario = fixture_scenario();
        let mut engine = Engine::with_seed(scenario, seed)
            .expect("engine must construct");
        let result = engine.run().expect("engine must complete");
        prop_assert!(
            (0.0..=1.0).contains(&result.final_state.tension),
            "terminal tension {} out of [0,1] for seed {}",
            result.final_state.tension,
            seed
        );
        for snap in &result.snapshots {
            prop_assert!(
                (0.0..=1.0).contains(&snap.tension),
                "snapshot tick={} tension {} out of [0,1] for seed {}",
                snap.tick,
                snap.tension,
                seed
            );
        }
    }

    /// **Invariant: same `(scenario, seed)` ⇒ bit-identical RunResult
    /// JSON.** This is the determinism contract `--verify` relies on.
    /// Two independent engine runs must produce byte-equal JSON output
    /// for the same seed.
    #[test]
    fn engine_is_deterministic_under_fixed_seed(seed in any::<u64>()) {
        let scenario = fixture_scenario();
        let mut e1 = Engine::with_seed(scenario.clone(), seed)
            .expect("engine 1 must construct");
        let mut e2 = Engine::with_seed(scenario, seed)
            .expect("engine 2 must construct");
        let r1 = e1.run().expect("engine 1 must complete");
        let r2 = e2.run().expect("engine 2 must complete");
        let j1 = serde_json::to_string(&r1).expect("serialize r1");
        let j2 = serde_json::to_string(&r2).expect("serialize r2");
        prop_assert_eq!(j1, j2);
    }
}
