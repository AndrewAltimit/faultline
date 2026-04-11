use serde::{Deserialize, Serialize};

use crate::ids::{FactionId, RegionId, SegmentId};
use crate::map::InfrastructureType;

/// The overall political environment of the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoliticalClimate {
    pub tension: f64,
    pub institutional_trust: f64,
    pub media_landscape: MediaLandscape,
    pub population_segments: Vec<PopulationSegment>,
    pub global_modifiers: Vec<ClimateModifier>,
}

/// Parameters describing the media environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaLandscape {
    pub fragmentation: f64,
    pub disinformation_susceptibility: f64,
    pub state_control: f64,
    pub social_media_penetration: f64,
    pub internet_availability: f64,
}

/// A demographic segment of the civilian population.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PopulationSegment {
    pub id: SegmentId,
    pub name: String,
    pub fraction: f64,
    pub concentrated_in: Vec<RegionId>,
    pub sympathies: Vec<FactionSympathy>,
    pub activation_threshold: f64,
    pub activation_actions: Vec<CivilianAction>,
    pub volatility: f64,
    #[serde(default)]
    pub activated: bool,
}

/// A segment's sympathy toward a particular faction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactionSympathy {
    pub faction: FactionId,
    pub sympathy: f64,
}

/// Actions civilians may take once activated.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum CivilianAction {
    NonCooperation {
        effectiveness_reduction: f64,
    },
    Protest {
        intensity: f64,
    },
    Intelligence {
        target_faction: FactionId,
        quality: f64,
    },
    MaterialSupport {
        target_faction: FactionId,
        rate: f64,
    },
    ArmedResistance {
        target_faction: FactionId,
        unit_strength: f64,
    },
    Flee {
        rate: f64,
    },
    Sabotage {
        target_infra_type: Option<InfrastructureType>,
        probability: f64,
    },
}

/// External or systemic modifiers to the political climate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "modifier")]
pub enum ClimateModifier {
    EconomicCrisis {
        severity: f64,
    },
    NaturalDisaster {
        region: RegionId,
        severity: f64,
    },
    InternationalPressure {
        target_faction: FactionId,
        intensity: f64,
    },
    HealthCrisis {
        severity: f64,
    },
    ElectionCycle {
        legitimacy_modifier: f64,
    },
}
