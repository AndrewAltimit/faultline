use serde::{Deserialize, Serialize};

use crate::belief::{DeceptionPayload, IntelligencePayload};
use crate::faction::{Diplomacy, ForceUnit};
use crate::ids::{
    EdgeId, EventId, FactionId, InfraId, InstitutionId, NetworkId, NodeId, RegionId, SegmentId,
    TechCardId,
};

/// A scripted or conditional event that may fire during the sim.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventDefinition {
    pub id: EventId,
    pub name: String,
    pub description: String,
    pub earliest_tick: Option<u32>,
    pub latest_tick: Option<u32>,
    pub conditions: Vec<EventCondition>,
    pub probability: f64,
    pub repeatable: bool,
    pub effects: Vec<EventEffect>,
    pub chain: Option<EventId>,
    /// Counterfactual defender responses the scenario author wants
    /// surfaced in analysis. These are *declarative* alternatives the
    /// defender could take if the event fires — the engine does not
    /// auto-select one. Reports enumerate them for the Policy
    /// Implications section; the `--counterfactual event.<id>.option=<key>`
    /// CLI mode can activate one to compare against baseline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defender_options: Vec<DefenderOption>,
}

/// A counterfactual defender response bundled to an event.
///
/// Each option bundles a dollar cost (the investment the defender
/// would have to make *ahead of time* to hold this response at
/// readiness) and a set of modifying effects that *replace* the
/// event's default effects when the option is active. Options are
/// analytical — they exist to make "what if the defender had
/// pre-positioned X?" questions addressable without hand-editing TOML.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderOption {
    /// Stable identifier referenced by `--counterfactual event.<eid>.option=<key>`.
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Preparedness cost for holding this response at readiness.
    #[serde(default)]
    pub preparedness_cost: f64,
    /// Effects that replace the event's baseline `effects` when this
    /// option is selected. Empty = this option cancels the event.
    #[serde(default)]
    pub modifier_effects: Vec<EventEffect>,
}

/// Conditions that must hold for an event to fire.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "condition")]
pub enum EventCondition {
    RegionControl {
        region: RegionId,
        faction: FactionId,
        controlled: bool,
    },
    TensionAbove {
        threshold: f64,
    },
    TensionBelow {
        threshold: f64,
    },
    FactionStrengthAbove {
        faction: FactionId,
        threshold: f64,
    },
    FactionStrengthBelow {
        faction: FactionId,
        threshold: f64,
    },
    MoraleAbove {
        faction: FactionId,
        threshold: f64,
    },
    MoraleBelow {
        faction: FactionId,
        threshold: f64,
    },
    InstitutionLoyaltyBelow {
        institution: InstitutionId,
        threshold: f64,
    },
    InfraStatusBelow {
        infra: InfraId,
        threshold: f64,
    },
    EventFired {
        event: EventId,
        fired: bool,
    },
    TickAtLeast {
        tick: u32,
    },
    SegmentActivated {
        segment: SegmentId,
    },
    Expression {
        expr: String,
    },
}

/// Effects applied when an event fires.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "effect")]
pub enum EventEffect {
    DamageInfra {
        infra: InfraId,
        damage: f64,
    },
    MoraleShift {
        faction: FactionId,
        delta: f64,
    },
    LoyaltyShift {
        institution: InstitutionId,
        delta: f64,
    },
    InstitutionDefection {
        institution: InstitutionId,
        to_faction: FactionId,
    },
    SpawnUnits {
        faction: FactionId,
        units: Vec<ForceUnit>,
    },
    DestroyUnits {
        faction: FactionId,
        region: RegionId,
        damage: f64,
    },
    DiplomacyChange {
        faction_a: FactionId,
        faction_b: FactionId,
        new_stance: Diplomacy,
    },
    TensionShift {
        delta: f64,
    },
    SympathyShift {
        segment: SegmentId,
        faction: FactionId,
        delta: f64,
    },
    TechAccess {
        faction: FactionId,
        tech: TechCardId,
        grant: bool,
    },
    MediaEvent {
        narrative: String,
        credibility: f64,
        reach: f64,
        favors: Option<FactionId>,
    },
    ResourceChange {
        faction: FactionId,
        delta: f64,
    },
    Narrative {
        text: String,
    },
    /// Reduce an edge's effective capacity by multiplying its
    /// `runtime_capacity_factor` by `factor`. `factor < 1.0`
    /// models interdiction; `factor == 0.0` severs the edge.
    /// `factor > 1.0` is permitted (hardening / surge capacity) but
    /// the runtime factor is clamped to `[0, 4]` to prevent runaway
    /// authoring errors. Unknown network / edge ids are a no-op at
    /// runtime; engine validation rejects them at scenario load.
    NetworkEdgeCapacity {
        network: NetworkId,
        edge: EdgeId,
        factor: f64,
    },
    /// Mark a network node as disrupted — every edge
    /// incident to it is treated as severed (capacity factor 0) for
    /// metrics and resilience curves. The static schema is unchanged;
    /// the disruption is per-run runtime state. Repeated disruption
    /// of the same node is idempotent.
    NetworkNodeDisrupt {
        network: NetworkId,
        node: NodeId,
    },
    /// Add `faction` to the set of factions with attacker-style
    /// visibility into a network node. Surfaced in the
    /// report as an information loss; does not change capacity. The
    /// effect is cumulative — a second infiltration by a different
    /// faction adds that faction to the visibility set.
    NetworkInfiltrate {
        network: NetworkId,
        node: NodeId,
        faction: FactionId,
    },
    /// Add `magnitude` displaced fraction to `region` (Epic D round-three
    /// item 4 — refugee / displacement flows). `magnitude` is interpreted
    /// as a fraction-of-region-population delta in `[0, 1]`; the
    /// displacement phase clamps the resulting per-region total to that
    /// range and propagates it across adjacent regions every tick. Unknown
    /// regions are a no-op at runtime; engine validation rejects them at
    /// scenario load.
    Displacement {
        region: RegionId,
        magnitude: f64,
    },
    /// Plant a false belief in `target_faction`'s persistent belief
    /// state (Epic M round-one — belief asymmetry). The `source_faction`
    /// is the planting party (recorded for cross-run analytics —
    /// "which factions spread the most successful disinformation?");
    /// the `payload` describes the false fact. The deception is
    /// seamless from inside the simulation — the target's AI cannot
    /// distinguish a planted belief from a direct observation — but
    /// the resulting belief entry is tagged
    /// [`crate::belief::BeliefSource::Deceived`] so the post-run
    /// belief-asymmetry report can quantify how often the deception
    /// drove behavior. No-op when the scenario does not enable
    /// `simulation.belief_model.enabled`; validation rejects unknown
    /// faction / force / region references at scenario load.
    DeceptionOp {
        source_faction: FactionId,
        target_faction: FactionId,
        payload: DeceptionPayload,
    },
    /// Truthfully share intelligence from `source_faction` to
    /// `target_faction` — the target's belief is overwritten with the
    /// *current ground-truth* state of the referenced entity at full
    /// confidence (Epic M round-one). Models alliance-style intel
    /// sharing, captured prisoners, sympathetic third-party
    /// reporting. Unlike [`Self::DeceptionOp`], the resulting belief
    /// entry is tagged
    /// [`crate::belief::BeliefSource::DirectObservation`] —
    /// it's true at the moment of transfer. Subsequent ticks may stale
    /// it through normal decay if the target loses sight of the entity.
    /// No-op when `simulation.belief_model.enabled = false`; validation
    /// rejects unknown references at scenario load.
    IntelligenceShare {
        source_faction: FactionId,
        target_faction: FactionId,
        payload: IntelligencePayload,
    },
}
