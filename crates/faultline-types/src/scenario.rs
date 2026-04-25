use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::campaign::KillChain;
use crate::events::EventDefinition;
use crate::faction::Faction;
use crate::ids::{EventId, FactionId, KillChainId, TechCardId, VictoryId};
use crate::map::MapConfig;
use crate::politics::PoliticalClimate;
use crate::simulation::SimulationConfig;
use crate::stats::ConfidenceLevel;
use crate::tech::TechCard;
use crate::victory::VictoryCondition;

/// The complete definition of a simulation scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub meta: ScenarioMeta,
    pub map: MapConfig,
    pub factions: BTreeMap<FactionId, Faction>,
    pub technology: BTreeMap<TechCardId, TechCard>,
    pub political_climate: PoliticalClimate,
    pub events: BTreeMap<EventId, EventDefinition>,
    pub simulation: SimulationConfig,
    pub victory_conditions: BTreeMap<VictoryId, VictoryCondition>,
    /// Multi-phase kill chains. Optional — scenarios without
    /// campaign analysis omit this field entirely.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kill_chains: BTreeMap<KillChainId, KillChain>,
    /// Defender budget cap in dollars. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender_budget: Option<f64>,
    /// Attacker budget cap in dollars. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attacker_budget: Option<f64>,
}

/// Metadata about the scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioMeta {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
    /// Coarse author-supplied confidence tag for the scenario as a
    /// whole. Signals "this is a conceptual sketch" vs.
    /// "this is publication-ready rigor" to report readers. Orthogonal
    /// to the Wilson CIs on individual rates — those measure sampling
    /// uncertainty; this one measures parameter defensibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceLevel>,
}
