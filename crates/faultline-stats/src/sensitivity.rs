//! Sensitivity analysis: sweep one parameter across a range and observe
//! how Monte Carlo outcomes change.
//!
//! The `set_param` / `get_param` path layer is also used by
//! `--counterfactual <path>=<value>` in the CLI: Epic B's counterfactual
//! mode patches the same dotted paths documented here, runs a Monte
//! Carlo batch against the patched scenario, and compares the result
//! to the baseline.

use tracing::info;

use faultline_types::ids::{KillChainId, PhaseId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, SensitivityResult};

use crate::{MonteCarloRunner, StatsError};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run sensitivity analysis by varying a single parameter across a range.
///
/// For each value in the sweep, a full Monte Carlo batch is executed and the
/// resulting [`MonteCarloSummary`] is stored in [`SensitivityResult::outcomes`].
pub fn run_sensitivity(
    base_scenario: &Scenario,
    config: &MonteCarloConfig,
    param: &str,
    low: f64,
    high: f64,
    steps: u32,
) -> Result<SensitivityResult, StatsError> {
    if steps == 0 {
        return Err(StatsError::InvalidConfig(
            "sensitivity steps must be > 0".into(),
        ));
    }
    if low > high {
        return Err(StatsError::InvalidConfig(
            "sensitivity low must be <= high".into(),
        ));
    }

    let baseline_value = get_param(base_scenario, param)?;

    let varied_values: Vec<f64> = if steps == 1 {
        vec![low]
    } else {
        (0..steps)
            .map(|i| low + (high - low) * f64::from(i) / f64::from(steps - 1))
            .collect()
    };

    let mut outcomes = Vec::with_capacity(varied_values.len());

    for (i, &value) in varied_values.iter().enumerate() {
        info!(
            step = i + 1,
            total = steps,
            param,
            value,
            "sensitivity sweep"
        );

        let mut scenario = base_scenario.clone();
        set_param(&mut scenario, param, value)?;

        let result = MonteCarloRunner::run(config, &scenario)?;
        outcomes.push(result.summary);
    }

    Ok(SensitivityResult {
        parameter: param.to_string(),
        baseline_value,
        varied_values,
        outcomes,
    })
}

// ---------------------------------------------------------------------------
// Parameter access
// ---------------------------------------------------------------------------

/// Supported parameter paths for sensitivity analysis and
/// `--counterfactual` overrides.
///
/// Format: `<section>.<id>[.<sub>...].<field>`
///
/// Supported:
/// - `faction.<faction_id>.initial_morale`
/// - `faction.<faction_id>.initial_resources`
/// - `faction.<faction_id>.resource_rate`
/// - `faction.<faction_id>.logistics_capacity`
/// - `faction.<faction_id>.command_resilience`
/// - `faction.<faction_id>.intelligence`
/// - `political_climate.tension`
/// - `political_climate.institutional_trust`
/// - `political_climate.media.disinformation_susceptibility`
/// - `political_climate.media.state_control`
/// - `kill_chain.<chain_id>.phase.<phase_id>.base_success_probability`
/// - `kill_chain.<chain_id>.phase.<phase_id>.detection_probability_per_tick`
/// - `kill_chain.<chain_id>.phase.<phase_id>.attribution_difficulty`
/// - `kill_chain.<chain_id>.phase.<phase_id>.prerequisite_success_boost`
/// - `kill_chain.<chain_id>.phase.<phase_id>.cost.attacker_dollars`
/// - `kill_chain.<chain_id>.phase.<phase_id>.cost.defender_dollars`
pub fn get_param(scenario: &Scenario, param: &str) -> Result<f64, StatsError> {
    let parts: Vec<&str> = param.split('.').collect();

    match parts.as_slice() {
        ["faction", faction_id, field] => {
            let fid = faultline_types::ids::FactionId::from(*faction_id);
            let faction = scenario.factions.get(&fid).ok_or_else(|| {
                StatsError::InvalidConfig(format!("faction '{faction_id}' not found"))
            })?;
            match *field {
                "initial_morale" => Ok(faction.initial_morale),
                "initial_resources" => Ok(faction.initial_resources),
                "resource_rate" => Ok(faction.resource_rate),
                "logistics_capacity" => Ok(faction.logistics_capacity),
                "command_resilience" => Ok(faction.command_resilience),
                "intelligence" => Ok(faction.intelligence),
                _ => Err(StatsError::InvalidConfig(format!(
                    "unknown faction field: '{field}'"
                ))),
            }
        },
        ["political_climate", field] => match *field {
            "tension" => Ok(scenario.political_climate.tension),
            "institutional_trust" => Ok(scenario.political_climate.institutional_trust),
            _ => Err(StatsError::InvalidConfig(format!(
                "unknown political_climate field: '{field}'"
            ))),
        },
        ["political_climate", "media", field] => match *field {
            "disinformation_susceptibility" => Ok(scenario
                .political_climate
                .media_landscape
                .disinformation_susceptibility),
            "state_control" => Ok(scenario.political_climate.media_landscape.state_control),
            _ => Err(StatsError::InvalidConfig(format!(
                "unknown media field: '{field}'"
            ))),
        },
        ["kill_chain", chain_id, "phase", phase_id, field] => {
            let phase = get_phase(scenario, chain_id, phase_id)?;
            match *field {
                "base_success_probability" => Ok(phase.base_success_probability),
                "detection_probability_per_tick" => Ok(phase.detection_probability_per_tick),
                "attribution_difficulty" => Ok(phase.attribution_difficulty),
                "prerequisite_success_boost" => Ok(phase.prerequisite_success_boost),
                _ => Err(StatsError::InvalidConfig(format!(
                    "unknown phase field '{field}' in kill_chain '{chain_id}' phase '{phase_id}'"
                ))),
            }
        },
        ["kill_chain", chain_id, "phase", phase_id, "cost", field] => {
            let phase = get_phase(scenario, chain_id, phase_id)?;
            match *field {
                "attacker_dollars" => Ok(phase.cost.attacker_dollars),
                "defender_dollars" => Ok(phase.cost.defender_dollars),
                "attacker_resources" => Ok(phase.cost.attacker_resources),
                _ => Err(StatsError::InvalidConfig(format!(
                    "unknown phase cost field '{field}' in kill_chain '{chain_id}' phase '{phase_id}'"
                ))),
            }
        },
        _ => Err(StatsError::InvalidConfig(format!(
            "unsupported parameter path: '{param}'"
        ))),
    }
}

pub fn set_param(scenario: &mut Scenario, param: &str, value: f64) -> Result<(), StatsError> {
    let parts: Vec<&str> = param.split('.').collect();

    match parts.as_slice() {
        ["faction", faction_id, field] => {
            let fid = faultline_types::ids::FactionId::from(*faction_id);
            let faction = scenario.factions.get_mut(&fid).ok_or_else(|| {
                StatsError::InvalidConfig(format!("faction '{faction_id}' not found"))
            })?;
            match *field {
                "initial_morale" => faction.initial_morale = value,
                "initial_resources" => faction.initial_resources = value,
                "resource_rate" => faction.resource_rate = value,
                "logistics_capacity" => faction.logistics_capacity = value,
                "command_resilience" => faction.command_resilience = value,
                "intelligence" => faction.intelligence = value,
                _ => {
                    return Err(StatsError::InvalidConfig(format!(
                        "unknown faction field: '{field}'"
                    )));
                },
            }
        },
        ["political_climate", field] => match *field {
            "tension" => scenario.political_climate.tension = value,
            "institutional_trust" => scenario.political_climate.institutional_trust = value,
            _ => {
                return Err(StatsError::InvalidConfig(format!(
                    "unknown political_climate field: '{field}'"
                )));
            },
        },
        ["political_climate", "media", field] => match *field {
            "disinformation_susceptibility" => {
                scenario
                    .political_climate
                    .media_landscape
                    .disinformation_susceptibility = value;
            },
            "state_control" => {
                scenario.political_climate.media_landscape.state_control = value;
            },
            _ => {
                return Err(StatsError::InvalidConfig(format!(
                    "unknown media field: '{field}'"
                )));
            },
        },
        ["kill_chain", chain_id, "phase", phase_id, field] => {
            let phase = get_phase_mut(scenario, chain_id, phase_id)?;
            match *field {
                "base_success_probability" => phase.base_success_probability = value,
                "detection_probability_per_tick" => phase.detection_probability_per_tick = value,
                "attribution_difficulty" => phase.attribution_difficulty = value,
                "prerequisite_success_boost" => phase.prerequisite_success_boost = value,
                _ => {
                    return Err(StatsError::InvalidConfig(format!(
                        "unknown phase field '{field}' in kill_chain '{chain_id}' phase '{phase_id}'"
                    )));
                },
            }
        },
        ["kill_chain", chain_id, "phase", phase_id, "cost", field] => {
            let phase = get_phase_mut(scenario, chain_id, phase_id)?;
            match *field {
                "attacker_dollars" => phase.cost.attacker_dollars = value,
                "defender_dollars" => phase.cost.defender_dollars = value,
                "attacker_resources" => phase.cost.attacker_resources = value,
                _ => {
                    return Err(StatsError::InvalidConfig(format!(
                        "unknown phase cost field '{field}' in kill_chain '{chain_id}' phase '{phase_id}'"
                    )));
                },
            }
        },
        _ => {
            return Err(StatsError::InvalidConfig(format!(
                "unsupported parameter path: '{param}'"
            )));
        },
    }

    Ok(())
}

fn get_phase<'a>(
    scenario: &'a Scenario,
    chain_id: &str,
    phase_id: &str,
) -> Result<&'a faultline_types::campaign::CampaignPhase, StatsError> {
    let cid = KillChainId::from(chain_id);
    let chain = scenario
        .kill_chains
        .get(&cid)
        .ok_or_else(|| StatsError::InvalidConfig(format!("kill chain '{chain_id}' not found")))?;
    let pid = PhaseId::from(phase_id);
    chain.phases.get(&pid).ok_or_else(|| {
        StatsError::InvalidConfig(format!(
            "phase '{phase_id}' not found in kill chain '{chain_id}'"
        ))
    })
}

fn get_phase_mut<'a>(
    scenario: &'a mut Scenario,
    chain_id: &str,
    phase_id: &str,
) -> Result<&'a mut faultline_types::campaign::CampaignPhase, StatsError> {
    let cid = KillChainId::from(chain_id);
    let chain = scenario
        .kill_chains
        .get_mut(&cid)
        .ok_or_else(|| StatsError::InvalidConfig(format!("kill chain '{chain_id}' not found")))?;
    let pid = PhaseId::from(phase_id);
    chain.phases.get_mut(&pid).ok_or_else(|| {
        StatsError::InvalidConfig(format!(
            "phase '{phase_id}' not found in kill chain '{chain_id}'"
        ))
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use faultline_types::faction::{Faction, FactionType, ForceUnit, UnitType};
    use faultline_types::ids::{FactionId, ForceId, RegionId, VictoryId};
    use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::strategy::Doctrine;
    use faultline_types::victory::{VictoryCondition, VictoryType};

    fn minimal_scenario() -> Scenario {
        let r1 = RegionId::from("region-a");
        let r2 = RegionId::from("region-b");
        let f_gov = FactionId::from("gov");
        let f_rebel = FactionId::from("rebel");

        let mut regions = BTreeMap::new();
        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "Region A".into(),
                population: 100_000,
                urbanization: 0.5,
                initial_control: Some(f_gov.clone()),
                strategic_value: 5.0,
                borders: vec![r2.clone()],
                centroid: None,
            },
        );
        regions.insert(
            r2.clone(),
            Region {
                id: r2.clone(),
                name: "Region B".into(),
                population: 50_000,
                urbanization: 0.3,
                initial_control: Some(f_rebel.clone()),
                strategic_value: 3.0,
                borders: vec![r1.clone()],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(
            f_gov.clone(),
            make_faction(f_gov.clone(), "Government", r1.clone()),
        );
        factions.insert(
            f_rebel.clone(),
            make_faction(f_rebel.clone(), "Rebels", r2.clone()),
        );

        let mut victory_conditions = BTreeMap::new();
        let vc_id = VictoryId::from("gov-win");
        victory_conditions.insert(
            vc_id.clone(),
            VictoryCondition {
                id: vc_id,
                name: "Government Dominance".into(),
                faction: f_gov,
                condition: VictoryType::MilitaryDominance {
                    enemy_strength_below: 0.01,
                },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "Test Scenario".into(),
                description: "Minimal scenario for testing".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 2,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![
                    TerrainModifier {
                        region: r1,
                        terrain_type: TerrainType::Urban,
                        movement_modifier: 1.0,
                        defense_modifier: 1.0,
                        visibility: 0.8,
                    },
                    TerrainModifier {
                        region: r2,
                        terrain_type: TerrainType::Rural,
                        movement_modifier: 1.0,
                        defense_modifier: 0.8,
                        visibility: 0.9,
                    },
                ],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.5,
                institutional_trust: 0.6,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.7,
                    internet_availability: 0.8,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 10,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(42),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
        }
    }

    fn make_faction(id: FactionId, name: &str, region: RegionId) -> Faction {
        let force_id = ForceId::from(format!("{}-inf", id));
        let mut forces = BTreeMap::new();
        forces.insert(
            force_id.clone(),
            ForceUnit {
                id: force_id,
                name: format!("{name} Infantry"),
                unit_type: UnitType::Infantry,
                region,
                strength: 100.0,
                mobility: 1.0,
                force_projection: None,
                upkeep: 1.0,
                morale_modifier: 0.0,
                capabilities: vec![],
            },
        );
        Faction {
            id,
            name: name.into(),
            faction_type: FactionType::Insurgent,
            description: "Test faction".into(),
            color: "#000000".into(),
            forces,
            tech_access: vec![],
            initial_morale: 0.8,
            logistics_capacity: 100.0,
            initial_resources: 1000.0,
            resource_rate: 10.0,
            recruitment: None,
            command_resilience: 0.5,
            intelligence: 0.5,
            diplomacy: vec![],
            doctrine: Doctrine::Conventional,
            escalation_rules: None,
            defender_capacities: BTreeMap::new(),
        }
    }

    #[test]
    fn get_set_faction_morale() {
        let scenario = minimal_scenario();
        let val =
            get_param(&scenario, "faction.gov.initial_morale").expect("should get faction morale");
        assert!((val - 0.8).abs() < f64::EPSILON);

        let mut scenario2 = scenario.clone();
        set_param(&mut scenario2, "faction.gov.initial_morale", 0.5)
            .expect("should set faction morale");
        let val2 =
            get_param(&scenario2, "faction.gov.initial_morale").expect("should get updated morale");
        assert!((val2 - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn get_set_tension() {
        let scenario = minimal_scenario();
        let val = get_param(&scenario, "political_climate.tension").expect("should get tension");
        assert!((val - 0.5).abs() < f64::EPSILON);

        let mut scenario2 = scenario.clone();
        set_param(&mut scenario2, "political_climate.tension", 0.9).expect("should set tension");
        let val2 =
            get_param(&scenario2, "political_climate.tension").expect("should get updated tension");
        assert!((val2 - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn invalid_param_returns_error() {
        let scenario = minimal_scenario();
        assert!(get_param(&scenario, "bogus.path").is_err());
        assert!(get_param(&scenario, "faction.nonexistent.initial_morale").is_err());
        assert!(get_param(&scenario, "faction.gov.nonexistent_field").is_err());
    }

    #[test]
    fn sensitivity_sweep_produces_correct_steps() {
        let scenario = minimal_scenario();
        let config = MonteCarloConfig {
            num_runs: 2,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = run_sensitivity(
            &scenario,
            &config,
            "faction.gov.initial_morale",
            0.2,
            1.0,
            3,
        )
        .expect("sensitivity sweep should succeed");

        assert_eq!(result.varied_values.len(), 3);
        assert!((result.varied_values[0] - 0.2).abs() < f64::EPSILON);
        assert!((result.varied_values[1] - 0.6).abs() < f64::EPSILON);
        assert!((result.varied_values[2] - 1.0).abs() < f64::EPSILON);
        assert_eq!(result.outcomes.len(), 3);
        assert!((result.baseline_value - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn sensitivity_zero_steps_errors() {
        let scenario = minimal_scenario();
        let config = MonteCarloConfig {
            num_runs: 1,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };
        assert!(
            run_sensitivity(&scenario, &config, "political_climate.tension", 0.0, 1.0, 0).is_err()
        );
    }

    #[test]
    fn sensitivity_inverted_range_errors() {
        let scenario = minimal_scenario();
        let config = MonteCarloConfig {
            num_runs: 1,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };
        assert!(
            run_sensitivity(&scenario, &config, "political_climate.tension", 0.9, 0.1, 3).is_err()
        );
    }

    #[test]
    fn get_set_all_faction_params() {
        let scenario = minimal_scenario();
        let params = [
            ("faction.gov.initial_resources", 1000.0),
            ("faction.gov.resource_rate", 10.0),
            ("faction.gov.logistics_capacity", 100.0),
            ("faction.gov.command_resilience", 0.5),
            ("faction.gov.intelligence", 0.5),
        ];
        for (param, expected) in &params {
            let val = get_param(&scenario, param).unwrap_or_else(|_| panic!("get {param} failed"));
            assert!(
                (val - expected).abs() < f64::EPSILON,
                "{param} expected {expected}, got {val}"
            );
        }

        // Set and verify round-trip.
        for (param, _) in &params {
            let mut s = scenario.clone();
            set_param(&mut s, param, 42.0).unwrap_or_else(|_| panic!("set {param} failed"));
            let val = get_param(&s, param).unwrap_or_else(|_| panic!("get {param} after set"));
            assert!(
                (val - 42.0).abs() < f64::EPSILON,
                "{param} should be 42.0 after set"
            );
        }
    }

    #[test]
    fn get_set_media_params() {
        let scenario = minimal_scenario();

        let val = get_param(
            &scenario,
            "political_climate.media.disinformation_susceptibility",
        )
        .expect("should get disinfo");
        assert!((val - 0.3).abs() < f64::EPSILON);

        let val =
            get_param(&scenario, "political_climate.media.state_control").expect("should get sc");
        assert!((val - 0.4).abs() < f64::EPSILON);

        let mut s = scenario.clone();
        set_param(
            &mut s,
            "political_climate.media.disinformation_susceptibility",
            0.8,
        )
        .expect("set disinfo");
        let val = get_param(&s, "political_climate.media.disinformation_susceptibility")
            .expect("get disinfo after set");
        assert!((val - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn get_set_institutional_trust() {
        let scenario = minimal_scenario();
        let val = get_param(&scenario, "political_climate.institutional_trust")
            .expect("should get trust");
        assert!((val - 0.6).abs() < f64::EPSILON);

        let mut s = scenario;
        set_param(&mut s, "political_climate.institutional_trust", 0.2).expect("set trust");
        let val =
            get_param(&s, "political_climate.institutional_trust").expect("get trust after set");
        assert!((val - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn sensitivity_single_step_uses_low_value() {
        let scenario = minimal_scenario();
        let config = MonteCarloConfig {
            num_runs: 1,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = run_sensitivity(&scenario, &config, "political_climate.tension", 0.3, 0.3, 1)
            .expect("single step should succeed");

        assert_eq!(result.varied_values.len(), 1);
        assert!((result.varied_values[0] - 0.3).abs() < f64::EPSILON);
        assert_eq!(result.outcomes.len(), 1);
    }

    #[test]
    fn sensitivity_outcomes_match_varied_values_count() {
        let scenario = minimal_scenario();
        let config = MonteCarloConfig {
            num_runs: 2,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };

        let result = run_sensitivity(
            &scenario,
            &config,
            "faction.gov.initial_morale",
            0.1,
            0.9,
            5,
        )
        .expect("sweep should succeed");

        assert_eq!(
            result.outcomes.len(),
            result.varied_values.len(),
            "one outcome per varied value"
        );
        assert_eq!(result.outcomes.len(), 5);

        for summary in &result.outcomes {
            assert_eq!(
                summary.total_runs, 2,
                "each step should run 2 Monte Carlo runs"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Epic B kill-chain path tests
    //
    // The same `set_param` / `get_param` paths are used by
    // `--counterfactual` overrides; these tests pin the expanded path
    // grammar and the error messages that surface when a path is
    // malformed. The minimal scenario doesn't carry kill chains, so
    // these tests build a small chain on top of it.
    // -----------------------------------------------------------------------

    use faultline_types::campaign::{CampaignPhase, KillChain, PhaseCost};
    use faultline_types::ids::{KillChainId, PhaseId};

    fn scenario_with_chain() -> Scenario {
        let mut scenario = minimal_scenario();
        let chain_id = KillChainId::from("alpha");
        let phase_id = PhaseId::from("recon");
        let mut phases = std::collections::BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Recon".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 0.7,
                min_duration: 1,
                max_duration: 2,
                detection_probability_per_tick: 0.05,
                prerequisite_success_boost: 0.1,
                attribution_difficulty: 0.6,
                cost: PhaseCost {
                    attacker_dollars: 1_000.0,
                    defender_dollars: 50_000.0,
                    attacker_resources: 1.5,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Alpha".into(),
                description: String::new(),
                attacker: FactionId::from("gov"),
                target: FactionId::from("rebel"),
                entry_phase: phase_id,
                phases,
            },
        );
        scenario
    }

    #[test]
    fn get_set_kill_chain_phase_fields_roundtrip() {
        let scenario = scenario_with_chain();
        let cases = [
            ("kill_chain.alpha.phase.recon.base_success_probability", 0.7),
            (
                "kill_chain.alpha.phase.recon.detection_probability_per_tick",
                0.05,
            ),
            ("kill_chain.alpha.phase.recon.attribution_difficulty", 0.6),
            (
                "kill_chain.alpha.phase.recon.prerequisite_success_boost",
                0.1,
            ),
            (
                "kill_chain.alpha.phase.recon.cost.attacker_dollars",
                1_000.0,
            ),
            (
                "kill_chain.alpha.phase.recon.cost.defender_dollars",
                50_000.0,
            ),
            ("kill_chain.alpha.phase.recon.cost.attacker_resources", 1.5),
        ];
        for (path, expected) in cases {
            let v = get_param(&scenario, path).unwrap_or_else(|e| panic!("get {path}: {e}"));
            assert!(
                (v - expected).abs() < 1e-9,
                "{path} expected {expected}, got {v}"
            );

            let mut s = scenario.clone();
            set_param(&mut s, path, 0.42).unwrap_or_else(|e| panic!("set {path}: {e}"));
            let v2 = get_param(&s, path).expect("get after set");
            assert!((v2 - 0.42).abs() < 1e-9, "{path} should roundtrip 0.42");
        }
    }

    #[test]
    fn get_kill_chain_path_errors_include_chain_and_phase_context() {
        let scenario = scenario_with_chain();

        // Missing chain — error must name the chain we were looking for.
        let err = get_param(
            &scenario,
            "kill_chain.does_not_exist.phase.recon.base_success_probability",
        )
        .expect_err("missing chain should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("does_not_exist"),
            "error must name the missing chain; got: {msg}"
        );

        // Missing phase — error must name the phase.
        let err = get_param(
            &scenario,
            "kill_chain.alpha.phase.no_such_phase.base_success_probability",
        )
        .expect_err("missing phase should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no_such_phase"),
            "error must name the missing phase; got: {msg}"
        );

        // Unknown phase field — error must name the chain and phase
        // along with the bad field, so the user knows where to look.
        let err = get_param(&scenario, "kill_chain.alpha.phase.recon.no_such_field")
            .expect_err("unknown field should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no_such_field") && msg.contains("alpha") && msg.contains("recon"),
            "error must name the chain, phase, and bad field; got: {msg}"
        );

        // Unknown cost field — same expectation under the .cost.<x> branch.
        let err = get_param(
            &scenario,
            "kill_chain.alpha.phase.recon.cost.no_such_cost_field",
        )
        .expect_err("unknown cost field should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no_such_cost_field") && msg.contains("alpha") && msg.contains("recon"),
            "error must name chain/phase/field for cost branch; got: {msg}"
        );
    }

    #[test]
    fn set_kill_chain_path_errors_include_chain_and_phase_context() {
        // Same expectations on the mutating side.
        let mut scenario = scenario_with_chain();

        let err = set_param(
            &mut scenario,
            "kill_chain.alpha.phase.recon.no_such_field",
            0.5,
        )
        .expect_err("unknown field should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("no_such_field") && msg.contains("alpha") && msg.contains("recon"),
            "set_param error must name the chain, phase, and bad field; got: {msg}"
        );
    }

    #[test]
    fn unsupported_top_level_path_errors() {
        let scenario = scenario_with_chain();
        let err = get_param(&scenario, "totally_unknown.path").expect_err("should error");
        assert!(format!("{err}").contains("unsupported parameter path"));
    }

    #[test]
    fn sensitivity_can_sweep_kill_chain_phase_parameter() {
        // End-to-end: the sensitivity sweeper must accept the new
        // kill_chain phase paths so the same harness can produce a
        // detection-probability sweep without code changes.
        let scenario = scenario_with_chain();
        let config = MonteCarloConfig {
            num_runs: 3,
            seed: Some(42),
            collect_snapshots: false,
            parallel: false,
        };
        let result = run_sensitivity(
            &scenario,
            &config,
            "kill_chain.alpha.phase.recon.detection_probability_per_tick",
            0.0,
            0.5,
            3,
        )
        .expect("sweep should succeed against a kill-chain phase parameter");
        assert_eq!(result.outcomes.len(), 3);
        assert!((result.baseline_value - 0.05).abs() < 1e-9);
    }
}
