use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use faultline_types::faction::{ForceUnit, UnitType};
use faultline_types::ids::{
    EventId, FactionId, ForceId, InfraId, InstitutionId, RegionId, TechCardId,
};
use faultline_types::politics::PoliticalClimate;
use faultline_types::stats::StateSnapshot;

/// All mutable runtime state for a running simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationState {
    /// Current tick number (starts at 0).
    pub tick: u32,
    /// Per-faction runtime state.
    pub faction_states: BTreeMap<FactionId, RuntimeFactionState>,
    /// Which faction (if any) controls each region.
    pub region_control: BTreeMap<RegionId, Option<FactionId>>,
    /// Infrastructure health in `[0.0, 1.0]`.
    pub infra_status: BTreeMap<InfraId, f64>,
    /// Institution loyalty in `[0.0, 1.0]`.
    pub institution_loyalty: BTreeMap<InstitutionId, f64>,
    /// Political climate (cloned from scenario, mutated at runtime).
    pub political_climate: PoliticalClimate,
    /// Set of event IDs that have already fired (cumulative, for one-shot guard).
    pub events_fired: BTreeSet<EventId>,
    /// Events that fired during the current tick (cleared each tick).
    pub events_fired_this_tick: Vec<EventId>,
    /// Periodic state snapshots for analysis.
    pub snapshots: Vec<StateSnapshot>,
    /// Aggregated non-kinetic metrics. Summed across all
    /// in-flight kill chains so scenario victory conditions can check
    /// them uniformly.
    #[serde(default)]
    pub non_kinetic: NonKineticMetrics,
}

/// Non-kinetic outcome metrics.
///
/// Each field lives in `[0, 1]` and represents cumulative pressure or
/// damage along a dimension that is not directly measured by combat
/// or territorial control.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NonKineticMetrics {
    pub information_dominance: f64,
    pub institutional_erosion: f64,
    pub coercion_pressure: f64,
    pub political_cost: f64,
}

/// Runtime state tracked per faction during the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeFactionState {
    pub faction_id: FactionId,
    /// Aggregate military strength across all forces.
    pub total_strength: f64,
    /// Current morale in `[0.0, 1.0]`.
    pub morale: f64,
    /// Accumulated resources.
    pub resources: f64,
    /// Resource income per tick.
    pub resource_rate: f64,
    /// Logistics capacity.
    pub logistics_capacity: f64,
    /// Regions currently controlled.
    pub controlled_regions: Vec<RegionId>,
    /// Force units owned by this faction.
    pub forces: BTreeMap<ForceId, ForceUnit>,
    /// Tech cards currently deployed.
    pub tech_deployed: Vec<TechCardId>,
    /// Tracks how many consecutive ticks this faction has held
    /// specific regions (for HoldRegions victory conditions).
    pub region_hold_ticks: BTreeMap<RegionId, u32>,
    /// Whether this faction has been eliminated (strength = 0
    /// and no recruitment possible).
    pub eliminated: bool,
}

impl RuntimeFactionState {
    /// Recompute `total_strength` from the force roster.
    pub fn recompute_strength(&mut self) {
        self.total_strength = self.forces.values().map(|f| f.strength).sum();
    }

    /// Return the list of regions where this faction has forces.
    pub fn force_regions(&self) -> BTreeSet<RegionId> {
        self.forces.values().map(|f| f.region.clone()).collect()
    }

    /// Check if any force is a guerrilla-style unit.
    pub fn has_guerrilla_units(&self) -> bool {
        self.forces
            .values()
            .any(|f| matches!(f.unit_type, UnitType::Militia | UnitType::SpecialOperations))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::faction::ForceUnit;

    fn make_test_faction_state() -> RuntimeFactionState {
        RuntimeFactionState {
            faction_id: FactionId::from("test"),
            total_strength: 0.0,
            morale: 0.8,
            resources: 100.0,
            resource_rate: 10.0,
            logistics_capacity: 50.0,
            controlled_regions: vec![],
            forces: BTreeMap::new(),
            tech_deployed: vec![],
            region_hold_ticks: BTreeMap::new(),
            eliminated: false,
        }
    }

    fn make_force(
        id: &str,
        region: &str,
        strength: f64,
        unit_type: UnitType,
    ) -> (ForceId, ForceUnit) {
        let fid = ForceId::from(id);
        let unit = ForceUnit {
            id: fid.clone(),
            name: id.into(),
            unit_type,
            region: RegionId::from(region),
            strength,
            mobility: 1.0,
            force_projection: None,
            upkeep: 1.0,
            morale_modifier: 0.0,
            capabilities: vec![],
        };
        (fid, unit)
    }

    #[test]
    fn recompute_strength_sums_forces() {
        let mut fs = make_test_faction_state();

        let (id1, u1) = make_force("f1", "r1", 50.0, UnitType::Infantry);
        let (id2, u2) = make_force("f2", "r2", 30.0, UnitType::Infantry);
        let (id3, u3) = make_force("f3", "r3", 20.0, UnitType::Armor);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);
        fs.forces.insert(id3, u3);

        fs.recompute_strength();

        assert!(
            (fs.total_strength - 100.0).abs() < f64::EPSILON,
            "total_strength should be 100.0, got {}",
            fs.total_strength,
        );
    }

    #[test]
    fn force_regions_lists_all() {
        let mut fs = make_test_faction_state();

        let (id1, u1) = make_force("f1", "r1", 50.0, UnitType::Infantry);
        let (id2, u2) = make_force("f2", "r2", 30.0, UnitType::Infantry);
        let (id3, u3) = make_force("f3", "r3", 20.0, UnitType::Infantry);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);
        fs.forces.insert(id3, u3);

        let regions = fs.force_regions();
        assert_eq!(regions.len(), 3, "should have 3 regions");
        assert!(regions.contains(&RegionId::from("r1")));
        assert!(regions.contains(&RegionId::from("r2")));
        assert!(regions.contains(&RegionId::from("r3")));
    }

    #[test]
    fn has_guerrilla_units_true() {
        let mut fs = make_test_faction_state();
        let (id, unit) = make_force("militia1", "r1", 50.0, UnitType::Militia);
        fs.forces.insert(id, unit);

        assert!(
            fs.has_guerrilla_units(),
            "should detect militia as guerrilla"
        );
    }

    #[test]
    fn has_guerrilla_units_false() {
        let mut fs = make_test_faction_state();
        let (id1, u1) = make_force("armor1", "r1", 50.0, UnitType::Armor);
        let (id2, u2) = make_force("inf1", "r2", 30.0, UnitType::Infantry);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);

        assert!(
            !fs.has_guerrilla_units(),
            "armor and infantry should not be guerrilla"
        );
    }
}
