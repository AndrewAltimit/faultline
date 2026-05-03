//! Scenario-library anti-drift and determinism regression tests.
//!
//! Two concerns live here:
//!
//!  1. **Schema anti-drift.** Every bundled `scenarios/*.toml` file
//!     must parse into `Scenario` and run to completion on the current
//!     engine. Before this test the only scenarios covered by
//!     integration tests were `tutorial_asymmetric` and
//!     `us_institutional_fracture`; everything else only ran via
//!     manual `cargo run` invocations, which meant the schema could
//!     drift out from under those files silently.
//!
//!  2. **Run-level determinism.** Given the same scenario TOML and
//!     the same seed, two independent `Engine::run()` calls must
//!     produce byte-identical serialized final snapshots. Faultline's
//!     headline guarantee is "same config + same seed = identical
//!     output"; this test locks it down against accidental
//!     `HashMap`, wall-clock, or unseeded-RNG regressions.
//!
//! Keep this file cheap to run — it already loads every bundled
//! scenario and runs each one to completion, so avoid adding
//! multi-thousand-tick scenarios to `scenarios/` without reviewing
//! the impact here.

use std::path::{Path, PathBuf};

use faultline_engine::Engine;
use faultline_types::scenario::Scenario;

/// All bundled scenarios. Keeping this list explicit (rather than
/// globbing the directory at test time) means a new scenario file
/// lands with an explicit test update and a reviewer sees it.
const BUNDLED_SCENARIOS: &[&str] = &[
    "alert_fatigue_soc.toml",
    "calibration_demo.toml",
    "capabilities_demo.toml",
    "coalition_fracture_demo.toml",
    "coevolution_demo.toml",
    "compound_kill_chains.toml",
    "defender_posture_optimization.toml",
    "defender_robustness_demo.toml",
    "drone_swarm_destabilization.toml",
    "europe_eastern_flank.toml",
    "europe_energy_sabotage.toml",
    "multifront_soc_escalation.toml",
    "narrative_competition_demo.toml",
    "network_resilience_demo.toml",
    "persistent_covert_surveillance.toml",
    "strategy_search_demo.toml",
    "supply_interdiction_demo.toml",
    "tutorial_asymmetric.toml",
    "tutorial_symmetric.toml",
    "us_institutional_fracture.toml",
];

fn scenarios_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios")
}

fn load_scenario(filename: &str) -> Scenario {
    let path = scenarios_dir().join(filename);
    let toml_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    toml::from_str(&toml_str).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

#[test]
fn bundled_scenarios_directory_matches_expected_list() {
    // Guard against a new scenario being added to scenarios/ without
    // also being added to BUNDLED_SCENARIOS. This is the guardrail
    // that makes the explicit-list approach safe.
    let mut on_disk: Vec<String> = std::fs::read_dir(scenarios_dir())
        .expect("scenarios dir should exist")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.ends_with(".toml") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    on_disk.sort();

    let mut expected: Vec<String> = BUNDLED_SCENARIOS.iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert_eq!(
        on_disk, expected,
        "scenarios/ directory does not match BUNDLED_SCENARIOS — add the new file to the list in tests/scenario_library.rs"
    );
}

#[test]
fn all_bundled_scenarios_parse_and_run_to_completion() {
    // Schema anti-drift: every bundled scenario must parse and run
    // without error on the current engine. A panic here means a
    // scenario file has drifted relative to the type definitions in
    // faultline-types or the engine's validation logic.
    for filename in BUNDLED_SCENARIOS {
        let scenario = load_scenario(filename);
        let mut engine = Engine::with_seed(scenario, 42).unwrap_or_else(|e| {
            panic!("Engine::with_seed failed for {filename}: {e:?}");
        });
        let run = engine
            .run()
            .unwrap_or_else(|e| panic!("engine.run() failed for {filename}: {e:?}"));

        // Basic sanity: we should have at least one tick and a
        // terminal snapshot.
        assert!(run.final_tick > 0, "{filename} terminated before tick 1");
        assert_eq!(
            run.final_state.tick, run.final_tick,
            "{filename} final_state.tick should match final_tick"
        );
    }
}

#[test]
fn run_is_deterministic_for_fixed_seed() {
    // Determinism regression: same scenario + same seed must produce
    // byte-identical serialized final snapshots on two independent
    // engine runs. Exercises tutorial_asymmetric (events + civilian
    // activation + fog of war), us_institutional_fracture (multi-
    // faction institutional dynamics), and compound_kill_chains
    // (Monte Carlo kill-chain resolution) to catch regressions in
    // different engine subsystems.
    let scenarios_to_check = [
        ("tutorial_asymmetric.toml", 42u64),
        ("us_institutional_fracture.toml", 20260412u64),
        ("compound_kill_chains.toml", 20260412u64),
    ];

    for (filename, seed) in scenarios_to_check {
        let scenario_a = load_scenario(filename);
        let scenario_b = load_scenario(filename);

        let mut engine_a = Engine::with_seed(scenario_a, seed)
            .unwrap_or_else(|e| panic!("engine A for {filename}: {e:?}"));
        let mut engine_b = Engine::with_seed(scenario_b, seed)
            .unwrap_or_else(|e| panic!("engine B for {filename}: {e:?}"));

        let run_a = engine_a
            .run()
            .unwrap_or_else(|e| panic!("run A for {filename}: {e:?}"));
        let run_b = engine_b
            .run()
            .unwrap_or_else(|e| panic!("run B for {filename}: {e:?}"));

        assert_eq!(
            run_a.final_tick, run_b.final_tick,
            "{filename}: two runs at seed {seed} diverged on final_tick ({} vs {})",
            run_a.final_tick, run_b.final_tick
        );

        // Serialize to JSON for a byte-comparable representation.
        // BTreeMap ordering makes serde_json output stable.
        let snap_a = serde_json::to_string(&run_a.final_state)
            .unwrap_or_else(|e| panic!("serialize A for {filename}: {e}"));
        let snap_b = serde_json::to_string(&run_b.final_state)
            .unwrap_or_else(|e| panic!("serialize B for {filename}: {e}"));

        assert_eq!(
            snap_a, snap_b,
            "{filename}: determinism violation — two runs at seed {seed} produced different final snapshots"
        );

        // Event log determinism catches event ordering regressions
        // that can slip past a final-state comparison when two
        // different event schedules converge on the same terminal
        // state.
        assert_eq!(
            run_a.event_log.len(),
            run_b.event_log.len(),
            "{filename}: event log length diverged at seed {seed}"
        );
        for (i, (a, b)) in run_a.event_log.iter().zip(&run_b.event_log).enumerate() {
            assert_eq!(
                a.tick, b.tick,
                "{filename}: event[{i}].tick diverged at seed {seed}"
            );
            assert_eq!(
                a.event_id, b.event_id,
                "{filename}: event[{i}].event_id diverged at seed {seed}"
            );
        }

        // Campaign reports are the primary analytical signal for
        // kill-chain scenarios; compare them directly.
        let campaigns_a = serde_json::to_string(&run_a.campaign_reports)
            .unwrap_or_else(|e| panic!("serialize campaigns A for {filename}: {e}"));
        let campaigns_b = serde_json::to_string(&run_b.campaign_reports)
            .unwrap_or_else(|e| panic!("serialize campaigns B for {filename}: {e}"));
        assert_eq!(
            campaigns_a, campaigns_b,
            "{filename}: campaign_reports diverged at seed {seed}"
        );
    }
}
