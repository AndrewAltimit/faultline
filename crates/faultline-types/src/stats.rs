use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, FactionId, RegionId};
use crate::strategy::FactionState;

/// Configuration for Monte Carlo simulation runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub num_runs: u32,
    pub seed: Option<u64>,
    pub collect_snapshots: bool,
    pub parallel: bool,
}

/// Aggregated results from all Monte Carlo runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub runs: Vec<RunResult>,
    pub summary: MonteCarloSummary,
}

/// Results from a single simulation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    pub run_index: u32,
    pub seed: u64,
    pub outcome: Outcome,
    pub final_tick: u32,
    pub snapshots: Vec<StateSnapshot>,
}

/// The outcome of a single run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub victor: Option<FactionId>,
    pub victory_condition: Option<String>,
    pub final_tension: f64,
}

/// Summary statistics across all Monte Carlo runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonteCarloSummary {
    pub total_runs: u32,
    pub win_rates: BTreeMap<FactionId, f64>,
    pub average_duration: f64,
    pub metric_distributions: BTreeMap<MetricType, DistributionStats>,
}

/// Descriptive statistics for a distribution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistributionStats {
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub percentile_5: f64,
    pub percentile_95: f64,
}

/// Categories of metrics tracked across runs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MetricType {
    Duration,
    FinalTension,
    TotalCasualties,
    InfrastructureDamage,
    CivilianDisplacement,
    ResourcesExpended,
    Custom(String),
}

/// Results from sensitivity analysis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SensitivityResult {
    pub parameter: String,
    pub baseline_value: f64,
    pub varied_values: Vec<f64>,
    pub outcomes: Vec<MonteCarloSummary>,
}

/// A snapshot of the full simulation state at a given tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub tick: u32,
    pub faction_states: BTreeMap<FactionId, FactionState>,
    pub region_control: BTreeMap<RegionId, Option<FactionId>>,
    pub tension: f64,
    pub events_fired_this_tick: Vec<EventId>,
}
