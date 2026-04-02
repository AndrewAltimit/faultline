use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::faction::Diplomacy;
use crate::ids::{FactionId, ForceId, InfraId, InstitutionId, RegionId, TechCardId};

/// Weights and doctrine guiding a faction's AI decisions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StrategicPriority {
    pub survival_weight: f64,
    pub objective_weight: f64,
    pub opportunity_weight: f64,
    pub risk_aversion: f64,
    pub doctrine: Doctrine,
}

/// High-level military doctrine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Doctrine {
    Conventional,
    Guerrilla,
    Defensive,
    Disruption,
    CounterInsurgency,
    Blitzkrieg,
    Adaptive,
}

/// An action a faction can take during a tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum FactionAction {
    MoveUnit {
        force: ForceId,
        destination: RegionId,
    },
    Attack {
        force: ForceId,
        target_region: RegionId,
    },
    Defend {
        force: ForceId,
        region: RegionId,
    },
    DeployTech {
        tech: TechCardId,
        region: RegionId,
    },
    Recruit {
        region: RegionId,
    },
    DiplomacyProposal {
        proposal: DiplomacyProposal,
    },
    Sabotage {
        force: ForceId,
        target_infra: InfraId,
    },
    InfoOp {
        region: RegionId,
        narrative: String,
    },
}

/// A proposal to change diplomatic relations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiplomacyProposal {
    pub from: FactionId,
    pub to: FactionId,
    pub proposed_stance: Diplomacy,
}

/// What a faction can observe about the world (fog-of-war view).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactionWorldView {
    pub faction: FactionId,
    pub known_regions: BTreeMap<RegionId, Option<FactionId>>,
    pub detected_forces: Vec<DetectedForce>,
    pub infra_states: BTreeMap<InfraId, InfraState>,
    pub political_climate: PoliticalClimateView,
    pub diplomacy: BTreeMap<FactionId, Diplomacy>,
    pub morale: f64,
    pub resources: f64,
    pub tick: u32,
}

/// A force unit detected through intelligence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedForce {
    pub force_id: ForceId,
    pub faction: FactionId,
    pub region: RegionId,
    pub estimated_strength: f64,
    pub confidence: f64,
}

/// Observed state of an infrastructure node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InfraState {
    pub infra_id: InfraId,
    pub status: f64,
    pub controlled_by: Option<FactionId>,
}

/// A faction's partial view of the political climate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoliticalClimateView {
    pub tension: f64,
    pub institutional_trust: f64,
    pub civilian_sentiment: f64,
}

/// Runtime state for a faction during the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactionState {
    pub faction_id: FactionId,
    pub morale: f64,
    pub resources: f64,
    pub logistics_capacity: f64,
    pub tech_deployed: Vec<TechCardId>,
    pub controlled_regions: Vec<RegionId>,
    pub total_strength: f64,
    pub institution_loyalty: BTreeMap<InstitutionId, f64>,
}
