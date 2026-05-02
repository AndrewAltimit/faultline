//! Shared test fixtures for the report module's unit tests. Lives at
//! the report-module level so per-section tests can share the same
//! minimal `Scenario` and `MonteCarloSummary` constructors without
//! duplicating boilerplate.

use std::collections::BTreeMap;

use faultline_types::map::{MapConfig, MapSource};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::stats::MonteCarloSummary;

pub(crate) fn empty_summary() -> MonteCarloSummary {
    MonteCarloSummary {
        total_runs: 0,
        win_rates: BTreeMap::new(),
        win_rate_cis: BTreeMap::new(),
        average_duration: 0.0,
        metric_distributions: BTreeMap::new(),
        regional_control: BTreeMap::new(),
        event_probabilities: BTreeMap::new(),
        campaign_summaries: BTreeMap::new(),
        feasibility_matrix: Vec::new(),
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
    }
}

pub(crate) fn minimal_scenario() -> Scenario {
    Scenario {
        meta: ScenarioMeta {
            name: "Report Test".into(),
            description: "description".into(),
            author: "test".into(),
            version: "0.0.1".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            historical_analogue: None,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 1,
                height: 1,
            },
            regions: BTreeMap::new(),
            infrastructure: BTreeMap::new(),
            terrain: vec![],
        },
        factions: BTreeMap::new(),
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.0,
            institutional_trust: 0.5,
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
            max_ticks: 1,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(0),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
        },
        victory_conditions: BTreeMap::new(),
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: std::collections::BTreeMap::new(),
    }
}
