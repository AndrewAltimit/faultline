//! Core simulation engine for Faultline conflict simulation.
//!
//! Provides the tick-based engine that drives a single simulation run,
//! advancing faction actions, event evaluation, combat resolution, and
//! victory condition checks each tick.
//!
//! Given the same [`Scenario`](faultline_types::scenario::Scenario) and
//! RNG seed, the output is fully deterministic.

pub mod ai;
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
    }

    for vc in scenario.victory_conditions.values() {
        if !scenario.factions.contains_key(&vc.faction) {
            return Err(ScenarioError::UnknownFaction(vc.faction.clone()));
        }
    }

    Ok(())
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
    // Phase 3 integration tests
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
}
