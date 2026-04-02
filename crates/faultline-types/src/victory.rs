use serde::{Deserialize, Serialize};

use crate::ids::{FactionId, RegionId, VictoryId};

/// A condition under which a faction can win.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VictoryCondition {
    pub id: VictoryId,
    pub name: String,
    pub faction: FactionId,
    pub condition: VictoryType,
}

/// The specific type of victory condition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum VictoryType {
    StrategicControl {
        threshold: f64,
    },
    MilitaryDominance {
        enemy_strength_below: f64,
    },
    HoldRegions {
        regions: Vec<RegionId>,
        duration: u32,
    },
    InstitutionalCollapse {
        trust_below: f64,
    },
    PeaceSettlement,
    Custom {
        variable: String,
        threshold: f64,
        above: bool,
    },
}
