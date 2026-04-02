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
