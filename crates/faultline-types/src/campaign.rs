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

use crate::ids::{FactionId, KillChainId, PhaseId, RegionId};
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
/// asymmetry measurement the ETRA framework targets.
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
