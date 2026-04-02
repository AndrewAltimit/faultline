use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{FactionId, InfraId, RegionId};

/// Top-level map configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapConfig {
    pub source: MapSource,
    pub regions: BTreeMap<RegionId, Region>,
    pub infrastructure: BTreeMap<InfraId, InfrastructureNode>,
    pub terrain: Vec<TerrainModifier>,
}

/// How the map geometry is provided.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MapSource {
    BuiltIn { name: String },
    GeoJson { path: String },
    Grid { width: u32, height: u32 },
}

/// A named region on the map.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub name: String,
    pub population: u64,
    pub urbanization: f64,
    pub initial_control: Option<FactionId>,
    pub strategic_value: f64,
    pub borders: Vec<RegionId>,
    pub centroid: Option<GeoPoint>,
}

/// A piece of infrastructure located in a region.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InfrastructureNode {
    pub id: InfraId,
    pub name: String,
    pub region: RegionId,
    pub infra_type: InfrastructureType,
    pub criticality: f64,
    pub initial_status: f64,
    pub repairable: Option<u32>,
}

/// Categories of infrastructure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InfrastructureType {
    PowerGrid,
    Telecommunications,
    TransportHub,
    GovernmentBuilding,
    MediaStation,
    WaterSystem,
    FuelDepot,
    Hospital,
    SupplyChain,
    Internet,
}

/// Terrain overlay for a region.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainModifier {
    pub region: RegionId,
    pub terrain_type: TerrainType,
    pub movement_modifier: f64,
    pub defense_modifier: f64,
    pub visibility: f64,
}

/// Categories of terrain.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainType {
    Urban,
    Suburban,
    Rural,
    Forest,
    Mountain,
    Desert,
    Coastal,
    Riverine,
    Arctic,
}

/// A geographic coordinate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}
