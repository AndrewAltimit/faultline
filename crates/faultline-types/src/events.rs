use serde::{Deserialize, Serialize};

use crate::faction::{Diplomacy, ForceUnit};
use crate::ids::{EventId, FactionId, InfraId, InstitutionId, RegionId, SegmentId, TechCardId};

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
}
