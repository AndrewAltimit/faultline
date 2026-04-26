use std::collections::BTreeMap;
use std::fmt;

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

impl fmt::Display for TerrainType {
    /// Stable, human-readable label for report rendering. Decoupled
    /// from `Debug` so a future variant rename doesn't silently change
    /// user-facing report text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            TerrainType::Urban => "Urban",
            TerrainType::Suburban => "Suburban",
            TerrainType::Rural => "Rural",
            TerrainType::Forest => "Forest",
            TerrainType::Mountain => "Mountain",
            TerrainType::Desert => "Desert",
            TerrainType::Coastal => "Coastal",
            TerrainType::Riverine => "Riverine",
            TerrainType::Arctic => "Arctic",
        };
        f.write_str(label)
    }
}

/// A geographic coordinate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

// ---------------------------------------------------------------------------
// Environmental schedule (Epic D — weather, time-of-day)
// ---------------------------------------------------------------------------

/// A timeline of environmental windows that modify per-region terrain
/// effects and a global kill-chain detection multiplier.
///
/// Empty by default — scenarios with no environmental modeling pay
/// zero overhead and the engine path collapses to the previous
/// behavior (every multiplier resolves to 1.0).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentSchedule {
    /// Active windows, in declaration order. Multiple windows can be
    /// active simultaneously; their factors compose multiplicatively.
    pub windows: Vec<EnvironmentWindow>,
}

impl EnvironmentSchedule {
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

/// A single environmental window — when it is active and what it
/// modifies. All factors default to 1.0 (no effect) so omitting a
/// field is safe.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentWindow {
    /// Stable identifier used in report output and authoring tooling.
    pub id: String,
    pub name: String,
    /// When this window is active (tick-indexed).
    pub activation: Activation,
    /// Terrain types this window's per-terrain effects apply to.
    /// Empty = applies to every terrain type.
    #[serde(default)]
    pub applies_to: Vec<TerrainType>,
    /// Multiplicative modifier on `TerrainModifier.movement_modifier`.
    #[serde(default = "one_f64")]
    pub movement_factor: f64,
    /// Multiplicative modifier on `TerrainModifier.defense_modifier`.
    /// Read by the combat phase when resolving regional engagements.
    #[serde(default = "one_f64")]
    pub defense_factor: f64,
    /// Multiplicative modifier on `TerrainModifier.visibility`.
    #[serde(default = "one_f64")]
    pub visibility_factor: f64,
    /// Multiplicative modifier applied globally to every kill-chain
    /// phase's `detection_probability_per_tick`. Applied globally
    /// (not per-terrain) because kill chains are modeled as
    /// faction-vs-faction operations without a region; e.g. a
    /// `Night` window with `detection_factor = 0.6` reduces every
    /// active phase's detection roll by 40% across the scenario.
    #[serde(default = "one_f64")]
    pub detection_factor: f64,
}

fn one_f64() -> f64 {
    1.0
}

/// When an [`EnvironmentWindow`] is active.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Activation {
    /// Active on every tick (used for persistent climate effects).
    Always,
    /// Active when `start <= tick <= end` (inclusive).
    TickRange { start: u32, end: u32 },
    /// Repeating cycle — active when
    /// `(tick - phase) mod period < duration`. Useful for
    /// time-of-day cycles: `Cycle { period: 24, phase: 18,
    /// duration: 12 }` is night under an hourly tick (active hours
    /// 18..30 mod 24, i.e. 18:00–06:00).
    Cycle {
        period: u32,
        phase: u32,
        duration: u32,
    },
}

impl Activation {
    /// Whether this activation triggers at the given simulation tick.
    pub fn is_active_at(&self, tick: u32) -> bool {
        match self {
            Activation::Always => true,
            Activation::TickRange { start, end } => tick >= *start && tick <= *end,
            Activation::Cycle {
                period,
                phase,
                duration,
            } => {
                if *period == 0 || *duration == 0 {
                    return false;
                }
                // Promote to u64 so the inner sum never overflows: when
                // `*phase % p == 0` the term `p - *phase % p` equals `p`,
                // and `tick % p + p` would wrap u32 for `p > 2^31`.
                let p = u64::from(*period);
                let phase = u64::from(*phase);
                let tick = u64::from(tick);
                let shifted = (tick % p + (p - phase % p)) % p;
                shifted < u64::from(*duration)
            },
        }
    }
}
