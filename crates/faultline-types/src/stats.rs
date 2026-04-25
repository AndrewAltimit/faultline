use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, FactionId, InfraId, KillChainId, PhaseId, RegionId};
use crate::strategy::FactionState;

/// Configuration for Monte Carlo simulation runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub num_runs: u32,
    pub seed: Option<u64>,
    pub collect_snapshots: bool,
    /// Reserved for future parallel execution inside `MonteCarloRunner::run`.
    ///
    /// Currently unused: the in-crate runner is unconditionally sequential
    /// (parallelism in the native CLI is handled by `faultline-cli` via a
    /// rayon pool over `Engine::run` calls, not via this flag), so callers
    /// should set this to `false`. The field is kept on the struct so that
    /// a future parallel runner can be wired in without a breaking schema
    /// change.
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
    /// Per-kill-chain terminal report. Empty when the
    /// scenario has no kill chains.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub campaign_reports: BTreeMap<KillChainId, CampaignReport>,
}

/// End-of-run snapshot of a single kill chain's resolution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignReport {
    pub chain_id: KillChainId,
    /// Final status of each phase in the chain.
    pub phase_outcomes: BTreeMap<PhaseId, PhaseOutcome>,
    /// Accumulated detection probability per phase.
    pub detection_accumulation: BTreeMap<PhaseId, f64>,
    pub defender_alerted: bool,
    pub attacker_spend: f64,
    pub defender_spend: f64,
    pub attribution_confidence: f64,
    pub information_dominance: f64,
    pub institutional_erosion: f64,
    pub coercion_pressure: f64,
    pub political_cost: f64,
}

/// The terminal state of a single campaign phase.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PhaseOutcome {
    Pending,
    Active,
    Succeeded { tick: u32 },
    Failed { tick: u32 },
    Detected { tick: u32 },
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
    /// 95% Wilson score intervals for `win_rates`. Same keys.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub win_rate_cis: BTreeMap<FactionId, ConfidenceInterval>,
    pub average_duration: f64,
    pub metric_distributions: BTreeMap<MetricType, DistributionStats>,
    /// Per-region probability of each faction controlling it at the end of the simulation.
    pub regional_control: BTreeMap<RegionId, BTreeMap<FactionId, f64>>,
    /// Probability (0.0–1.0) of each event firing across all runs.
    pub event_probabilities: BTreeMap<EventId, f64>,
    /// Per-kill-chain phase-level aggregation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub campaign_summaries: BTreeMap<KillChainId, CampaignSummary>,
    /// Feasibility matrix rows per kill chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feasibility_matrix: Vec<FeasibilityRow>,
    /// Doctrinal seam analysis scores per kill chain.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub seam_scores: BTreeMap<KillChainId, SeamScore>,
}

/// Aggregate statistics for one kill chain across all runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignSummary {
    pub chain_id: KillChainId,
    /// Per-phase aggregate outcomes.
    pub phase_stats: BTreeMap<PhaseId, PhaseStats>,
    /// Fraction of runs where the chain reached its terminal phase
    /// with at least one success (any kinetic output delivered).
    pub overall_success_rate: f64,
    /// Fraction of runs where the defender was alerted at any point.
    pub detection_rate: f64,
    /// Mean attacker dollar outlay across runs.
    pub mean_attacker_spend: f64,
    /// Mean defender dollar outlay across runs.
    pub mean_defender_spend: f64,
    /// Cost asymmetry ratio: defender_spend / attacker_spend (0 if
    /// attacker spend is zero).
    pub cost_asymmetry_ratio: f64,
    /// Mean attribution confidence (0 = unknown, 1 = definitive).
    pub mean_attribution_confidence: f64,
}

/// Aggregate statistics for a single phase across runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseStats {
    pub phase_id: PhaseId,
    pub success_rate: f64,
    pub failure_rate: f64,
    pub detection_rate: f64,
    pub not_reached_rate: f64,
    /// Mean tick at which the phase resolved (success/fail/detection).
    /// `None` if no runs reached a terminal state for this phase.
    pub mean_completion_tick: Option<f64>,
    /// 95% Wilson score intervals for the four rates above. `Some`
    /// when `total_runs > 0`; `None` means the runner had no data to
    /// estimate from. The outer `Option` enforces the all-or-none
    /// invariant at the type level — partial CIs are unrepresentable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_95: Option<PhaseStatsCIs>,
}

/// 95% Wilson score intervals for the rates on [`PhaseStats`]. All
/// four fields share the same denominator (`total_runs`), so this
/// struct is constructed atomically — the enclosing `Option` on
/// [`PhaseStats::ci_95`] carries the "no data" state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseStatsCIs {
    pub success_rate: ConfidenceInterval,
    pub failure_rate: ConfidenceInterval,
    pub detection_rate: ConfidenceInterval,
    pub not_reached_rate: ConfidenceInterval,
}

/// Feasibility matrix row for one kill chain.
///
/// Each field is scored `[0, 1]` with a qualitative confidence rating
/// derived from variance across Monte Carlo runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeasibilityRow {
    pub chain_id: KillChainId,
    pub chain_name: String,
    /// Technology readiness (average success probability across phases).
    pub technology_readiness: f64,
    /// Operational complexity (1.0 - shortest-path success probability).
    pub operational_complexity: f64,
    /// Probability the operation is detected before completion.
    pub detection_probability: f64,
    /// Overall success probability of the full kill chain.
    pub success_probability: f64,
    /// Consequence severity (normalized damage + institutional erosion).
    pub consequence_severity: f64,
    /// Attribution difficulty (mean `1 - attribution_confidence`).
    pub attribution_difficulty: f64,
    /// Cost asymmetry ratio (defender $ / attacker $).
    pub cost_asymmetry_ratio: f64,
    /// Confidence ratings based on MC variance.
    pub confidence: FeasibilityConfidence,
    /// 95% Wilson score intervals for the rate-valued cells.
    /// Populated when enough runs exist to compute them.
    #[serde(default)]
    pub ci_95: FeasibilityCIs,
}

/// 95% confidence intervals for the rate-valued [`FeasibilityRow`]
/// fields. All entries are optional because a CI is undefined at
/// `n == 0`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeasibilityCIs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_probability: Option<ConfidenceInterval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_probability: Option<ConfidenceInterval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consequence_severity: Option<ConfidenceInterval>,
}

/// Serializable 95% confidence interval on a scalar estimate.
///
/// Fields are `pub` so that `serde` derives and downstream readers
/// (report rendering, integration tests, JS callers via wasm) can
/// consume them directly. For *construction*, prefer
/// [`ConfidenceInterval::new`] — it enforces the invariant
/// `lower <= point <= upper` and guards against silently emitting
/// nonsensical intervals (`lower > upper`, etc.) into report output.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceInterval {
    /// Point estimate (observed proportion or mean).
    pub point: f64,
    pub lower: f64,
    pub upper: f64,
    /// Sample size supporting the estimate.
    pub n: u32,
}

impl ConfidenceInterval {
    /// Construct a `ConfidenceInterval` with invariant checks.
    ///
    /// Panics in debug builds if `lower`, `point`, or `upper` are
    /// non-finite, or if `lower <= point <= upper` does not hold. In
    /// release builds the values are used as-given (no clamping) so
    /// this is a zero-cost wrapper in hot paths.
    pub fn new(point: f64, lower: f64, upper: f64, n: u32) -> Self {
        debug_assert!(
            point.is_finite() && lower.is_finite() && upper.is_finite(),
            "ConfidenceInterval bounds must be finite: point={point} lower={lower} upper={upper}"
        );
        debug_assert!(
            lower <= point && point <= upper,
            "ConfidenceInterval invariant violated: lower={lower} point={point} upper={upper}"
        );
        Self {
            point,
            lower,
            upper,
            n,
        }
    }
}

/// Confidence ratings per feasibility factor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeasibilityConfidence {
    pub technology_readiness: ConfidenceLevel,
    pub operational_complexity: ConfidenceLevel,
    pub detection_probability: ConfidenceLevel,
    pub success_probability: ConfidenceLevel,
    pub consequence_severity: ConfidenceLevel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

/// Doctrinal seam score — how much of the attack success probability
/// is attributable to exploiting gaps between defensive domains.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeamScore {
    pub chain_id: KillChainId,
    /// Count of phases targeting two or more defensive domains.
    pub cross_domain_phase_count: u32,
    /// Mean number of distinct defensive domains targeted per phase.
    pub mean_domains_per_phase: f64,
    /// Frequency of each domain across the chain.
    pub domain_frequency: BTreeMap<String, u32>,
    /// Share of success probability attributable to seam exploitation
    /// (weighted by cross-domain phase success rates).
    pub seam_exploitation_share: f64,
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
    /// 95% percentile-bootstrap CI on the mean. `None` when the
    /// distribution is empty or when the consumer computed the stats
    /// without supplying a bootstrap seed (e.g. a stored summary from
    /// a pre-bootstrap build).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_ci_mean: Option<ConfidenceInterval>,
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
    /// Campaign reports are small — preserved verbatim, not delta-encoded.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub campaign_reports: BTreeMap<KillChainId, CampaignReport>,
}
