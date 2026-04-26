//! End-to-end Epic K validation: defender capacity / queue dynamics.
//!
//! Drives the bundled `alert_fatigue_soc.toml` through the full Monte
//! Carlo runner and asserts the four invariants the Epic K design is
//! supposed to deliver:
//!
//! 1. Tier-1 saturates in the majority of runs (the noisy_enumeration
//!    phase actually achieves its design intent).
//! 2. The defender_queue_reports vector is populated and BTreeMap-
//!    ordered (faction, role) so downstream renderers / external
//!    citers see a deterministic shape.
//! 3. Cross-run aggregation in `compute_summary.defender_capacity`
//!    matches the per-run vector — same mean utilization, same
//!    saturated-runs count, same shadow-detection totals.
//! 4. Determinism: two `Engine::with_seed(_, S)` runs over the same
//!    scenario produce bit-identical defender_queue_reports — proving
//!    the new RNG consumer (Poisson noise sampling) is deterministic
//!    under the same seed.

use std::path::Path;

use faultline_engine::Engine;
use faultline_stats::MonteCarloRunner;
use faultline_types::stats::MonteCarloConfig;

/// Load the Epic K archetype scenario via the migration layer so the
/// test mirrors how the CLI loads it.
fn load_alert_fatigue_scenario() -> faultline_types::scenario::Scenario {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/alert_fatigue_soc.toml");
    let toml_str = std::fs::read_to_string(&path).expect("scenario file readable");
    let loaded =
        faultline_types::migration::load_scenario_str(&toml_str).expect("scenario loads cleanly");
    loaded.scenario
}

#[test]
fn alert_fatigue_scenario_saturates_tier1_in_majority_of_runs() {
    let scenario = load_alert_fatigue_scenario();
    let config = MonteCarloConfig {
        num_runs: 100,
        seed: Some(0xA1E47F47),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC run");
    let tier1 = result
        .summary
        .defender_capacity
        .iter()
        .find(|q| q.role.0 == "tier1_alerts")
        .expect("tier1_alerts row in summary");
    // Threshold is generous (60% of runs) — the design point is
    // ~85%, but RNG variance at N=100 can dip into the 70s. The
    // test pins the *mechanism* works, not a specific rate.
    assert!(
        tier1.time_to_saturation.saturated_runs >= 60,
        "expected majority of runs to saturate tier1, got {} of {}",
        tier1.time_to_saturation.saturated_runs,
        tier1.n_runs
    );
    // Mean shadow detections must be > 0 — that's the alert-fatigue
    // mechanism actually firing rather than the schema being passive.
    assert!(
        tier1.mean_shadow_detections > 0.0,
        "no shadow detections recorded — saturation gating not engaging"
    );
    // Tier-1 max utilization should hit 100% (queue at cap) in at
    // least one run.
    assert!(
        (tier1.max_utilization - 1.0).abs() < 0.01,
        "expected max utilization at 100% (queue hits cap), got {}",
        tier1.max_utilization
    );
}

#[test]
fn defender_queue_reports_are_btreemap_ordered() {
    let scenario = load_alert_fatigue_scenario();
    let mut engine = Engine::with_seed(scenario, 42).expect("engine");
    let result = engine.run().expect("run");
    assert_eq!(result.defender_queue_reports.len(), 2, "two roles declared");
    // Sorted by (faction, role) ascending.
    let r0 = &result.defender_queue_reports[0];
    let r1 = &result.defender_queue_reports[1];
    let key0 = (r0.faction.0.clone(), r0.role.0.clone());
    let key1 = (r1.faction.0.clone(), r1.role.0.clone());
    assert!(
        key0 < key1,
        "queue reports must be (faction, role) ascending: got {:?} then {:?}",
        key0,
        key1
    );
}

#[test]
fn summary_aggregation_matches_per_run_reports() {
    let scenario = load_alert_fatigue_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(0xCAFEBABE),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC run");
    // For each (faction, role) row in the summary, recompute mean
    // utilization from the underlying per-run reports and compare —
    // catches accidental skew (e.g. wrong divisor, dropped runs).
    for q_summary in &result.summary.defender_capacity {
        let runs_for_role: Vec<_> = result
            .runs
            .iter()
            .flat_map(|r| {
                r.defender_queue_reports
                    .iter()
                    .filter(|q| q.faction == q_summary.faction && q.role == q_summary.role)
            })
            .collect();
        assert_eq!(
            runs_for_role.len() as u32,
            q_summary.n_runs,
            "summary n_runs must equal count of per-run reports for ({}, {})",
            q_summary.faction,
            q_summary.role
        );
        let recomputed_mean: f64 =
            runs_for_role.iter().map(|r| r.utilization).sum::<f64>() / runs_for_role.len() as f64;
        assert!(
            (recomputed_mean - q_summary.mean_utilization).abs() < 1e-9,
            "mean_utilization drift for ({}, {}): {} vs {}",
            q_summary.faction,
            q_summary.role,
            recomputed_mean,
            q_summary.mean_utilization
        );
        let recomputed_sat: u32 = runs_for_role
            .iter()
            .filter(|r| r.time_to_saturation.is_some())
            .count() as u32;
        assert_eq!(
            recomputed_sat, q_summary.time_to_saturation.saturated_runs,
            "saturated_runs drift for ({}, {})",
            q_summary.faction, q_summary.role
        );
    }
}

#[test]
fn determinism_same_seed_produces_identical_queue_reports() {
    // The new RNG consumer (Poisson noise sampling) must preserve
    // the determinism contract. Two engines with the same seed must
    // produce bit-identical defender_queue_reports.
    let scenario = load_alert_fatigue_scenario();
    let mut e1 = Engine::with_seed(scenario.clone(), 17).expect("engine 1");
    let mut e2 = Engine::with_seed(scenario, 17).expect("engine 2");
    let r1 = e1.run().expect("run 1");
    let r2 = e2.run().expect("run 2");
    assert_eq!(
        r1.defender_queue_reports.len(),
        r2.defender_queue_reports.len()
    );
    for (q1, q2) in r1
        .defender_queue_reports
        .iter()
        .zip(r2.defender_queue_reports.iter())
    {
        assert_eq!(q1.faction, q2.faction);
        assert_eq!(q1.role, q2.role);
        assert_eq!(q1.final_depth, q2.final_depth);
        assert_eq!(q1.max_depth, q2.max_depth);
        assert_eq!(q1.total_enqueued, q2.total_enqueued);
        assert_eq!(q1.total_serviced, q2.total_serviced);
        assert_eq!(q1.total_dropped, q2.total_dropped);
        assert_eq!(q1.shadow_detections, q2.shadow_detections);
        assert_eq!(q1.time_to_saturation, q2.time_to_saturation);
        assert!((q1.mean_depth - q2.mean_depth).abs() < f64::EPSILON);
        assert!((q1.utilization - q2.utilization).abs() < f64::EPSILON);
    }
}

#[test]
fn no_capacity_scenario_produces_empty_queue_summary() {
    // Negative control: a scenario without `defender_capacities` must
    // produce an empty defender_capacity vector — proving the legacy
    // hot path is unaffected.
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/tutorial_symmetric.toml");
    let toml_str = std::fs::read_to_string(&path).expect("scenario file readable");
    let scenario = faultline_types::migration::load_scenario_str(&toml_str)
        .expect("scenario loads")
        .scenario;
    let config = MonteCarloConfig {
        num_runs: 5,
        seed: Some(1),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC run");
    assert!(
        result.summary.defender_capacity.is_empty(),
        "scenario without defender_capacities must produce empty summary row"
    );
    for run in &result.runs {
        assert!(
            run.defender_queue_reports.is_empty(),
            "per-run queue reports must be empty too"
        );
    }
    // And cross-version JSON shape: the field must be elided when
    // empty (#[serde(default, skip_serializing_if = "Vec::is_empty")])
    // so legacy tooling reading the summary doesn't see a noise key.
    let json = serde_json::to_string(&result.summary).expect("serialize summary");
    assert!(
        !json.contains("\"defender_capacity\""),
        "empty defender_capacity must be elided from summary JSON"
    );
}

#[test]
fn validate_scenario_rejects_unknown_defender_role_reference() {
    // A kill-chain phase that names an undeclared (faction, role)
    // must be rejected at load time. Catches author typos that
    // would otherwise silently no-op (no gating, no enqueue).
    let scenario = load_alert_fatigue_scenario();
    let mut bad = scenario.clone();
    // Pick the first phase and rewrite gated_by_defender to point at
    // a role nobody declared.
    let chain = bad
        .kill_chains
        .values_mut()
        .next()
        .expect("scenario has a kill chain");
    let phase = chain
        .phases
        .values_mut()
        .next()
        .expect("kill chain has a phase");
    phase.gated_by_defender = Some(faultline_types::campaign::DefenderRoleRef {
        faction: faultline_types::ids::FactionId::from("blue_soc"),
        role: faultline_types::ids::DefenderRoleId::from("nonexistent_role"),
    });
    let err = faultline_engine::validate_scenario(&bad)
        .expect_err("validate must reject unknown role reference");
    let msg = format!("{err}");
    assert!(
        msg.contains("unknown defender role") || msg.contains("nonexistent_role"),
        "unhelpful error message: {msg}"
    );
}
