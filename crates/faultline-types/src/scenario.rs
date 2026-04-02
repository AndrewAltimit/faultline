use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::events::EventDefinition;
use crate::faction::Faction;
use crate::ids::{EventId, FactionId, TechCardId, VictoryId};
use crate::map::MapConfig;
use crate::politics::PoliticalClimate;
use crate::simulation::SimulationConfig;
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
}

/// Metadata about the scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioMeta {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
}
