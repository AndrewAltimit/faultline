//! Multi-phase campaign / kill chain modeling.
//!
//! Real-world threat campaigns are not single kinetic engagements — they
//! are ordered sequences of phases (reconnaissance → emplacement →
//! credential harvest → kinetic action) where intelligence produced in
//! early phases modifies the success probability of later phases.
//!
//! This module provides the *configuration* types (defined in scenario
//! TOML). Runtime evolution lives in `faultline-engine::campaign`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{DefenderRoleId, FactionId, KillChainId, PhaseId, RegionId};
use crate::stats::ConfidenceLevel;

// ---------------------------------------------------------------------------
// Kill chain
// ---------------------------------------------------------------------------

/// A named, ordered sequence of [`CampaignPhase`]s targeting a faction.
///
/// Execution begins at `entry_phase`. Subsequent phases are reached by
/// resolving branches at phase completion; a phase with no branch
/// definitions ends the chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KillChain {
    pub id: KillChainId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Faction executing the campaign.
    pub attacker: FactionId,
    /// Faction being targeted.
    pub target: FactionId,
    pub entry_phase: PhaseId,
    pub phases: BTreeMap<PhaseId, CampaignPhase>,
}

// ---------------------------------------------------------------------------
// Campaign phase
// ---------------------------------------------------------------------------

/// A single phase within a kill chain.
///
/// A phase takes between `min_duration` and `max_duration` ticks from
/// activation. Each active tick independently rolls for detection
/// (accumulating exposure). At completion, the phase rolls success
/// against `base_success_probability`, modified by intelligence gained
/// from prerequisite phases.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignPhase {
    pub id: PhaseId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Prior phases that must succeed before this one can begin.
    #[serde(default)]
    pub prerequisites: Vec<PhaseId>,
    /// Baseline success probability in `[0.0, 1.0]`.
    pub base_success_probability: f64,
    pub min_duration: u32,
    pub max_duration: u32,
    /// Per-tick probability that the defender detects the operation
    /// while it is active. Accumulates over phase duration.
    #[serde(default)]
    pub detection_probability_per_tick: f64,
    /// Additive boost to success probability applied per successful
    /// prerequisite phase (clamped to `[0, 1]` after application).
    #[serde(default)]
    pub prerequisite_success_boost: f64,
    /// Attribution difficulty after the operation: `0.0` = trivially
    /// attributable (clear forensics), `1.0` = completely opaque.
    #[serde(default = "default_attribution")]
    pub attribution_difficulty: f64,
    /// Dollar-denominated cost annotations.
    #[serde(default)]
    pub cost: PhaseCost,
    /// Defensive domains whose gaps this phase targets.
    #[serde(default)]
    pub targets_domains: Vec<DefensiveDomain>,
    /// Effects applied to the world state on successful completion.
    #[serde(default)]
    pub outputs: Vec<PhaseOutput>,
    /// Branching logic for resolving the next phase.
    #[serde(default)]
    pub branches: Vec<PhaseBranch>,
    /// Author's self-assessment of the *parameter quality* for this
    /// phase (base rates, detection probability, attribution
    /// difficulty). Orthogonal to the Monte Carlo-derived confidence
    /// reported in `FeasibilityConfidence`, which reflects sampling
    /// stability. `None` = author has not rated it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_confidence: Option<ConfidenceLevel>,
    /// IWI / IOC library — observable indicators the defender could
    /// monitor for to detect this phase before completion. Declarative
    /// in this iteration: not consumed by the detection roll, but
    /// surfaced in the Countermeasure Analysis report section so
    /// analysts can reason about which observables a monitoring
    /// posture would have to cover.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warning_indicators: Vec<WarningIndicator>,
    /// Defender-side noise this phase generates while active. Each
    /// entry routes a Poisson-distributed stream of "tickets" (alerts,
    /// tips, samples to triage) into a named defender role's queue
    /// every active tick. Drives the alert-fatigue / FOIA-flood /
    /// forensic-backlog scenario classes (Epic K). Empty = legacy
    /// phase that does not enqueue defender work; the defender
    /// capacity machinery sees no traffic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defender_noise: Vec<DefenderNoise>,
    /// If set, the per-tick detection roll for this phase is multiplied
    /// by the named defender role's `saturated_detection_factor` when
    /// that role's queue is at full depth. Models the analytic premise
    /// behind alert fatigue: the SOC will catch a real signal *if*
    /// it has bandwidth to look at it. Distinct from `defender_noise`
    /// — a phase can saturate one role but be gated by another (e.g.
    /// volume attack overwhelms tier-1, while the real intrusion
    /// would have been caught by tier-2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gated_by_defender: Option<DefenderRoleRef>,
}

/// A stream of synthetic defender work this phase generates.
///
/// Items are sampled from a Poisson distribution with mean
/// `items_per_tick` *each active tick* and pushed into the named role's
/// queue. The queue's [`OverflowPolicy`](crate::faction::OverflowPolicy)
/// decides what happens when the queue is full.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderNoise {
    /// The faction whose defender capacity receives the work.
    pub defender: FactionId,
    /// Which role within the faction handles it.
    pub role: DefenderRoleId,
    /// Mean items enqueued per active tick (Poisson rate `>= 0`).
    pub items_per_tick: f64,
}

/// A reference to one (faction, role) defender capacity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderRoleRef {
    pub faction: FactionId,
    pub role: DefenderRoleId,
}

/// An indicator-and-warning entry attached to a campaign phase.
///
/// Models a single observable the defender could monitor for to catch
/// the adversary in this phase. The `detectability` field captures
/// how reliably the observable is picked up *if* the defender is
/// actually looking for it; `time_to_detect_ticks` captures the
/// latency from phase start to reliable detection. Both are aggregate
/// statistical descriptors expected to be sourceable from OSINT (e.g.
/// published MTTD figures for a given monitoring discipline).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WarningIndicator {
    /// Stable identifier (e.g. "beaconing_rf_emissions").
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Collection discipline required to see the observable.
    pub observable: ObservableDiscipline,
    /// Probability that an adequately-resourced monitor catches the
    /// observable *during* the phase, in `[0.0, 1.0]`.
    #[serde(default)]
    pub detectability: f64,
    /// Expected latency from phase activation to reliable detection,
    /// in simulation ticks. `None` = no published estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_detect_ticks: Option<u32>,
    /// Rough annual dollar cost of a monitoring posture that covers
    /// this observable, if the author has a sourceable estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitoring_cost_annual: Option<f64>,
}

/// Collection discipline a warning indicator requires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservableDiscipline {
    /// Signals intelligence — RF, network, wireless emissions.
    SIGINT,
    /// Human intelligence — sources, tips, insider reports.
    HUMINT,
    /// Open-source intelligence — public publications, social media.
    OSINT,
    /// Geospatial / imagery intelligence.
    GEOINT,
    /// Measurement and signature intelligence — non-imaging sensors,
    /// chemical / radiological / environmental.
    MASINT,
    /// Cyber intelligence — endpoint, network, or cloud telemetry.
    CYBINT,
    /// Financial intelligence — funds flow, procurement records.
    FININT,
    /// Physical inspection — on-site examination.
    Physical,
    /// Anything that does not fit the above.
    Custom(String),
}

fn default_attribution() -> f64 {
    0.5
}

// ---------------------------------------------------------------------------
// Cost annotation
// ---------------------------------------------------------------------------

/// Dollar-denominated costs associated with a phase.
///
/// Attacker costs represent investment required to execute the phase.
/// Defender costs represent investment required to *close* the gap the
/// phase exploits. The ratio `defender / attacker` is the cost
/// asymmetry measurement the threat-assessment framework targets.
///
/// Accumulation timing differs by side: `attacker_dollars` is charged
/// whether the phase succeeds or fails (the attacker pays for the
/// attempt either way), but `defender_dollars` is charged **only when
/// the phase succeeds** — the gap stays unclosed until something gets
/// through, so a defender that repels every attempt accrues zero
/// spend. Authors targeting the `defender_budget` overrun penalty
/// should size `defender_dollars` for the cost of plugging a *landed*
/// attack, not the cost of a per-attempt response.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PhaseCost {
    #[serde(default)]
    pub attacker_dollars: f64,
    #[serde(default)]
    pub defender_dollars: f64,
    /// Scenario resource units consumed from the attacker's pool
    /// (distinct from dollar accounting).
    #[serde(default)]
    pub attacker_resources: f64,
    /// Author's self-assessment of the dollar-cost defensibility.
    /// High = commodity parts / published rate cards; Low = expert
    /// estimate with wide uncertainty. `None` = unrated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceLevel>,
}

// ---------------------------------------------------------------------------
// Phase output
// ---------------------------------------------------------------------------

/// An effect applied when a phase completes successfully.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PhaseOutput {
    /// Intelligence gain that boosts subsequent phases beyond the
    /// baseline `prerequisite_success_boost`.
    IntelligenceGain {
        amount: f64,
    },
    /// Damage infrastructure in a specific region.
    InfraDamage {
        region: RegionId,
        factor: f64,
    },
    /// Change political tension by a delta.
    TensionDelta {
        delta: f64,
    },
    /// Change a faction's morale.
    MoraleDelta {
        faction: FactionId,
        delta: f64,
    },
    /// Non-kinetic outputs.
    InformationDominance {
        delta: f64,
    },
    InstitutionalErosion {
        delta: f64,
    },
    CoercionPressure {
        delta: f64,
    },
    PoliticalCost {
        delta: f64,
    },
    /// Generic custom metric for analysis output.
    Custom {
        key: String,
        value: f64,
    },
    /// Decapitate the named faction's top leader (Epic D).
    ///
    /// Advances the target faction's leadership rank index by one and
    /// applies a one-shot morale shock. During the
    /// `succession_recovery_ticks` window after the strike, the
    /// target's morale is capped at the new rank's
    /// recovery-interpolated effectiveness.
    ///
    /// No-op if the target has no [`LeadershipCadre`] declared
    /// (legacy factions cannot be decapitated). When the rank index
    /// advances past the end of the cadre, the faction enters a
    /// terminal "leaderless" state — effectiveness collapses and
    /// further decapitations are recorded but cannot drop the cap
    /// any further.
    LeadershipDecapitation {
        target_faction: FactionId,
        /// One-time morale drop applied to the target on the
        /// strike tick. Clamped to `[0.0, 1.0]` against current
        /// morale.
        #[serde(default)]
        morale_shock: f64,
    },
}

// ---------------------------------------------------------------------------
// Branching
// ---------------------------------------------------------------------------

/// A conditional transition from one phase to another.
///
/// The first matching branch wins. Evaluation order is the declared
/// vector order, which is preserved by serde on `Vec`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseBranch {
    pub condition: BranchCondition,
    pub next_phase: PhaseId,
}

/// Condition under which a branch is taken.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BranchCondition {
    /// Branch taken only if the phase succeeded.
    OnSuccess,
    /// Branch taken only if the phase failed outright.
    OnFailure,
    /// Branch taken only if the operation was detected while active.
    OnDetection,
    /// Branch taken with a fixed probability (rolled independently).
    Probability { p: f64 },
    /// Always take this branch (used as a terminal fallback).
    Always,
    /// Branch taken when a global metric (tension, information
    /// dominance, …) has stayed on the requested side of `threshold`
    /// for at least `sustained_ticks` consecutive ticks at the time of
    /// branch evaluation. The `sustained_ticks` requirement supplies
    /// hysteresis: a single-tick spike will not flip a branch in or
    /// out, which would otherwise produce extreme MC variance when a
    /// metric oscillates near a threshold.
    EscalationThreshold {
        metric: EscalationMetric,
        threshold: f64,
        direction: ThresholdDirection,
        sustained_ticks: u32,
    },
    /// Branch taken when **any** of the inner conditions match.
    ///
    /// Lets a single branch fire on multiple equivalent triggers
    /// (e.g. `OnDetection` OR `EscalationThreshold(Tension > 0.7)`)
    /// without having to declare two branches that point at the same
    /// `next_phase`. Inner conditions are evaluated short-circuit
    /// left-to-right, matching the declared `Vec` order.
    ///
    /// Nesting is allowed (an `OrAny` inside another `OrAny`) but
    /// rarely useful. An empty `conditions` vector is rejected at
    /// scenario validation since "OR over nothing" is ambiguous
    /// (vacuously false vs. an unfilled author template).
    OrAny { conditions: Vec<BranchCondition> },
}

/// Global metrics an [`BranchCondition::EscalationThreshold`] can read.
///
/// All metrics are clamped to `[0, 1]` by the engine on every tick (or
/// `[-1, 1]` for `InformationDominance`), so threshold values are
/// interpreted in those ranges. `Tension` reads
/// `political_climate.tension`; the rest read the corresponding
/// `non_kinetic` accumulator.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EscalationMetric {
    Tension,
    InformationDominance,
    InstitutionalErosion,
    CoercionPressure,
    PoliticalCost,
}

/// Which side of the threshold the metric must be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ThresholdDirection {
    /// Metric must be `>= threshold`.
    Above,
    /// Metric must be `<= threshold`.
    Below,
}

// ---------------------------------------------------------------------------
// Defensive domains
// ---------------------------------------------------------------------------

/// Categories of defensive discipline. The "seam" between two or more
/// of these — where no single organizational owner is responsible — is
/// the attack surface the seam-scoring model aims to quantify.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DefensiveDomain {
    PhysicalSecurity,
    NetworkSecurity,
    CounterUAS,
    ExecutiveProtection,
    CivilianEmergency,
    SignalsIntelligence,
    InsiderThreat,
    SupplyChainSecurity,
    Custom(String),
}
