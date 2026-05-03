use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::faction::Diplomacy;
use crate::ids::{
    DefenderRoleId, EdgeId, EventId, FactionId, InfraId, KillChainId, NetworkId, NodeId, PhaseId,
    RegionId, SegmentId, TechCardId,
};
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
    /// Per-defender-role queue summary at run end. Empty when
    /// no scenario faction declares `defender_capacities`. Sorted
    /// (faction_id, role_id) for deterministic rendering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defender_queue_reports: Vec<DefenderQueueReport>,
    /// Per-network terminal-state report. Empty when the
    /// scenario declares no networks. Each entry carries the resilience
    /// trajectory across ticks plus terminal metrics; aggregated across
    /// runs by [`MonteCarloSummary::network_summaries`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub network_reports: BTreeMap<NetworkId, NetworkReport>,
    /// Log of every alliance-fracture firing in this run, in tick
    /// order. Each entry records which faction's
    /// stance flipped against which counterparty, the rule that
    /// fired, and the previous / new stance. Empty when the scenario
    /// declares no `alliance_fracture` rules or none of its
    /// conditions were satisfied during the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fracture_events: Vec<FractureEvent>,
    /// Per-faction supply-pressure summary for this run. Only
    /// populated for factions that own at least
    /// one `kind = "supply"` network — legacy factions are elided.
    /// Keyed by `FactionId` for deterministic rendering.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub supply_pressure_reports: BTreeMap<FactionId, SupplyPressureReport>,
    /// Log of every civilian-segment activation in this run
    /// (population-segment activation), in tick order.
    /// Each entry records the tick, the segment that activated, the
    /// faction whose sympathy crossed the activation threshold, and
    /// the action variants attached to the segment. Empty when the
    /// scenario declares no `population_segments` or none crossed
    /// their threshold during the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub civilian_activations: Vec<CivilianActivationEvent>,
    /// Per-faction tech-card cost activity for this run. Records
    /// what each faction spent on tech
    /// (deployment + maintenance), which techs were denied at deploy
    /// time because the faction couldn't afford them, and which were
    /// decommissioned mid-run because maintenance outran resources.
    /// Only populated for factions whose `tech_access` produced any of
    /// those four signals (non-zero deployment spend, non-zero
    /// maintenance spend, at least one denial, or at least one
    /// decommission). `coverage_limit` activity is *not* surfaced here
    /// — coverage gating is a within-tick limiter on combat
    /// contribution and the per-tick counter is `#[serde(skip)]`
    /// transient state, so it never persists post-tick. Legacy
    /// scenarios with zero-cost tech rosters elide the entry entirely
    /// (the outer `BTreeMap` skips when empty).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tech_costs: BTreeMap<FactionId, TechCostReport>,
}

/// Per-faction tech-card cost activity for one run.
///
/// Captured live by the engine as deployment runs at init and as
/// maintenance ticks accumulate. The four `Vec` fields preserve the
/// runtime decision order so the report can render a faithful "what
/// happened, in what order" trace; the two scalars are the headline
/// totals the cross-run aggregator rolls up.
///
/// Field semantics:
/// - `deployed_techs` — the cards the faction successfully deployed
///   at engine init, in `tech_access` order. Subset of `tech_access`
///   minus `denied_at_deployment`.
/// - `denied_at_deployment` — cards the faction *could not* afford at
///   init time. Each was charged against starting resources in
///   declaration order; the first one whose `deployment_cost` exceeded
///   what was left was denied, and the iteration continued in case a
///   later (cheaper) card could fit. So the list reflects
///   "couldn't fit this one given prior choices", not "card was too
///   expensive in absolute terms".
/// - `decommissioned` — cards lost mid-run because the faction's
///   `cost_per_tick` deduction would have driven `resources` below
///   zero. Recorded with the tick at which the loss happened.
/// - `total_deployment_spend` — sum of `deployment_cost` over
///   `deployed_techs`. This is exactly what the engine deducted from
///   `initial_resources` at init.
/// - `total_maintenance_spend` — sum of per-tick deductions over the
///   life of the run. Cards that decommissioned partway through
///   contribute only the ticks they were active.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TechCostReport {
    pub faction: FactionId,
    #[serde(default)]
    pub deployed_techs: Vec<TechCardId>,
    #[serde(default)]
    pub denied_at_deployment: Vec<TechCardId>,
    #[serde(default)]
    pub decommissioned: Vec<TechDecommissionEvent>,
    pub total_deployment_spend: f64,
    pub total_maintenance_spend: f64,
}

/// One mid-run tech-card decommission event.
///
/// Captured at the tick when `cost_per_tick` couldn't be paid out of
/// the faction's current `resources`. The card is removed from
/// `tech_deployed` for the rest of the run and contributes no further
/// combat / detection effects. The maintenance for the *non-paid* tick
/// is **not** charged — the faction stops paying as soon as it can't.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TechDecommissionEvent {
    pub tick: u32,
    pub tech: TechCardId,
}

/// One civilian-segment activation firing (population-segment
/// activation).
///
/// Captured at the tick when the segment's top sympathy crossed
/// `activation_threshold`. `action_kinds` lists the discriminant
/// names of the [`crate::politics::CivilianAction`] variants the
/// segment carries — stored as plain strings rather than the typed
/// enum so cross-run aggregation can count action firings without
/// dragging the full payload (and so the manifest schema stays
/// stable as new variants are added).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CivilianActivationEvent {
    pub tick: u32,
    pub segment: SegmentId,
    pub favored_faction: FactionId,
    /// One entry per element of the segment's `activation_actions`.
    /// Order matches the scenario-authored order — the activation
    /// processor iterates the same vector, so this gives a faithful
    /// "what fired in what order" trace.
    #[serde(default)]
    pub action_kinds: Vec<String>,
}

/// Per-faction supply-pressure summary for one run.
///
/// All four scalars are derived from the per-tick samples the engine
/// captured in [`crate::stats::SupplyPressureReport`]'s upstream state
/// (`RuntimeFactionState.supply_pressure_*`). The report represents a
/// faction that *owned* at least one active supply network — factions
/// without one are not represented at all (the outer `BTreeMap` on
/// [`RunResult::supply_pressure_reports`] elides them entirely).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyPressureReport {
    pub faction: FactionId,
    /// Number of attrition ticks where supply pressure was sampled —
    /// equal to the number of attrition ticks the faction was alive
    /// for and owned a non-degenerate supply network. Always `>= 1`
    /// because the engine only emits this report when at least one
    /// pressure sample was recorded; factions whose owned supply
    /// networks all have zero baseline capacity are skipped, as are
    /// factions eliminated before the first attrition tick.
    pub samples: u32,
    /// Mean per-tick pressure across `samples` (in `[0, 1]`).
    pub mean_pressure: f64,
    /// Minimum per-tick pressure observed (in `[0, 1]`). `1.0` means
    /// supply was never interdicted in this run.
    pub min_pressure: f64,
    /// Number of attrition ticks where pressure was strictly below
    /// the engine's reporting threshold (currently 0.9). A proxy for
    /// "ticks under meaningful supply stress" so the analyst sees
    /// duration of pressure separately from severity.
    pub pressured_ticks: u32,
}

/// One alliance-fracture firing.
///
/// Captured at the tick when the rule's condition was first satisfied.
/// `previous_stance` reflects the live runtime stance — including any
/// prior `EventEffect::DiplomacyChange` overrides — at the moment of
/// firing, so a chain of fractures (Allied -> Cooperative -> Hostile)
/// records each leg correctly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FractureEvent {
    pub tick: u32,
    pub faction: FactionId,
    pub counterparty: FactionId,
    pub rule_id: String,
    pub previous_stance: Diplomacy,
    pub new_stance: Diplomacy,
}

/// End-of-run snapshot of one network's resilience trajectory.
///
/// `samples` holds one [`NetworkSample`] per observed tick (in capture
/// order — the engine emits one per tick after the network phase).
/// Cross-run aggregation reads `terminal_*` for the post-mortem and
/// `samples` for survival-curve-style overlays.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkReport {
    pub network: NetworkId,
    /// Number of nodes the static topology declared. Edge counts are
    /// not carried because the report is rendered against the live
    /// scenario, which knows the full edge set; the analyst reads
    /// "N/M nodes connected" against an authoritative denominator.
    pub static_node_count: u32,
    pub static_edge_count: u32,
    /// Per-tick network samples in capture order. Empty when the
    /// engine did not record any (e.g. zero-tick run).
    #[serde(default)]
    pub samples: Vec<NetworkSample>,
    /// Set of disrupted nodes at run end. Sorted for deterministic
    /// output; `BTreeSet` is canonical here for the same reason
    /// `BTreeMap` is used elsewhere.
    #[serde(default)]
    pub terminal_disrupted_nodes: std::collections::BTreeSet<NodeId>,
    /// Per-edge runtime capacity multipliers at run end. Edges absent
    /// from the map were never modified (multiplier = 1.0).
    #[serde(default)]
    pub terminal_edge_factors: BTreeMap<EdgeId, f64>,
    /// Per-faction set of nodes infiltrated by run end. Empty inner
    /// set for factions with no infiltrations is elided; outer map
    /// stays empty when no infiltration occurred.
    #[serde(default)]
    pub terminal_infiltrated: BTreeMap<FactionId, std::collections::BTreeSet<NodeId>>,
}

/// One per-tick observation of a network's connectivity state.
///
/// Captured *after* event effects fire so a same-tick interdiction
/// shows up in this tick's sample (mirrors how `metric_history` stores
/// end-of-tick escalation snapshots).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkSample {
    pub tick: u32,
    /// Number of weakly connected components (treating the graph as
    /// undirected for resilience purposes). `1` = fully connected;
    /// rises as the network fragments.
    pub component_count: u32,
    /// Largest weakly-connected-component size (node count).
    pub largest_component: u32,
    /// Total residual capacity = sum of `capacity * runtime_factor`
    /// across edges where neither endpoint is disrupted.
    pub residual_capacity: f64,
    /// Count of currently-disrupted nodes.
    pub disrupted_nodes: u32,
}

/// End-of-run snapshot of a single defender role's queue activity.
///
/// Pure summary: derived from the per-tick state the engine kept on
/// `DefenderQueueState`. The values that matter for cross-run
/// aggregation are the rate-style fields (utilization, dropped,
/// shadow_detections) and the timing field (`time_to_saturation`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderQueueReport {
    pub faction: FactionId,
    pub role: DefenderRoleId,
    /// Capacity carried through for downstream rendering — the analyst
    /// reads "depth N of capacity C" rather than digging up the schema.
    pub capacity: u32,
    /// Final tick's queue depth.
    pub final_depth: u32,
    /// Mean depth over the run (`total_depth_sum / ticks_observed`).
    pub mean_depth: f64,
    /// Maximum depth observed at any tick.
    pub max_depth: u32,
    /// `mean_depth / capacity`, clamped to `[0, 1]`. Convenience for
    /// renderers that want utilization without dividing themselves.
    pub utilization: f64,
    /// Total items pushed onto the queue across the run.
    pub total_enqueued: u64,
    /// Total items the role serviced (subtracted by the rate model).
    pub total_serviced: u64,
    /// Total items dropped via [`OverflowPolicy::DropNew`] /
    /// [`OverflowPolicy::DropOldest`](crate::faction::OverflowPolicy).
    /// Always `0` under [`OverflowPolicy::Backlog`].
    pub total_dropped: u64,
    /// Tick at which the queue first hit `depth >= capacity`. `None`
    /// when it never saturated — the run is right-censored for the
    /// time-to-saturation distribution.
    pub time_to_saturation: Option<u32>,
    /// Detection rolls suppressed by saturation: rolls that the engine
    /// computed *would have fired* at the unattenuated probability but
    /// did not after multiplying by `saturated_detection_factor`.
    /// The "shadow" name is field-standard in queueing-theory writing
    /// for missed-event rates under load.
    pub shadow_detections: u32,
    /// Items that arrived on this queue via cross-role escalation
    /// (Epic D round-three item 3 — multi-front resource contention).
    /// Tracks the chain link from upstream: the count delivered here
    /// from another saturated role's overflow, independent of how
    /// much of it then further spills downstream. Pairs with the
    /// upstream role's `spillover_out` for the conservation
    /// invariant `A.spillover_out == B.spillover_in`. Always `0`
    /// for scenarios that do not declare `overflow_to` anywhere on
    /// the faction. When this role itself further spills,
    /// `spillover_in` may exceed `total_enqueued` (the further-
    /// spilled portion arrived but never entered this queue's
    /// policy).
    #[serde(default)]
    pub spillover_in: u64,
    /// Items this queue redirected to its `overflow_to` target
    /// rather than enqueueing (Epic D round-three item 3). Not
    /// counted in `total_enqueued`. A non-zero value here paired
    /// with a zero on the target's `spillover_in` would indicate a
    /// chain that lost work past its hard recursion-depth guard —
    /// the report renders both columns side-by-side so the analyst
    /// can audit chain-conservation by inspection.
    #[serde(default)]
    pub spillover_out: u64,
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
    /// Output-output Pearson correlation matrix over per-run scalars
    /// (duration, casualties, attacker spend, …). `None` when fewer
    /// than two runs exist or no scalar metric varies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_matrix: Option<CorrelationMatrix>,
    /// Pareto frontier across runs over (attacker cost, success,
    /// stealth). Each entry is a non-dominated run; the rest of the
    /// runs sit "behind" the frontier on at least one axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pareto_frontier: Option<ParetoFrontier>,
    /// Per-(faction, role) defender-capacity queue analytics aggregated
    /// across runs. Empty when no scenario faction declares
    /// `defender_capacities`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defender_capacity: Vec<DefenderCapacitySummary>,
    /// Per-network resilience aggregate across runs. Empty
    /// when the scenario declares no networks. Holds the
    /// critical-node ranking (deterministic Brandes betweenness over
    /// the static topology) plus mean / max disrupted-node and
    /// fragmentation counts across runs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub network_summaries: BTreeMap<NetworkId, NetworkSummary>,
    /// Per-alliance-rule fracture analytics aggregated across runs.
    /// `None` when no scenario faction declares
    /// an `alliance_fracture` rule — the report section elides
    /// entirely in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alliance_dynamics: Option<AllianceDynamics>,
    /// Per-faction supply-pressure aggregate across runs. Empty when
    /// the scenario declares no
    /// active supply networks — the report section elides in that
    /// case. Keyed by `FactionId` for deterministic rendering.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub supply_pressure_summaries: BTreeMap<FactionId, SupplyPressureSummary>,
    /// Per-segment civilian-activation aggregate across runs. Empty
    /// when the scenario declares no
    /// `population_segments` or none ever activated — the report
    /// section elides in that case. Keyed by `SegmentId` for
    /// deterministic rendering.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub civilian_activation_summaries: BTreeMap<SegmentId, SegmentActivationSummary>,
    /// Per-faction tech-card cost aggregate across runs. Empty when
    /// no faction's `tech_access` ever produced a [`TechCostReport`]
    /// — the report section elides on that signal. Keyed by
    /// `FactionId` for deterministic rendering.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tech_cost_summaries: BTreeMap<FactionId, TechCostSummary>,
    /// Calibration verdict against the scenario's `historical_analogue`.
    /// `None` when the scenario declares no analogue ("purely synthetic"
    /// — the report section emits a synthetic-scenario disclaimer in
    /// that case). When `Some`, contains a per-observation verdict plus
    /// a roll-up Pass/Marginal/Fail count. See
    /// `faultline_stats::calibration` for the producer and
    /// `faultline_stats::report::calibration` for the renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<CalibrationReport>,
}

/// Calibration verdict against a scenario's `historical_analogue`.
///
/// One row per declared `HistoricalObservation`, plus a roll-up. Computed
/// in `faultline_stats::calibration::compute_calibration` from
/// `(scenario, runs, summary)` — pure post-processing of the run set,
/// no engine re-runs, so the report's `output_hash` is fully determined
/// by the manifest `(scenario, seed, num_runs)` triple.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationReport {
    /// Copy of the analogue's `name` field for report attribution.
    /// Duplicated here so the report renderer doesn't have to also hold
    /// onto the source `Scenario` to produce a section header.
    pub analogue_name: String,
    /// Per-observation calibration row, in the order the analogue
    /// declared them. Length equals the analogue's `observations.len()`.
    pub observations: Vec<ObservationCalibration>,
    /// Roll-up across observations.
    pub overall: CalibrationVerdict,
}

/// Per-observation calibration result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservationCalibration {
    /// Human-readable label describing the metric, e.g.
    /// `"winner = blue"` or `"duration_ticks ∈ [5, 12]"`. Generated by
    /// the calibration producer; the renderer uses this as the row
    /// label so it doesn't need to re-format the source enum.
    pub label: String,
    /// What the engine produced for this observation, formatted for
    /// human reading. e.g. `"blue: 73.0% [Wilson 65.1% – 79.6%]"` or
    /// `"coverage 64% (mean 8.1, σ 2.3)"`. Renderer-friendly: the
    /// producer takes the small additional cost of formatting once so
    /// the renderer can stay shape-agnostic.
    pub mc_summary: String,
    /// Author's confidence in the historical record itself for this
    /// observation, copied through from the source. `None` when the
    /// author didn't tag confidence — the renderer shows "—" in that
    /// case rather than implying any particular level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_confidence: Option<ConfidenceLevel>,
    /// The verdict for this observation alone.
    pub verdict: CalibrationVerdict,
    /// Free-form notes copied through from the source observation.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// Coarse pass/marginal/fail tag.
///
/// `Marginal` exists because Pass/Fail with no middle bucket would force
/// every "MC outcome is in the right neighbourhood but not exactly
/// matching" case to choose a side, which loses information. The exact
/// thresholds are defined per-metric in
/// `faultline_stats::calibration` — Winner uses MC mass on the observed
/// faction, WinRate uses Wilson-CI overlap with the historical interval,
/// DurationTicks uses the fraction of MC runs falling in the historical
/// interval. See the module's docstring for the full ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationVerdict {
    Pass,
    Marginal,
    Fail,
}

/// Cross-run civilian-activation analytics for one population segment.
///
/// Aggregates [`CivilianActivationEvent`] rows across the Monte Carlo
/// batch. `activation_count` is the number of runs in which the
/// segment ever activated (capped at `n_runs` because activation is
/// one-shot per run by the engine's latch on `PopulationSegment.activated`).
/// `mean_activation_tick` is `None` when the segment never activated
/// in any run — the rate-style fields stay defined (`activation_count`
/// is `0`, `activation_rate` is `0.0`) so the report row can render
/// without a special case for "no fires."
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SegmentActivationSummary {
    pub segment: SegmentId,
    /// Free-text label copied from `PopulationSegment.name`. Empty
    /// only if the author left the field blank.
    #[serde(default)]
    pub name: String,
    /// Author-supplied faction the segment is most strongly aligned
    /// with at activation time. Sympathy drift can drive different
    /// runs to different favored factions; this field reflects the
    /// *modal* favored faction across the run set, with ties resolved
    /// to the lexicographically largest `FactionId` (deterministic
    /// consequence of `Iterator::max_by_key` keeping the last maximum
    /// on a `BTreeMap`-ordered iteration). When no run activated the
    /// segment, falls back to the highest-sympathy faction declared
    /// on the segment so the report row still names a representative
    /// beneficiary.
    pub favored_faction: FactionId,
    /// Total runs in the batch.
    pub n_runs: u32,
    /// Number of runs in which this segment activated at any tick.
    /// Bounded by `n_runs` since the engine's `activated` latch makes
    /// activation one-shot per run.
    pub activation_count: u32,
    /// `activation_count / n_runs`, in `[0, 1]`.
    pub activation_rate: f64,
    /// Mean tick of activation across runs that activated. `None`
    /// when `activation_count == 0` (any value would be misleading
    /// — empty support).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_activation_tick: Option<f64>,
    /// Per-action-variant firing counts across all activations in the
    /// batch. Keyed by the [`crate::politics::CivilianAction`]
    /// discriminant name. Sums to `activation_count *
    /// activation_actions.len()` modulo any duplicates the author
    /// wrote into the same segment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_kind_counts: BTreeMap<String, u32>,
}

/// Cross-run supply-pressure analytics for one faction.
///
/// Each scalar aggregates the corresponding per-run scalar on
/// [`SupplyPressureReport`]:
/// - `mean_of_means` is the mean across runs of `mean_pressure` —
///   the typical operating supply level.
/// - `mean_of_mins` is the mean across runs of `min_pressure` — the
///   typical worst-case dip.
/// - `worst_min` is the smallest `min_pressure` observed in any
///   single run — the "how bad can it get" tail.
/// - `mean_pressured_ticks` is the mean across runs of
///   `pressured_ticks` — the typical duration of meaningful stress.
/// - `runs_with_any_pressure` is the number of runs where supply
///   ever dipped below the reporting threshold; divide by `n_runs`
///   for the "fraction of runs under stress" rate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupplyPressureSummary {
    pub faction: FactionId,
    pub n_runs: u32,
    pub mean_of_means: f64,
    pub mean_of_mins: f64,
    pub worst_min: f64,
    pub mean_pressured_ticks: f64,
    pub runs_with_any_pressure: u32,
}

/// Cross-run tech-card cost analytics for one faction.
///
/// Aggregates per-run [`TechCostReport`] rows across the Monte Carlo
/// batch. Three concerns are surfaced separately because they answer
/// different design questions:
/// - `mean_total_spend` — the typical total tech burn (deployment +
///   maintenance) per run. Useful for budget sizing.
/// - `runs_with_denial` — how often the faction's `tech_access` roster
///   exceeded what it could afford to deploy. A non-zero rate is a
///   diagnostic — either the roster is over-spec'd for the starting
///   resources or the faction is meant to make hard choices.
/// - `runs_with_decommission` and `mean_decommissions_per_run` — how
///   often (and how many) deployed cards collapsed mid-run from
///   maintenance starvation. A non-zero rate signals the faction's
///   `resource_rate` can't sustain the active roster.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TechCostSummary {
    pub faction: FactionId,
    pub n_runs: u32,
    pub mean_deployment_spend: f64,
    pub mean_maintenance_spend: f64,
    pub mean_total_spend: f64,
    pub runs_with_denial: u32,
    pub runs_with_decommission: u32,
    pub mean_decommissions_per_run: f64,
}

/// Cross-run alliance-fracture analytics.
///
/// Each [`FractureRuleSummary`] aggregates one declared rule across
/// the full Monte Carlo batch. `final_stance_distribution` reports
/// how many runs ended at each terminal stance for the
/// (faction, counterparty) pair; the sum across stances equals
/// `n_runs` because every run contributes exactly one terminal
/// stance — including the scenario's authored baseline for runs
/// where the rule never fired.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllianceDynamics {
    pub rules: Vec<FractureRuleSummary>,
}

/// Aggregate fracture analytics for one declared rule across runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FractureRuleSummary {
    pub faction: FactionId,
    pub counterparty: FactionId,
    pub rule_id: String,
    /// Free-text label copied from `FractureRule.description`. Empty
    /// when the author didn't supply one.
    #[serde(default)]
    pub description: String,
    /// Total runs in the batch. Stored explicitly so the report can
    /// render `fire_count / n_runs` without consulting the parent
    /// summary.
    pub n_runs: u32,
    /// Number of runs where this rule fired at any tick.
    pub fire_count: u32,
    /// `fire_count / n_runs`, in `[0, 1]`.
    pub fire_rate: f64,
    /// Mean tick of firing across runs that fired. `None` when the
    /// rule never fired in any run (so any value would be misleading
    /// — empty support).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_fire_tick: Option<f64>,
    /// Tick of firing in each run that fired, in ascending order.
    /// Empty when `fire_count == 0`. Surfaced so the report can
    /// render percentile timing if it wants to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fire_ticks: Vec<u32>,
    /// Distribution over terminal stance at run end. Sums to
    /// `n_runs`. Stances absent from the map have count zero.
    pub final_stance_distribution: BTreeMap<Diplomacy, u32>,
}

/// Aggregate per-run network analytics.
///
/// Mean / max stats are over [`RunResult::network_reports`]; the
/// `critical_nodes` ranking is computed once over the static topology
/// (it doesn't depend on runtime mutations) and surfaced so reports
/// can call out structural single points of failure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkSummary {
    pub network: NetworkId,
    pub n_runs: u32,
    pub mean_disrupted_nodes: f64,
    pub max_disrupted_nodes: u32,
    pub mean_terminal_components: f64,
    pub max_terminal_components: u32,
    /// Fraction of runs that ended with at least one node disrupted.
    pub fragmentation_rate: f64,
    /// Top-N nodes by Brandes betweenness centrality on the
    /// static topology. Sorted by descending centrality. Length
    /// is bounded (currently min(10, node_count)) so the report
    /// stays readable on dense networks.
    #[serde(default)]
    pub critical_nodes: Vec<CriticalNode>,
}

/// One row of the critical-node ranking.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CriticalNode {
    pub node: NodeId,
    pub name: String,
    /// Brandes betweenness score on the static topology, treating
    /// the graph as **undirected** (the score answers "removing this
    /// node disconnects how many shortest paths regardless of flow
    /// direction"). Normalized by `(n - 1) * (n - 2)`, the standard
    /// undirected betweenness denominator (matches NetworkX
    /// `betweenness_centrality(normalized=True)`). Range `[0, 1]`;
    /// `1.0` is achieved only by a node that lies on every
    /// non-trivial shortest path (e.g., the centre of an undirected
    /// star).
    pub betweenness: f64,
    /// Author-supplied criticality multiplier (`NetworkNode.criticality`).
    /// Surfaced alongside betweenness so the report can show "most
    /// structurally central" and "most analytically important" together.
    pub criticality: f64,
}

/// Aggregate queue analytics for one (faction, role) defender across
/// the full Monte Carlo batch.
///
/// `mean_*` / `max_*` aggregate the per-run scalars on
/// [`DefenderQueueReport`]. [`TimeToSaturation`] is right-censored:
/// runs that never saturated count toward `right_censored` rather than
/// being treated as instant or infinite saturation, which would bias
/// the descriptive stats either way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderCapacitySummary {
    pub faction: FactionId,
    pub role: DefenderRoleId,
    pub capacity: u32,
    /// Number of runs that contributed a queue report (always equal
    /// to `total_runs` for a scenario that defines this role).
    pub n_runs: u32,
    /// Mean of `utilization` across runs.
    pub mean_utilization: f64,
    /// Maximum `utilization` observed in any single run.
    pub max_utilization: f64,
    /// Mean of `max_depth` across runs.
    pub mean_max_depth: f64,
    /// Mean of `total_dropped` across runs.
    pub mean_dropped: f64,
    /// Mean of `shadow_detections` across runs.
    pub mean_shadow_detections: f64,
    /// Time-to-saturation distribution across runs (right-censored).
    pub time_to_saturation: TimeToSaturation,
    /// Mean of `DefenderQueueReport.spillover_in` across runs (Epic D
    /// round-three item 3 — multi-front resource contention). Captures
    /// the average per-run inbound escalation pressure on this role.
    /// `0.0` for roles in scenarios that do not declare any
    /// cross-role `overflow_to`.
    #[serde(default)]
    pub mean_spillover_in: f64,
    /// Mean of `DefenderQueueReport.spillover_out` across runs.
    /// Captures how much escalation pressure this role passed on to
    /// its overflow target on average. `0.0` for roles without
    /// `overflow_to`.
    #[serde(default)]
    pub mean_spillover_out: f64,
}

/// Time-to-saturation distribution for one defender role.
///
/// Mirrors the right-censored shape of
/// [`TimeToFirstDetection`] — runs that never saturated are counted in
/// `right_censored` so the mean / percentiles on `stats` describe only
/// the runs that actually hit capacity. A run-set with all
/// `right_censored == n_runs` produces `stats: None`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeToSaturation {
    pub saturated_runs: u32,
    pub right_censored: u32,
    pub samples: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<DistributionStats>,
}

/// Pearson correlation matrix over a fixed list of per-run scalar
/// outputs. Square and symmetric; the diagonal is always `Some(1.0)`
/// (modulo floating-point noise on degenerate samples).
///
/// Entries are `None` when one of the two series has zero variance —
/// this is mathematically "undefined correlation" and we surface it
/// explicitly rather than fudging to 0.0. `Option<f64>` is used over a
/// raw `f64` because `serde_json` round-trips NaN as `null` then fails
/// to deserialize back into `f64`, which would break
/// [`super::manifest::summary_hash`] and any external tooling reading
/// `summary.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrelationMatrix {
    /// Display labels in row / column order.
    pub labels: Vec<String>,
    /// Row-major `len() == labels.len() * labels.len()`. Index
    /// `[i * n + j]` is `corr(labels[i], labels[j])`. `None` means the
    /// correlation is undefined (zero variance on at least one input).
    pub values: Vec<Option<f64>>,
    /// Number of runs the correlations were computed over.
    pub n: u32,
}

/// Pareto frontier over per-run (attacker_cost, success, stealth).
///
/// For each run we project to a 3-tuple where:
/// - `attacker_cost` = sum of `attacker_spend` across all chain reports
///   (analyst minimizes)
/// - `success` = fraction of chains where any phase succeeded
///   (analyst maximizes)
/// - `stealth` = `1 - max chain detection rate per run` — i.e. zero
///   alerted chains scores 1.0, fully alerted scores 0.0 (analyst
///   maximizes)
///
/// A run dominates another if it is no worse on every axis and
/// strictly better on at least one. The frontier is the set of
/// non-dominated runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParetoFrontier {
    /// Frontier entries sorted by ascending `attacker_cost` for stable
    /// rendering.
    pub points: Vec<ParetoPoint>,
    /// Number of runs scanned to build the frontier.
    pub total_runs: u32,
}

/// One non-dominated run on the (cost, success, stealth) frontier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParetoPoint {
    pub run_index: u32,
    pub attacker_cost: f64,
    /// Fraction of chains in which any phase succeeded. `[0, 1]`.
    pub success: f64,
    /// `1 - max(chain detection accumulation)` per run. `[0, 1]`.
    pub stealth: f64,
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
    /// Distribution of *first-detection time* across runs that detected
    /// the chain at all. Runs where the defender was never alerted are
    /// **not** included — they show up as `right_censored` instead, so
    /// the mean / percentiles are not biased downward by treating
    /// non-detections as instant detections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_detection: Option<TimeToFirstDetection>,
    /// Distribution of *defender exposure time* — the gap between the
    /// first detection event and the run's terminal tick. Captures how
    /// much uncovered runway the operation kept after being seen.
    /// `None` when no runs detected the chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender_reaction_time: Option<DefenderReactionTime>,
    /// Per-phase Kaplan-Meier survival estimate for time-to-resolution,
    /// where "event" means any terminal status (success / failure /
    /// detection) and runs that never reach the phase are right-censored
    /// at the run's final tick.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub phase_survival: BTreeMap<PhaseId, KaplanMeierCurve>,
}

/// Time-to-first-detection summary for one kill chain.
///
/// Detection time is measured in ticks from the start of the run to the
/// first phase that transitioned to `PhaseOutcome::Detected`. A run
/// where the defender was never alerted contributes to `right_censored`
/// only — it does *not* appear in `samples` or skew the mean.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeToFirstDetection {
    /// Number of runs that detected the chain (the support of the
    /// distribution stats).
    pub detected_runs: u32,
    /// Number of runs where the defender was never alerted. These are
    /// right-censored at the run's `final_tick`.
    pub right_censored: u32,
    /// Tick of first detection in each detected run, sorted ascending.
    pub samples: Vec<u32>,
    /// Descriptive stats over `samples`. `None` when no runs detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<DistributionStats>,
}

/// Defender exposure / reaction-time summary for one kill chain.
///
/// "Reaction time" here is the count of ticks between the first
/// detection event and the run's terminal tick. It is *not* the time
/// the defender took to *act* (the engine doesn't yet model an explicit
/// defender response action) — it is the window of post-detection
/// runway the attacker had to keep operating. A long mean reaction
/// time means detection arrived too late to be useful; a short one
/// means the chain was caught near its end either way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderReactionTime {
    /// Number of runs that contributed a reaction-time sample.
    pub detected_runs: u32,
    /// Per-detected-run gap (ticks) from first detection to run end.
    pub samples: Vec<u32>,
    /// Descriptive stats over `samples`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<DistributionStats>,
}

/// Kaplan-Meier survival curve for time-to-event over Monte Carlo runs.
///
/// `S(t)` is the probability that a phase has *not* yet resolved by
/// tick `t`. Runs that ended without the phase resolving (still
/// `Pending` or `Active`) contribute as right-censored observations at
/// the run's final tick — they reduce the at-risk set but do not count
/// as events. The implicit value before the first event is `S = 1.0`;
/// `survival[i]` records `S(t_i)` *after* applying the i-th event.
/// `cumulative_hazard[i]` is `-ln(survival[i])`, or `None` when
/// `survival[i] == 0` (hazard is mathematically infinite — `Option`
/// avoids the JSON-roundtrip pitfall where `f64::INFINITY` serializes
/// as `null` and then fails to deserialize back into `f64`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KaplanMeierCurve {
    /// Distinct event times where the curve steps, sorted ascending.
    pub times: Vec<u32>,
    /// `S(t)` at each event time, *after* the step. The pre-event
    /// value is implicitly `1.0`.
    pub survival: Vec<f64>,
    /// Cumulative hazard `H(t) = -ln(S(t))`. `None` at indices where
    /// `S` has hit zero (`H` is infinite there).
    pub cumulative_hazard: Vec<Option<f64>>,
    /// Number of events (terminal resolutions) at each step.
    pub events: Vec<u32>,
    /// Number of runs at risk *just before* each event time.
    pub at_risk: Vec<u32>,
    /// Total number of right-censored observations across all event
    /// times (runs that ended with the phase still pending).
    pub censored: u32,
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
    /// Defender-queue summaries — preserved verbatim, not delta-encoded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defender_queue_reports: Vec<DefenderQueueReport>,
    /// Network reports — preserved verbatim, not delta-encoded.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub network_reports: BTreeMap<NetworkId, NetworkReport>,
    /// Alliance-fracture event log — preserved verbatim. Empty when
    /// no scenario faction declares an `alliance_fracture` block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fracture_events: Vec<FractureEvent>,
    /// Per-faction supply-pressure summary — preserved verbatim. Empty
    /// when the scenario declares no `kind = "supply"` networks.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub supply_pressure_reports: BTreeMap<FactionId, SupplyPressureReport>,
    /// Civilian-segment activation log — preserved verbatim. Empty
    /// when the scenario declares no `population_segments` or none
    /// activated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub civilian_activations: Vec<CivilianActivationEvent>,
    /// Per-faction tech-card cost activity — preserved verbatim. Empty
    /// when no faction's tech roster engaged the cost mechanic.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tech_costs: BTreeMap<FactionId, TechCostReport>,
}
