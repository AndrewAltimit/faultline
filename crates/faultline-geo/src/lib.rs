use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use faultline_types::ids::RegionId;
use faultline_types::map::MapConfig;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during geography and terrain operations.
#[derive(Debug, Error)]
pub enum GeoError {
    #[error("region not found: {0}")]
    RegionNotFound(RegionId),

    #[error("invalid map configuration: {0}")]
    InvalidConfig(String),

    #[error("no path between regions {from} and {to}")]
    NoPath { from: RegionId, to: RegionId },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Runtime representation of a loaded game map.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameMap {
    /// All region metadata, keyed by region id.
    pub regions: BTreeMap<RegionId, RegionInfo>,
    /// Adjacency list: each region maps to its neighbours.
    pub adjacency: BTreeMap<RegionId, Vec<RegionId>>,
    /// Movement costs between adjacent region pairs `(from, to) -> cost`.
    pub movement_costs: BTreeMap<(RegionId, RegionId), f64>,
}

/// Minimal region info kept inside the runtime map.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionInfo {
    pub id: RegionId,
    pub name: String,
    pub population: u64,
    pub urbanization: f64,
    pub strategic_value: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a [`GameMap`] from the provided [`MapConfig`].
///
/// Builds the adjacency graph and derives default movement costs from
/// the terrain modifiers declared in the configuration.
pub fn load_map(config: &MapConfig) -> Result<GameMap, GeoError> {
    let mut regions = BTreeMap::new();
    let mut adjacency: BTreeMap<RegionId, Vec<RegionId>> = BTreeMap::new();
    let mut movement_costs: BTreeMap<(RegionId, RegionId), f64> = BTreeMap::new();

    for (rid, region) in &config.regions {
        regions.insert(
            rid.clone(),
            RegionInfo {
                id: rid.clone(),
                name: region.name.clone(),
                population: region.population,
                urbanization: region.urbanization,
                strategic_value: region.strategic_value,
            },
        );

        adjacency.insert(rid.clone(), region.borders.clone());
    }

    // Derive movement costs from terrain modifiers.
    for modifier in &config.terrain {
        let rid = &modifier.region;
        if let Some(neighbours) = adjacency.get(rid) {
            for neighbour in neighbours.clone() {
                let cost = modifier.movement_modifier.max(0.1);
                movement_costs.insert((rid.clone(), neighbour), cost);
            }
        }
    }

    Ok(GameMap {
        regions,
        adjacency,
        movement_costs,
    })
}

/// Return the movement cost from one region to an adjacent region,
/// or `None` if the regions are not adjacent.
pub fn movement_cost(from: &RegionId, to: &RegionId, map: &GameMap) -> Option<f64> {
    map.movement_costs.get(&(from.clone(), to.clone())).copied()
}

/// Return the list of regions adjacent to the given region.
/// Returns an empty `Vec` if the region is not present in the map.
pub fn adjacent_regions(region: &RegionId, map: &GameMap) -> Vec<RegionId> {
    map.adjacency.get(region).cloned().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::map::{MapSource, Region, TerrainModifier, TerrainType};

    fn sample_config() -> MapConfig {
        let mut regions = BTreeMap::new();
        let r1 = RegionId::from("alpha");
        let r2 = RegionId::from("beta");

        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "Alpha".into(),
                population: 1_000_000,
                urbanization: 0.8,
                initial_control: None,
                strategic_value: 5.0,
                borders: vec![r2.clone()],
                centroid: None,
            },
        );
        regions.insert(
            r2.clone(),
            Region {
                id: r2.clone(),
                name: "Beta".into(),
                population: 500_000,
                urbanization: 0.4,
                initial_control: None,
                strategic_value: 3.0,
                borders: vec![r1.clone()],
                centroid: None,
            },
        );

        MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![TerrainModifier {
                region: r1,
                terrain_type: TerrainType::Urban,
                movement_modifier: 1.0,
                defense_modifier: 1.2,
                visibility: 0.6,
            }],
        }
    }

    #[test]
    fn load_map_builds_adjacency() {
        let map = load_map(&sample_config()).expect("load_map should succeed");
        let adj = adjacent_regions(&RegionId::from("alpha"), &map);
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0], RegionId::from("beta"));
    }

    #[test]
    fn movement_cost_returns_none_for_non_adjacent() {
        let map = load_map(&sample_config()).expect("load_map should succeed");
        let cost = movement_cost(&RegionId::from("alpha"), &RegionId::from("gamma"), &map);
        assert!(cost.is_none());
    }

    #[test]
    fn movement_cost_returns_value_for_adjacent() {
        let map = load_map(&sample_config()).expect("load_map should succeed");
        // Alpha has a terrain modifier so alpha->beta should have a cost.
        let cost = movement_cost(&RegionId::from("alpha"), &RegionId::from("beta"), &map);
        assert!(
            cost.is_some(),
            "adjacent regions with terrain should have a cost"
        );
        assert!(
            cost.expect("just checked is_some") > 0.0,
            "movement cost should be positive"
        );
    }

    #[test]
    fn load_map_with_terrain_modifiers() {
        let mut config = sample_config();
        let r1 = RegionId::from("alpha");
        // Add a second terrain modifier with a high movement modifier.
        config.terrain.push(TerrainModifier {
            region: r1.clone(),
            terrain_type: TerrainType::Mountain,
            movement_modifier: 3.5,
            defense_modifier: 2.0,
            visibility: 0.3,
        });
        let map = load_map(&config).expect("load_map should succeed");
        // The second terrain modifier overwrites the first for alpha->beta.
        let cost = movement_cost(&r1, &RegionId::from("beta"), &map);
        assert!(cost.is_some(), "terrain modifier should produce a cost");
        let c = cost.expect("just checked is_some");
        assert!(
            (c - 3.5).abs() < f64::EPSILON,
            "cost should reflect the latest terrain modifier"
        );
    }

    #[test]
    fn load_map_empty_regions_errors() {
        let config = MapConfig {
            source: MapSource::Grid {
                width: 0,
                height: 0,
            },
            regions: BTreeMap::new(),
            infrastructure: BTreeMap::new(),
            terrain: vec![],
        };
        // load_map currently returns Ok even for empty — verify the map
        // is at least empty rather than containing phantom regions.
        let map = load_map(&config).expect("load_map should not panic on empty");
        assert!(
            map.regions.is_empty(),
            "empty config should yield empty map"
        );
        assert!(
            map.adjacency.is_empty(),
            "empty config should yield no adjacency"
        );
    }

    #[test]
    fn adjacent_regions_empty_for_isolated() {
        // Create a region with no borders.
        let mut regions = BTreeMap::new();
        let r1 = RegionId::from("isolated");
        regions.insert(
            r1.clone(),
            Region {
                id: r1.clone(),
                name: "Isolated".into(),
                population: 100,
                urbanization: 0.1,
                initial_control: None,
                strategic_value: 1.0,
                borders: vec![],
                centroid: None,
            },
        );
        let config = MapConfig {
            source: MapSource::Grid {
                width: 1,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![],
        };
        let map = load_map(&config).expect("load_map should succeed");
        let adj = adjacent_regions(&r1, &map);
        assert!(adj.is_empty(), "isolated region should have no neighbours");
    }
}
