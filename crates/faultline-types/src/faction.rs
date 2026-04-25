use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{FactionId, ForceId, InstitutionId, RegionId, TechCardId};
use crate::strategy::Doctrine;

/// A participant in the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Faction {
    pub id: FactionId,
    pub name: String,
    pub faction_type: FactionType,
    pub description: String,
    pub color: String,
    pub forces: BTreeMap<ForceId, ForceUnit>,
    pub tech_access: Vec<TechCardId>,
    pub initial_morale: f64,
    pub logistics_capacity: f64,
    pub initial_resources: f64,
    pub resource_rate: f64,
    pub recruitment: Option<RecruitmentConfig>,
    pub command_resilience: f64,
    pub intelligence: f64,
    pub diplomacy: Vec<DiplomaticStance>,
    #[serde(default)]
    pub doctrine: Doctrine,
    /// Declarative doctrine / rules-of-engagement contract describing
    /// how this faction is *permitted* to escalate. Reports surface the
    /// ladder in Policy Implications; the engine itself does not
    /// currently enforce these — they document the decision-maker's
    /// standing orders so analysts can see when a counterfactual
    /// assumes the faction would violate its own doctrine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_rules: Option<EscalationRules>,
}

/// A scenario-author-asserted escalation ladder for a faction.
///
/// Purely declarative in this iteration — the engine does not consult
/// it when selecting actions. Surfaced in reports so analysts can see
/// which counterfactuals implicitly require the faction to cross a
/// doctrinal threshold. A later engine iteration may enforce the
/// ladder, at which point this type becomes load-bearing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRules {
    /// One-line summary of the faction's doctrine / ROE stance.
    #[serde(default)]
    pub posture: String,
    /// Ordered rungs the faction is permitted to climb. Earlier rungs
    /// are lower escalation; later rungs are higher. `None` on
    /// `trigger_tension` = rung is a permanent standing posture.
    #[serde(default)]
    pub ladder: Vec<EscalationRung>,
    /// Tension level above which the faction will *not* voluntarily
    /// de-escalate without an external event. Declarative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub de_escalation_floor: Option<f64>,
}

/// A single rung on an escalation ladder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRung {
    /// Stable identifier (e.g. "grey_zone", "kinetic", "strategic").
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Political tension at or above which the faction is *authorized*
    /// to operate at this rung. `None` = always authorized (e.g. a
    /// peacetime-permitted information-ops posture).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_tension: Option<f64>,
    /// Actions the faction is permitted to take at this rung. Free
    /// text — authors describe capabilities (e.g. "kinetic strikes
    /// against military targets outside own territory").
    #[serde(default)]
    pub permitted_actions: Vec<String>,
    /// Actions explicitly prohibited at this rung. Useful for
    /// documenting red lines ("no strikes against nuclear
    /// infrastructure").
    #[serde(default)]
    pub prohibited_actions: Vec<String>,
}

/// What kind of faction this is.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FactionType {
    Government {
        institutions: BTreeMap<InstitutionId, Institution>,
    },
    Military {
        branch: MilitaryBranch,
    },
    Insurgent,
    Civilian,
    PrivateMilitary,
    Foreign {
        is_proxy: bool,
    },
}

/// A government institution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Institution {
    pub id: InstitutionId,
    pub name: String,
    pub institution_type: InstitutionType,
    pub loyalty: f64,
    pub effectiveness: f64,
    pub personnel: u64,
    pub fracture_threshold: Option<f64>,
}

/// Categories of government institutions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstitutionType {
    LawEnforcement,
    Intelligence,
    Judiciary,
    Legislature,
    Executive,
    NationalGuard,
    FederalAgency,
    FinancialRegulator,
    MediaRegulator,
    Custom(String),
}

/// Branches of military service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MilitaryBranch {
    Army,
    Navy,
    AirForce,
    Marines,
    SpaceForce,
    CoastGuard,
    Combined,
    Custom(String),
}

/// A deployable military or paramilitary unit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForceUnit {
    pub id: ForceId,
    pub name: String,
    pub unit_type: UnitType,
    pub region: RegionId,
    pub strength: f64,
    pub mobility: f64,
    pub force_projection: Option<ForceProjection>,
    pub upkeep: f64,
    pub morale_modifier: f64,
    pub capabilities: Vec<UnitCapability>,
}

/// Categories of military/paramilitary units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitType {
    Infantry,
    Mechanized,
    Armor,
    Artillery,
    AirSupport,
    Naval,
    SpecialOperations,
    CyberUnit,
    DroneSwarm,
    LawEnforcement,
    Militia,
    Logistics,
    AirDefense,
    ElectronicWarfare,
    Custom(String),
}

/// How a unit can project force beyond its region.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum ForceProjection {
    Airlift { capacity: f64 },
    Naval { range: f64 },
    StandoffStrike { range: f64, damage: f64 },
}

/// Special capabilities a unit may possess.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnitCapability {
    Garrison,
    Raid,
    Sabotage {
        effectiveness: f64,
    },
    Recon {
        range: f64,
        detection: f64,
    },
    Interdiction {
        range: f64,
    },
    AreaDenial {
        radius: f64,
    },
    CounterUAS {
        effectiveness: f64,
    },
    EW {
        jamming_range: f64,
        effectiveness: f64,
    },
    Cyber {
        attack: f64,
        defense: f64,
    },
    InfoOps {
        reach: f64,
        persuasion: f64,
    },
    Humanitarian {
        capacity: f64,
    },
}

/// Configuration for recruiting new units over time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecruitmentConfig {
    pub rate: f64,
    pub population_threshold: f64,
    pub unit_type: UnitType,
    pub base_strength: f64,
    pub cost: f64,
}

/// A faction's diplomatic posture toward another faction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiplomaticStance {
    pub target_faction: FactionId,
    pub stance: Diplomacy,
}

/// Levels of diplomatic relations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Diplomacy {
    War,
    Hostile,
    Neutral,
    Cooperative,
    Allied,
}
