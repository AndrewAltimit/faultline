use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, FactionId, InfraId, RegionId};
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

/// A single event firing record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRecord {
    pub tick: u32,
    pub event_id: EventId,
}

/// Results from a single simulation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    pub run_index: u32,
    pub seed: u64,
    pub outcome: Outcome,
    pub final_tick: u32,
    /// Terminal state snapshot — always present regardless of snapshot_interval.
    pub final_state: StateSnapshot,
    pub snapshots: Vec<StateSnapshot>,
    /// Complete log of every event firing across all ticks.
    pub event_log: Vec<EventRecord>,
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
    /// Per-region probability of each faction controlling it at the end of the simulation.
    pub regional_control: BTreeMap<RegionId, BTreeMap<FactionId, f64>>,
    /// Probability (0.0–1.0) of each event firing across all runs.
    pub event_probabilities: BTreeMap<EventId, f64>,
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
    /// Infrastructure health per node in `[0.0, 1.0]`.
    pub infra_status: BTreeMap<InfraId, f64>,
    pub tension: f64,
    pub events_fired_this_tick: Vec<EventId>,
}

/// A delta-encoded snapshot storing only fields that changed from the previous.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaSnapshot {
    pub tick: u32,
    /// Only faction states that changed (any numeric field differs by > epsilon).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faction_states: BTreeMap<FactionId, FactionState>,
    /// Only region control that changed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub region_control: BTreeMap<RegionId, Option<FactionId>>,
    /// Only infra nodes that changed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub infra_status: BTreeMap<InfraId, f64>,
    /// Tension (always included — cheap).
    pub tension: f64,
    /// Events fired this tick (always included).
    pub events_fired_this_tick: Vec<EventId>,
}

/// A run with delta-encoded snapshots for memory-efficient storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaEncodedRun {
    pub run_index: u32,
    pub seed: u64,
    pub outcome: Outcome,
    pub final_tick: u32,
    pub final_state: StateSnapshot,
    /// First snapshot is a full `StateSnapshot` serialized as a delta (all fields present).
    /// Subsequent snapshots only contain changed fields.
    pub snapshots: Vec<DeltaSnapshot>,
    /// Complete event log preserved through encoding (not delta-encoded).
    pub event_log: Vec<EventRecord>,
}
