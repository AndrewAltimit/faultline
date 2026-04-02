use serde::{Deserialize, Serialize};

use crate::ids::TechCardId;
use crate::map::TerrainType;

/// A technology or capability that can be deployed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechCard {
    pub id: TechCardId,
    pub name: String,
    pub description: String,
    pub category: TechCategory,
    pub effects: Vec<TechEffect>,
    pub cost_per_tick: f64,
    pub deployment_cost: f64,
    pub countered_by: Vec<TechCardId>,
    pub terrain_modifiers: Vec<TerrainTechModifier>,
    pub coverage_limit: Option<u32>,
}

/// Categories of technology.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TechCategory {
    Surveillance,
    OffensiveDrone,
    CounterDrone,
    ElectronicWarfare,
    Cyber,
    Communications,
    InformationWarfare,
    Concealment,
    Logistics,
    Custom(String),
}

/// The effect a technology has when deployed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TechEffect {
    DetectionModifier { factor: f64 },
    CombatModifier { factor: f64 },
    InfraProtection { factor: f64 },
    MoraleEffect { target: MoraleTarget, delta: f64 },
    AreaDenial { strength: f64 },
    CommsDisruption { factor: f64 },
    AttritionModifier { factor: f64 },
    CivilianSentiment { delta: f64 },
    SupplyInterdiction { factor: f64 },
    IntelGain { probability: f64 },
    CounterTech { target: TechCardId, reduction: f64 },
}

/// Which group a morale effect targets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoraleTarget {
    Own,
    Enemy,
    Civilian,
    All,
}

/// How terrain modifies a technology's effectiveness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainTechModifier {
    pub terrain: TerrainType,
    pub effectiveness: f64,
}
