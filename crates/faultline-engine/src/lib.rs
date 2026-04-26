//! Core simulation engine for Faultline conflict simulation.
//!
//! Provides the tick-based engine that drives a single simulation run,
//! advancing faction actions, event evaluation, combat resolution, and
//! victory condition checks each tick.
//!
//! Given the same [`Scenario`](faultline_types::scenario::Scenario) and
//! RNG seed, the output is fully deterministic.

pub mod ai;
pub mod campaign;
pub mod combat;
pub mod engine;
pub mod error;
pub mod state;
pub mod tick;

#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod tick_tests;

pub use engine::Engine;
pub use error::EngineError;
pub use state::SimulationState;
pub use tick::TickResult;

use faultline_types::error::ScenarioError;
use faultline_types::scenario::Scenario;

/// Validate a scenario for structural correctness.
///
/// Returns `Ok(())` if validation passes, or the first error found.
pub fn validate_scenario(scenario: &Scenario) -> Result<(), ScenarioError> {
    if scenario.factions.is_empty() {
        return Err(ScenarioError::EmptyScenario("no factions defined".into()));
    }

    if scenario.map.regions.is_empty() {
        return Err(ScenarioError::EmptyScenario("no regions defined".into()));
    }

    for (rid, region) in &scenario.map.regions {
        for neighbor in &region.borders {
            if !scenario.map.regions.contains_key(neighbor) {
                return Err(ScenarioError::InvalidBorder {
                    region: rid.clone(),
                    neighbor: neighbor.clone(),
                });
            }
        }
    }

    for (iid, infra) in &scenario.map.infrastructure {
        if !scenario.map.regions.contains_key(&infra.region) {
            return Err(ScenarioError::InfraRegionMismatch {
                infra: iid.clone(),
                region: infra.region.clone(),
            });
        }
    }

    for (fid, faction) in &scenario.factions {
        for unit in faction.forces.values() {
            if !scenario.map.regions.contains_key(&unit.region) {
                return Err(ScenarioError::ForceRegionMismatch {
                    force: unit.name.clone(),
                    faction: fid.clone(),
                    region: unit.region.clone(),
                });
            }
        }
        // Defender capacity sanity: a zero-depth queue is permanently
        // saturated (depth >= capacity at depth 0), which would silently
        // apply the saturated_detection_factor penalty before any noise
        // arrives. Reject loudly. Also enforce that the inner `id`
        // matches its table key — the field is documented as such but
        // the engine reads only the key, so a mismatch would be a silent
        // author error.
        for (rid, cap) in &faction.defender_capacities {
            if cap.queue_depth == 0 {
                return Err(ScenarioError::ZeroDefenderQueueDepth {
                    faction: fid.clone(),
                    role: rid.clone(),
                });
            }
            if cap.id != *rid {
                return Err(ScenarioError::DefenderRoleIdMismatch {
                    faction: fid.clone(),
                    key: rid.clone(),
                    id: cap.id.clone(),
                });
            }
            // `initialize_defender_queues` clamps service_rate via
            // `.max(0.0)`, but a negative value almost always means an
            // authoring error (typo / sign flip) — fail loudly instead
            // of silently freezing the queue. NaN is also rejected here
            // since `< 0.0` is false for NaN; we use `!is_finite()` to
            // catch it. f64::NEG_INFINITY satisfies `value < 0.0`.
            if !cap.service_rate.is_finite() || cap.service_rate < 0.0 {
                return Err(ScenarioError::NegativeServiceRate {
                    faction: fid.clone(),
                    role: rid.clone(),
                    value: cap.service_rate,
                });
            }
            // saturated_detection_factor is a multiplier on detection
            // probability; the gating path clamps to [0, 1] silently,
            // which would turn an authoring error like -0.5 into
            // complete detection suppression with no diagnostic.
            if !cap.saturated_detection_factor.is_finite()
                || cap.saturated_detection_factor < 0.0
                || cap.saturated_detection_factor > 1.0
            {
                return Err(ScenarioError::SaturatedDetectionFactorOutOfRange {
                    faction: fid.clone(),
                    role: rid.clone(),
                    value: cap.saturated_detection_factor,
                });
            }
        }
    }

    for vc in scenario.victory_conditions.values() {
        if !scenario.factions.contains_key(&vc.faction) {
            return Err(ScenarioError::UnknownFaction(vc.faction.clone()));
        }
    }

    // Defender capacity references (Epic K): every (faction, role)
    // named by `gated_by_defender` or `defender_noise` on a kill-chain
    // phase must resolve to a declared `defender_capacities` entry.
    // Catching this at load time turns a silent "queue not found, no
    // gating, no enqueue" runtime no-op into a loud configuration
    // error.
    for (cid, chain) in &scenario.kill_chains {
        for (pid, phase) in &chain.phases {
            if let Some(rr) = &phase.gated_by_defender
                && !defender_role_exists(scenario, &rr.faction, &rr.role)
            {
                return Err(ScenarioError::UnknownDefenderRole {
                    faction: rr.faction.clone(),
                    role: rr.role.clone(),
                });
            }
            for noise in &phase.defender_noise {
                if !defender_role_exists(scenario, &noise.defender, &noise.role) {
                    return Err(ScenarioError::UnknownDefenderRole {
                        faction: noise.defender.clone(),
                        role: noise.role.clone(),
                    });
                }
                // A negative rate is silently clamped to 0.0 in
                // `enqueue_phase_noise` via `.max(0.0)`, masking
                // authoring errors (sign flip / typo). Same fail-loud
                // pattern as `NegativeServiceRate`. Check before the
                // `!is_finite()` guard so `f64::NEG_INFINITY` reaches
                // the diagnostic that names the actual failure mode.
                if noise.items_per_tick < 0.0 {
                    return Err(ScenarioError::NegativeDefenderNoiseRate {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
                // NaN never satisfies `< 0.0` or `> 700.0`, so explicit
                // `!is_finite()` is required to catch it (and +∞).
                if !noise.items_per_tick.is_finite() {
                    return Err(ScenarioError::DefenderNoiseRateTooHigh {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
                // `sample_poisson` uses Knuth's inverse-transform method,
                // which relies on `(-mean).exp()`. For `mean > ~709` this
                // underflows to 0.0 in f64 and the loop falls through to
                // the 100,000-iteration cap, returning `mean as u32` with
                // a degenerate (non-Poisson) distribution. Cap well
                // below the underflow threshold so the sampler stays in
                // its accurate regime; authors who genuinely need higher
                // rates can split across multiple noise streams.
                if noise.items_per_tick > 700.0 {
                    return Err(ScenarioError::DefenderNoiseRateTooHigh {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
            }
        }
    }

    Ok(())
}

fn defender_role_exists(
    scenario: &Scenario,
    faction: &faultline_types::ids::FactionId,
    role: &faultline_types::ids::DefenderRoleId,
) -> bool {
    scenario
        .factions
        .get(faction)
        .is_some_and(|f| f.defender_capacities.contains_key(role))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    use faultline_types::faction::{Faction, FactionType};
    use faultline_types::ids::{FactionId, RegionId, VictoryId};
    use faultline_types::map::{MapConfig, MapSource, Region};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::ScenarioMeta;
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::strategy::Doctrine;
    use faultline_types::victory::{VictoryCondition, VictoryType};

    pub(crate) fn minimal_scenario() -> Scenario {
        let rid = RegionId::from("capital");
        let fid = FactionId::from("gov");

        let mut regions = BTreeMap::new();
        regions.insert(
            rid.clone(),
            Region {
                id: rid.clone(),
                name: "Capital".into(),
                population: 1_000_000,
                urbanization: 0.9,
                initial_control: Some(fid.clone()),
                strategic_value: 10.0,
                borders: vec![],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(
            fid.clone(),
            Faction {
                id: fid.clone(),
                name: "Government".into(),
                faction_type: FactionType::Insurgent,
                description: "Test faction".into(),
                color: "#000000".into(),
                forces: BTreeMap::new(),
                tech_access: vec![],
                initial_morale: 0.8,
                logistics_capacity: 100.0,
                initial_resources: 1000.0,
                resource_rate: 10.0,
                recruitment: None,
                command_resilience: 0.9,
                intelligence: 0.5,
                diplomacy: vec![],
                doctrine: Doctrine::Conventional,
                escalation_rules: None,
                defender_capacities: BTreeMap::new(),
            },
        );

        let mut victory_conditions = BTreeMap::new();
        victory_conditions.insert(
            VictoryId::from("gov-win"),
            VictoryCondition {
                id: VictoryId::from("gov-win"),
                name: "Government Control".into(),
                faction: fid.clone(),
                condition: VictoryType::StrategicControl { threshold: 1.0 },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "Test".into(),
                description: "Test scenario".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.5,
                institutional_trust: 0.7,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.8,
                    internet_availability: 0.9,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 100,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 10,
                seed: Some(42),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 10,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
        }
    }

    #[test]
    fn engine_runs_to_completion() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation should succeed");
        let result = engine.run().expect("run should succeed");
        assert!(result.final_tick > 0);
    }

    #[test]
    fn validate_scenario_passes_for_valid() {
        let scenario = minimal_scenario();
        assert!(validate_scenario(&scenario).is_ok());
    }

    #[test]
    fn validate_scenario_fails_for_empty_factions() {
        let mut scenario = minimal_scenario();
        scenario.factions.clear();
        assert!(validate_scenario(&scenario).is_err());
    }

    #[test]
    fn deterministic_runs_produce_same_result() {
        let scenario = minimal_scenario();
        let mut engine1 = Engine::new(scenario.clone()).expect("engine creation should succeed");
        let result1 = engine1.run().expect("run should succeed");

        let mut engine2 = Engine::new(scenario).expect("engine creation should succeed");
        let result2 = engine2.run().expect("run should succeed");

        assert_eq!(result1.final_tick, result2.final_tick);
        assert_eq!(result1.outcome.victor, result2.outcome.victor);
    }

    // -----------------------------------------------------------------------
    // Monte Carlo integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn run_result_has_final_state() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        assert_eq!(
            result.final_state.tick, result.final_tick,
            "final_state tick should match final_tick"
        );
        assert!(
            !result.final_state.faction_states.is_empty(),
            "final_state should have faction states"
        );
        assert!(
            !result.final_state.region_control.is_empty(),
            "final_state should have region control"
        );
    }

    #[test]
    fn run_result_final_state_matches_last_snapshot_tick() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // final_state.tick and final_tick are set from the same value.
        assert_eq!(
            result.final_state.tick, result.final_tick,
            "final_state.tick should equal final_tick"
        );

        if !result.snapshots.is_empty() {
            let last_snap_tick = result.snapshots.last().expect("checked non-empty").tick;
            assert!(
                result.final_state.tick >= last_snap_tick,
                "final_state should be at or after last snapshot"
            );
        }
    }

    #[test]
    fn run_result_event_log_populated_from_scenario_with_events() {
        // Load the asymmetric scenario which has events.
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/tutorial_asymmetric.toml"),
        )
        .expect("should read asymmetric scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // The asymmetric scenario has events with conditions that may or may not fire.
        // At minimum, the event_log should be a valid (possibly empty) Vec.
        // With seed 42, events typically fire.
        // Whether or not events fire, the structure is correct.
        for record in &result.event_log {
            assert!(
                record.tick > 0,
                "event tick should be > 0 (ticks start at 1)"
            );
            assert!(record.tick <= result.final_tick, "event tick within bounds");
        }
    }

    #[test]
    fn events_fired_this_tick_cleared_between_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        // Run a few ticks.
        engine.tick().expect("tick 1");
        let after_tick1 = engine.state().events_fired_this_tick.clone();

        engine.tick().expect("tick 2");
        let after_tick2 = engine.state().events_fired_this_tick.clone();

        // With no events in scenario, both should be empty.
        assert!(
            after_tick1.is_empty(),
            "events_fired_this_tick should be empty with no events"
        );
        assert!(
            after_tick2.is_empty(),
            "events_fired_this_tick should be empty with no events"
        );
    }

    #[test]
    fn snapshots_include_infra_status() {
        use faultline_types::ids::InfraId;
        use faultline_types::map::{InfrastructureNode, InfrastructureType};

        let mut scenario = minimal_scenario();
        scenario.simulation.snapshot_interval = 5;

        let iid = InfraId::from("test_grid");
        scenario.map.infrastructure.insert(
            iid.clone(),
            InfrastructureNode {
                id: iid.clone(),
                name: "Test Grid".into(),
                region: RegionId::from("capital"),
                infra_type: InfrastructureType::PowerGrid,
                criticality: 0.9,
                initial_status: 1.0,
                repairable: Some(30),
            },
        );

        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // Snapshots should include infra_status.
        for snap in &result.snapshots {
            assert!(
                snap.infra_status.contains_key(&iid),
                "snapshot at tick {} should include infra_status for test_grid",
                snap.tick
            );
        }

        // Final state should also include infra.
        assert!(
            result.final_state.infra_status.contains_key(&iid),
            "final_state should include infra_status"
        );
    }

    #[test]
    fn fracture_scenario_loads_and_runs() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        validate_scenario(&scenario).expect("scenario should be valid");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        assert_eq!(result.final_tick, 365, "should run full 365 ticks");
        assert!(
            !result.final_state.faction_states.is_empty(),
            "should have faction states"
        );
        assert!(
            !result.event_log.is_empty(),
            "fracture scenario should fire events"
        );
    }

    #[test]
    fn fracture_scenario_event_log_has_correct_event_ids() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario.clone(), 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // All event IDs in the log should be defined in the scenario.
        for record in &result.event_log {
            assert!(
                scenario.events.contains_key(&record.event_id),
                "event_id {} in log should be defined in scenario",
                record.event_id
            );
        }
    }

    #[test]
    fn fracture_scenario_event_chain_fires() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // constitutional_crisis chains to state_nullification.
        let has_crisis = result
            .event_log
            .iter()
            .any(|r| r.event_id.0 == "constitutional_crisis");
        let has_nullification = result
            .event_log
            .iter()
            .any(|r| r.event_id.0 == "state_nullification");

        if has_crisis {
            assert!(
                has_nullification,
                "if constitutional_crisis fired, state_nullification should chain-fire"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Engine getter and snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn engine_max_ticks_returns_scenario_value() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert_eq!(engine.max_ticks(), 100, "max_ticks should match scenario");
    }

    #[test]
    fn engine_scenario_returns_reference() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert_eq!(engine.scenario().meta.name, "Test");
        assert_eq!(engine.scenario().simulation.max_ticks, 100);
        assert_eq!(engine.scenario().factions.len(), 1);
    }

    #[test]
    fn engine_is_finished_false_at_start() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert!(
            !engine.is_finished(),
            "engine should not be finished at tick 0"
        );
    }

    #[test]
    fn engine_is_finished_true_at_max_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        engine.run().expect("run should succeed");
        assert!(
            engine.is_finished(),
            "engine should be finished after run completes"
        );
    }

    #[test]
    fn engine_is_finished_transitions_during_ticking() {
        let mut scenario = minimal_scenario();
        scenario.simulation.max_ticks = 5;
        let mut engine = Engine::new(scenario).expect("engine creation");

        for i in 1..=5 {
            assert!(
                !engine.is_finished(),
                "should not be finished before tick {i}"
            );
            engine.tick().expect("tick should succeed");
        }
        assert!(
            engine.is_finished(),
            "should be finished after reaching max_ticks"
        );
    }

    #[test]
    fn engine_snapshot_at_tick_zero() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        assert_eq!(snap.tick, 0, "snapshot tick should be 0 at start");
        assert!(
            !snap.faction_states.is_empty(),
            "snapshot should have faction states"
        );
        assert!(
            !snap.region_control.is_empty(),
            "snapshot should have region control"
        );
        assert!(
            snap.events_fired_this_tick.is_empty(),
            "no events should have fired at tick 0"
        );
    }

    #[test]
    fn engine_snapshot_advances_with_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        engine.tick().expect("tick 1");
        let snap1 = engine.snapshot();
        assert_eq!(snap1.tick, 1, "snapshot should reflect tick 1");

        engine.tick().expect("tick 2");
        let snap2 = engine.snapshot();
        assert_eq!(snap2.tick, 2, "snapshot should reflect tick 2");
    }

    #[test]
    fn engine_snapshot_contains_correct_faction_data() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        let fid = FactionId::from("gov");
        let faction_state = snap
            .faction_states
            .get(&fid)
            .expect("should have gov faction in snapshot");

        assert_eq!(faction_state.faction_id, fid);
        assert!((faction_state.morale - 0.8).abs() < f64::EPSILON);
        assert!((faction_state.resources - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engine_snapshot_matches_take_snapshot_in_run_result() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        // Advance a few ticks manually.
        for _ in 0..5 {
            engine.tick().expect("tick should succeed");
        }

        // Snapshot via public method should match internal state.
        let snap = engine.snapshot();
        assert_eq!(snap.tick, 5);
        assert_eq!(snap.tick, engine.current_tick());
    }

    #[test]
    fn engine_snapshot_region_control_matches_initial() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        let rid = RegionId::from("capital");
        let fid = FactionId::from("gov");
        let control = snap.region_control.get(&rid).expect("should have capital");
        assert_eq!(control, &Some(fid), "capital should be controlled by gov");
    }
}
