use std::collections::HashSet;

use crate::events::{EventCondition, EventEffect};
use crate::faction::FactionType;
use crate::ids::{FactionId, RegionId, VictoryId};
use crate::map::MapSource;
use crate::scenario::Scenario;
use crate::simulation::AttritionModel;
use crate::victory::VictoryType;

/// The tutorial_symmetric.toml scenario embedded as a constant for testing.
const TUTORIAL_TOML: &str = r##"# Tutorial: Symmetric Conflict
#
# Two equal forces on a simple 4-region map. No technology cards,
# no events, no population dynamics. Pure Lanchester attrition
# with basic utility-based AI.
#
# This is the simplest possible scenario for testing the engine.

[meta]
name = "Tutorial — Symmetric Conflict"
description = """
Two equal factions, four regions in a square. Each faction starts
with one infantry unit in their home region. First to control 3 of 4
regions wins. Simple Lanchester square attrition.
"""
author = "AndrewAltimit"
version = "0.1.0"
tags = ["tutorial", "symmetric", "simple"]

[map]
[map.source]
type = "Grid"
width = 2
height = 2

[map.regions.north_west]
id = "north_west"
name = "North-West"
population = 500000
urbanization = 0.5
initial_control = "alpha"
strategic_value = 0.5
borders = ["north_east", "south_west"]

[map.regions.north_east]
id = "north_east"
name = "North-East"
population = 500000
urbanization = 0.5
strategic_value = 0.5
borders = ["north_west", "south_east"]

[map.regions.south_west]
id = "south_west"
name = "South-West"
population = 500000
urbanization = 0.5
strategic_value = 0.5
borders = ["north_west", "south_east"]

[map.regions.south_east]
id = "south_east"
name = "South-East"
population = 500000
urbanization = 0.5
initial_control = "bravo"
strategic_value = 0.5
borders = ["north_east", "south_west"]

[map.infrastructure]

[[map.terrain]]
region = "north_west"
terrain_type = "Rural"
movement_modifier = 1.0
defense_modifier = 1.0
visibility = 1.0

[[map.terrain]]
region = "north_east"
terrain_type = "Rural"
movement_modifier = 1.0
defense_modifier = 1.0
visibility = 1.0

[[map.terrain]]
region = "south_west"
terrain_type = "Rural"
movement_modifier = 1.0
defense_modifier = 1.0
visibility = 1.0

[[map.terrain]]
region = "south_east"
terrain_type = "Rural"
movement_modifier = 1.0
defense_modifier = 1.0
visibility = 1.0

# -- Factions ---------------------------------------------------------------

[factions.alpha]
id = "alpha"
name = "Faction Alpha"
description = "Northern force"
color = "#3366CC"
tech_access = []
initial_morale = 0.8
logistics_capacity = 10.0
initial_resources = 100.0
resource_rate = 5.0
command_resilience = 0.5
intelligence = 0.5
diplomacy = []

[factions.alpha.faction_type]
kind = "Military"
branch = "Army"

[factions.alpha.forces.alpha_infantry]
id = "alpha_infantry"
name = "Alpha 1st Infantry"
unit_type = "Infantry"
region = "north_west"
strength = 100.0
mobility = 1.0
upkeep = 2.0
morale_modifier = 0.0
capabilities = []

[factions.bravo]
id = "bravo"
name = "Faction Bravo"
description = "Southern force"
color = "#CC3333"
tech_access = []
initial_morale = 0.8
logistics_capacity = 10.0
initial_resources = 100.0
resource_rate = 5.0
command_resilience = 0.5
intelligence = 0.5
diplomacy = []

[factions.bravo.faction_type]
kind = "Military"
branch = "Army"

[factions.bravo.forces.bravo_infantry]
id = "bravo_infantry"
name = "Bravo 1st Infantry"
unit_type = "Infantry"
region = "south_east"
strength = 100.0
mobility = 1.0
upkeep = 2.0
morale_modifier = 0.0
capabilities = []

# -- Technology (none for this tutorial) ------------------------------------

[technology]

# -- Political Climate (minimal) -------------------------------------------

[political_climate]
tension = 0.3
institutional_trust = 0.7
population_segments = []
global_modifiers = []

[political_climate.media_landscape]
fragmentation = 0.3
disinformation_susceptibility = 0.2
state_control = 0.1
social_media_penetration = 0.5
internet_availability = 0.9

# -- Events (none for this tutorial) ----------------------------------------

[events]

# -- Simulation Config ------------------------------------------------------

[simulation]
max_ticks = 100
monte_carlo_runs = 100
fog_of_war = false
snapshot_interval = 10
seed = 42

[simulation.tick_duration]
Days = 1

[simulation.attrition_model]
Stochastic = { noise = 0.1 }

# -- Victory Conditions -----------------------------------------------------

[victory_conditions.alpha_control]
id = "alpha_control"
name = "Alpha Strategic Control"
faction = "alpha"

[victory_conditions.alpha_control.condition]
type = "StrategicControl"
threshold = 0.75

[victory_conditions.bravo_control]
id = "bravo_control"
name = "Bravo Strategic Control"
faction = "bravo"

[victory_conditions.bravo_control.condition]
type = "StrategicControl"
threshold = 0.75
"##;

// ============================================================================
// 1. TOML roundtrip test
// ============================================================================

#[test]
fn toml_roundtrip_tutorial_scenario() {
    let scenario: Scenario =
        toml::from_str(TUTORIAL_TOML).expect("failed to deserialize tutorial TOML");

    // Verify key fields from first parse
    assert_eq!(scenario.meta.name, "Tutorial \u{2014} Symmetric Conflict");
    assert_eq!(scenario.meta.author, "AndrewAltimit");
    assert_eq!(scenario.meta.version, "0.1.0");
    assert_eq!(scenario.meta.tags.len(), 3);
    assert_eq!(scenario.factions.len(), 2);
    assert_eq!(scenario.map.regions.len(), 4);
    assert_eq!(scenario.map.terrain.len(), 4);
    assert_eq!(scenario.victory_conditions.len(), 2);
    assert_eq!(scenario.simulation.max_ticks, 100);
    assert_eq!(scenario.simulation.monte_carlo_runs, 100);
    assert_eq!(scenario.simulation.seed, Some(42));
    assert!(!scenario.simulation.fog_of_war);

    // Serialize back to TOML
    let toml_str = toml::to_string(&scenario).expect("failed to serialize scenario to TOML");

    // Deserialize again
    let scenario2: Scenario =
        toml::from_str(&toml_str).expect("failed to deserialize roundtripped TOML");

    // Verify key fields match between first and second parse
    assert_eq!(scenario2.meta.name, scenario.meta.name);
    assert_eq!(scenario2.meta.author, scenario.meta.author);
    assert_eq!(scenario2.meta.version, scenario.meta.version);
    assert_eq!(scenario2.meta.tags, scenario.meta.tags);
    assert_eq!(scenario2.factions.len(), scenario.factions.len());
    assert_eq!(scenario2.map.regions.len(), scenario.map.regions.len());
    assert_eq!(scenario2.map.terrain.len(), scenario.map.terrain.len());
    assert_eq!(
        scenario2.victory_conditions.len(),
        scenario.victory_conditions.len()
    );
    assert_eq!(
        scenario2.simulation.max_ticks,
        scenario.simulation.max_ticks
    );
    assert_eq!(scenario2.simulation.seed, scenario.simulation.seed);

    // Verify faction details survived roundtrip
    let alpha = scenario2
        .factions
        .get(&FactionId::from("alpha"))
        .expect("alpha faction missing after roundtrip");
    assert_eq!(alpha.name, "Faction Alpha");
    assert_eq!(alpha.color, "#3366CC");
    assert!((alpha.initial_morale - 0.8).abs() < f64::EPSILON);
    assert_eq!(alpha.forces.len(), 1);

    let bravo = scenario2
        .factions
        .get(&FactionId::from("bravo"))
        .expect("bravo faction missing after roundtrip");
    assert_eq!(bravo.name, "Faction Bravo");
    assert_eq!(bravo.forces.len(), 1);

    // Verify region details survived roundtrip
    let nw = scenario2
        .map
        .regions
        .get(&RegionId::from("north_west"))
        .expect("north_west region missing after roundtrip");
    assert_eq!(nw.name, "North-West");
    assert_eq!(nw.population, 500_000);
    assert_eq!(nw.initial_control, Some(FactionId::from("alpha")));
}

// ============================================================================
// 2. JSON roundtrip test
// ============================================================================

#[test]
fn json_roundtrip_tutorial_scenario() {
    let scenario: Scenario =
        toml::from_str(TUTORIAL_TOML).expect("failed to deserialize tutorial TOML");

    let json_str = serde_json::to_string(&scenario).expect("failed to serialize scenario to JSON");

    let scenario2: Scenario =
        serde_json::from_str(&json_str).expect("failed to deserialize scenario from JSON");

    // Verify key fields
    assert_eq!(scenario2.meta.name, scenario.meta.name);
    assert_eq!(scenario2.meta.author, scenario.meta.author);
    assert_eq!(scenario2.factions.len(), scenario.factions.len());
    assert_eq!(scenario2.map.regions.len(), scenario.map.regions.len());
    assert_eq!(
        scenario2.simulation.max_ticks,
        scenario.simulation.max_ticks
    );
    assert_eq!(scenario2.simulation.seed, scenario.simulation.seed);

    // Verify nested data survived JSON roundtrip
    let alpha = scenario2
        .factions
        .get(&FactionId::from("alpha"))
        .expect("alpha faction missing after JSON roundtrip");
    assert_eq!(alpha.name, "Faction Alpha");
    assert!((alpha.initial_resources - 100.0).abs() < f64::EPSILON);
}

// ============================================================================
// 3. ID newtype tests
// ============================================================================

#[test]
fn faction_id_display() {
    let id = FactionId::from("rebels");
    assert_eq!(format!("{id}"), "rebels");
}

#[test]
fn faction_id_from_str() {
    let id = FactionId::from("alpha");
    assert_eq!(id.0, "alpha");
}

#[test]
fn faction_id_from_string() {
    let id = FactionId::from(String::from("bravo"));
    assert_eq!(id.0, "bravo");
}

#[test]
fn faction_id_eq_and_hash() {
    let a = FactionId::from("same");
    let b = FactionId::from("same");
    let c = FactionId::from("different");

    assert_eq!(a, b);
    assert_ne!(a, c);

    // Verify Hash works (same values produce same bucket)
    let mut set = HashSet::new();
    set.insert(a.clone());
    assert!(set.contains(&b));
    assert!(!set.contains(&c));
}

#[test]
fn faction_id_ord() {
    let a = FactionId::from("aaa");
    let b = FactionId::from("bbb");
    let c = FactionId::from("ccc");
    assert!(a < b);
    assert!(b < c);
    assert!(a < c);
}

#[test]
fn different_id_types_display_independently() {
    let faction = FactionId::from("alpha");
    let region = RegionId::from("alpha");
    let victory = VictoryId::from("alpha");

    // They can hold the same string but are distinct types
    assert_eq!(format!("{faction}"), "alpha");
    assert_eq!(format!("{region}"), "alpha");
    assert_eq!(format!("{victory}"), "alpha");
}

// ============================================================================
// 4. MapSource serde
// ============================================================================

#[test]
fn map_source_grid_toml_roundtrip() {
    let toml_str = r#"type = "Grid"
width = 10
height = 5
"#;
    let source: MapSource = toml::from_str(toml_str).expect("failed to deserialize Grid MapSource");
    match &source {
        MapSource::Grid { width, height } => {
            assert_eq!(*width, 10);
            assert_eq!(*height, 5);
        },
        other => panic!("expected Grid, got {other:?}"),
    }

    let serialized = toml::to_string(&source).expect("failed to serialize Grid");
    let roundtripped: MapSource =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Grid");
    match roundtripped {
        MapSource::Grid { width, height } => {
            assert_eq!(width, 10);
            assert_eq!(height, 5);
        },
        other => panic!("expected Grid, got {other:?}"),
    }
}

#[test]
fn map_source_builtin_toml_roundtrip() {
    let toml_str = r#"type = "BuiltIn"
name = "usa_states"
"#;
    let source: MapSource =
        toml::from_str(toml_str).expect("failed to deserialize BuiltIn MapSource");
    match &source {
        MapSource::BuiltIn { name } => {
            assert_eq!(name, "usa_states");
        },
        other => panic!("expected BuiltIn, got {other:?}"),
    }

    let serialized = toml::to_string(&source).expect("failed to serialize BuiltIn");
    let roundtripped: MapSource =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped BuiltIn");
    match roundtripped {
        MapSource::BuiltIn { name } => assert_eq!(name, "usa_states"),
        other => panic!("expected BuiltIn, got {other:?}"),
    }
}

#[test]
fn map_source_geojson_toml_roundtrip() {
    let toml_str = r#"type = "GeoJson"
path = "/data/regions.geojson"
"#;
    let source: MapSource =
        toml::from_str(toml_str).expect("failed to deserialize GeoJson MapSource");
    match &source {
        MapSource::GeoJson { path } => {
            assert_eq!(path, "/data/regions.geojson");
        },
        other => panic!("expected GeoJson, got {other:?}"),
    }

    let serialized = toml::to_string(&source).expect("failed to serialize GeoJson");
    let roundtripped: MapSource =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped GeoJson");
    match roundtripped {
        MapSource::GeoJson { path } => {
            assert_eq!(path, "/data/regions.geojson");
        },
        other => panic!("expected GeoJson, got {other:?}"),
    }
}

// ============================================================================
// 5. FactionType serde
// ============================================================================

#[test]
fn faction_type_military_toml_roundtrip() {
    let toml_str = r#"kind = "Military"
branch = "Army"
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize Military FactionType");
    match &ft {
        FactionType::Military { branch } => {
            assert_eq!(*branch, crate::faction::MilitaryBranch::Army);
        },
        other => panic!("expected Military, got {other:?}"),
    }

    let serialized = toml::to_string(&ft).expect("failed to serialize Military");
    let roundtripped: FactionType =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Military");
    match roundtripped {
        FactionType::Military { branch } => {
            assert_eq!(branch, crate::faction::MilitaryBranch::Army);
        },
        other => panic!("expected Military, got {other:?}"),
    }
}

#[test]
fn faction_type_insurgent_toml_roundtrip() {
    let toml_str = r#"kind = "Insurgent"
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize Insurgent FactionType");
    assert!(matches!(ft, FactionType::Insurgent));

    let serialized = toml::to_string(&ft).expect("failed to serialize Insurgent");
    let roundtripped: FactionType =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Insurgent");
    assert!(matches!(roundtripped, FactionType::Insurgent));
}

#[test]
fn faction_type_civilian_toml_roundtrip() {
    let toml_str = r#"kind = "Civilian"
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize Civilian FactionType");
    assert!(matches!(ft, FactionType::Civilian));

    let serialized = toml::to_string(&ft).expect("failed to serialize Civilian");
    let roundtripped: FactionType =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Civilian");
    assert!(matches!(roundtripped, FactionType::Civilian));
}

#[test]
fn faction_type_private_military_toml_roundtrip() {
    let toml_str = r#"kind = "PrivateMilitary"
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize PrivateMilitary FactionType");
    assert!(matches!(ft, FactionType::PrivateMilitary));
}

#[test]
fn faction_type_foreign_toml_roundtrip() {
    let toml_str = r#"kind = "Foreign"
is_proxy = true
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize Foreign FactionType");
    match &ft {
        FactionType::Foreign { is_proxy } => assert!(*is_proxy),
        other => panic!("expected Foreign, got {other:?}"),
    }

    let serialized = toml::to_string(&ft).expect("failed to serialize Foreign");
    let roundtripped: FactionType =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Foreign");
    match roundtripped {
        FactionType::Foreign { is_proxy } => assert!(is_proxy),
        other => panic!("expected Foreign, got {other:?}"),
    }
}

#[test]
fn faction_type_government_toml_roundtrip() {
    let toml_str = r#"kind = "Government"

[institutions]
"#;
    let ft: FactionType =
        toml::from_str(toml_str).expect("failed to deserialize Government FactionType");
    match &ft {
        FactionType::Government { institutions } => {
            assert!(institutions.is_empty());
        },
        other => panic!("expected Government, got {other:?}"),
    }

    let serialized = toml::to_string(&ft).expect("failed to serialize Government");
    let roundtripped: FactionType =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Government");
    assert!(matches!(roundtripped, FactionType::Government { .. }));
}

// ============================================================================
// 6. AttritionModel serde
// ============================================================================

/// Wrapper to test `AttritionModel` in a TOML table context,
/// since TOML cannot represent a bare enum variant at the top level.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct AttritionWrap {
    model: AttritionModel,
}

#[test]
fn attrition_model_lanchester_linear_toml() {
    let toml_str = "model = \"LanchesterLinear\"\n";
    let wrap: AttritionWrap =
        toml::from_str(toml_str).expect("failed to deserialize LanchesterLinear");
    assert!(matches!(wrap.model, AttritionModel::LanchesterLinear));

    let serialized = toml::to_string(&wrap).expect("failed to serialize");
    let roundtripped: AttritionWrap =
        toml::from_str(&serialized).expect("failed to roundtrip LanchesterLinear");
    assert!(matches!(
        roundtripped.model,
        AttritionModel::LanchesterLinear
    ));
}

#[test]
fn attrition_model_lanchester_square_toml() {
    let toml_str = "model = \"LanchesterSquare\"\n";
    let wrap: AttritionWrap =
        toml::from_str(toml_str).expect("failed to deserialize LanchesterSquare");
    assert!(matches!(wrap.model, AttritionModel::LanchesterSquare));
}

#[test]
fn attrition_model_hybrid_toml() {
    let toml_str = "model = \"Hybrid\"\n";
    let wrap: AttritionWrap = toml::from_str(toml_str).expect("failed to deserialize Hybrid");
    assert!(matches!(wrap.model, AttritionModel::Hybrid));
}

#[test]
fn attrition_model_stochastic_toml() {
    // Stochastic variant with noise field, serialized as a table
    let toml_str = r#"Stochastic = { noise = 0.25 }
"#;
    let model: AttritionModel = toml::from_str(toml_str).expect("failed to deserialize Stochastic");
    match &model {
        AttritionModel::Stochastic { noise } => {
            assert!((*noise - 0.25).abs() < f64::EPSILON);
        },
        other => panic!("expected Stochastic, got {other:?}"),
    }

    let serialized = toml::to_string(&model).expect("failed to serialize Stochastic");
    let roundtripped: AttritionModel =
        toml::from_str(&serialized).expect("failed to deserialize roundtripped Stochastic");
    match roundtripped {
        AttritionModel::Stochastic { noise } => {
            assert!((noise - 0.25).abs() < f64::EPSILON);
        },
        other => panic!("expected Stochastic, got {other:?}"),
    }
}

// ============================================================================
// 7. VictoryType serde
// ============================================================================

#[test]
fn victory_type_strategic_control_toml() {
    let toml_str = r#"type = "StrategicControl"
threshold = 0.75
"#;
    let vt: VictoryType = toml::from_str(toml_str).expect("failed to deserialize StrategicControl");
    match &vt {
        VictoryType::StrategicControl { threshold } => {
            assert!((*threshold - 0.75).abs() < f64::EPSILON);
        },
        other => panic!("expected StrategicControl, got {other:?}"),
    }

    let serialized = toml::to_string(&vt).expect("failed to serialize");
    let roundtripped: VictoryType =
        toml::from_str(&serialized).expect("failed to roundtrip StrategicControl");
    match roundtripped {
        VictoryType::StrategicControl { threshold } => {
            assert!((threshold - 0.75).abs() < f64::EPSILON);
        },
        other => panic!("expected StrategicControl, got {other:?}"),
    }
}

#[test]
fn victory_type_military_dominance_toml() {
    let toml_str = r#"type = "MilitaryDominance"
enemy_strength_below = 10.0
"#;
    let vt: VictoryType =
        toml::from_str(toml_str).expect("failed to deserialize MilitaryDominance");
    match &vt {
        VictoryType::MilitaryDominance {
            enemy_strength_below,
        } => {
            assert!((*enemy_strength_below - 10.0).abs() < f64::EPSILON);
        },
        other => {
            panic!("expected MilitaryDominance, got {other:?}")
        },
    }

    let serialized = toml::to_string(&vt).expect("failed to serialize");
    let roundtripped: VictoryType =
        toml::from_str(&serialized).expect("failed to roundtrip MilitaryDominance");
    match roundtripped {
        VictoryType::MilitaryDominance {
            enemy_strength_below,
        } => {
            assert!((enemy_strength_below - 10.0).abs() < f64::EPSILON);
        },
        other => {
            panic!("expected MilitaryDominance, got {other:?}")
        },
    }
}

#[test]
fn victory_type_peace_settlement_toml() {
    let toml_str = r#"type = "PeaceSettlement"
"#;
    let vt: VictoryType = toml::from_str(toml_str).expect("failed to deserialize PeaceSettlement");
    assert!(matches!(vt, VictoryType::PeaceSettlement));

    let serialized = toml::to_string(&vt).expect("failed to serialize");
    let roundtripped: VictoryType =
        toml::from_str(&serialized).expect("failed to roundtrip PeaceSettlement");
    assert!(matches!(roundtripped, VictoryType::PeaceSettlement));
}

#[test]
fn victory_type_hold_regions_toml() {
    let toml_str = r#"type = "HoldRegions"
regions = ["east", "west"]
duration = 10
"#;
    let vt: VictoryType = toml::from_str(toml_str).expect("failed to deserialize HoldRegions");
    match &vt {
        VictoryType::HoldRegions { regions, duration } => {
            assert_eq!(regions.len(), 2);
            assert_eq!(*duration, 10);
        },
        other => panic!("expected HoldRegions, got {other:?}"),
    }
}

#[test]
fn victory_type_custom_toml() {
    let toml_str = r#"type = "Custom"
variable = "tension"
threshold = 0.9
above = true
"#;
    let vt: VictoryType =
        toml::from_str(toml_str).expect("failed to deserialize Custom VictoryType");
    match &vt {
        VictoryType::Custom {
            variable,
            threshold,
            above,
        } => {
            assert_eq!(variable, "tension");
            assert!((*threshold - 0.9).abs() < f64::EPSILON);
            assert!(*above);
        },
        other => panic!("expected Custom, got {other:?}"),
    }
}

// ============================================================================
// 8. EventCondition serde
// ============================================================================

#[test]
fn event_condition_region_control_toml() {
    let toml_str = r#"condition = "RegionControl"
region = "east"
faction = "alpha"
controlled = true
"#;
    let cond: EventCondition =
        toml::from_str(toml_str).expect("failed to deserialize RegionControl condition");
    match &cond {
        EventCondition::RegionControl {
            region,
            faction,
            controlled,
        } => {
            assert_eq!(region.0, "east");
            assert_eq!(faction.0, "alpha");
            assert!(*controlled);
        },
        other => panic!("expected RegionControl, got {other:?}"),
    }

    let serialized = toml::to_string(&cond).expect("failed to serialize RegionControl");
    let roundtripped: EventCondition =
        toml::from_str(&serialized).expect("failed to roundtrip RegionControl");
    assert!(matches!(roundtripped, EventCondition::RegionControl { .. }));
}

#[test]
fn event_condition_tension_above_toml() {
    let toml_str = r#"condition = "TensionAbove"
threshold = 0.8
"#;
    let cond: EventCondition =
        toml::from_str(toml_str).expect("failed to deserialize TensionAbove");
    match &cond {
        EventCondition::TensionAbove { threshold } => {
            assert!((*threshold - 0.8).abs() < f64::EPSILON);
        },
        other => panic!("expected TensionAbove, got {other:?}"),
    }
}

#[test]
fn event_condition_tick_at_least_toml() {
    let toml_str = r#"condition = "TickAtLeast"
tick = 50
"#;
    let cond: EventCondition = toml::from_str(toml_str).expect("failed to deserialize TickAtLeast");
    match &cond {
        EventCondition::TickAtLeast { tick } => {
            assert_eq!(*tick, 50);
        },
        other => panic!("expected TickAtLeast, got {other:?}"),
    }
}

#[test]
fn event_condition_morale_below_toml() {
    let toml_str = r#"condition = "MoraleBelow"
faction = "bravo"
threshold = 0.2
"#;
    let cond: EventCondition = toml::from_str(toml_str).expect("failed to deserialize MoraleBelow");
    match &cond {
        EventCondition::MoraleBelow { faction, threshold } => {
            assert_eq!(faction.0, "bravo");
            assert!((*threshold - 0.2).abs() < f64::EPSILON);
        },
        other => panic!("expected MoraleBelow, got {other:?}"),
    }
}

#[test]
fn event_condition_expression_toml() {
    let toml_str = r#"condition = "Expression"
expr = "tension > 0.5 && tick > 10"
"#;
    let cond: EventCondition =
        toml::from_str(toml_str).expect("failed to deserialize Expression condition");
    match &cond {
        EventCondition::Expression { expr } => {
            assert_eq!(expr, "tension > 0.5 && tick > 10");
        },
        other => panic!("expected Expression, got {other:?}"),
    }
}

// ============================================================================
// 9. EventEffect serde
// ============================================================================

#[test]
fn event_effect_morale_shift_toml() {
    let toml_str = r#"effect = "MoraleShift"
faction = "alpha"
delta = -0.15
"#;
    let eff: EventEffect =
        toml::from_str(toml_str).expect("failed to deserialize MoraleShift effect");
    match &eff {
        EventEffect::MoraleShift { faction, delta } => {
            assert_eq!(faction.0, "alpha");
            assert!((*delta - (-0.15)).abs() < f64::EPSILON);
        },
        other => panic!("expected MoraleShift, got {other:?}"),
    }

    let serialized = toml::to_string(&eff).expect("failed to serialize MoraleShift");
    let roundtripped: EventEffect =
        toml::from_str(&serialized).expect("failed to roundtrip MoraleShift");
    assert!(matches!(roundtripped, EventEffect::MoraleShift { .. }));
}

#[test]
fn event_effect_tension_shift_toml() {
    let toml_str = r#"effect = "TensionShift"
delta = 0.3
"#;
    let eff: EventEffect = toml::from_str(toml_str).expect("failed to deserialize TensionShift");
    match &eff {
        EventEffect::TensionShift { delta } => {
            assert!((*delta - 0.3).abs() < f64::EPSILON);
        },
        other => panic!("expected TensionShift, got {other:?}"),
    }
}

#[test]
fn event_effect_damage_infra_toml() {
    let toml_str = r#"effect = "DamageInfra"
infra = "power_grid_1"
damage = 0.5
"#;
    let eff: EventEffect = toml::from_str(toml_str).expect("failed to deserialize DamageInfra");
    match &eff {
        EventEffect::DamageInfra { infra, damage } => {
            assert_eq!(infra.0, "power_grid_1");
            assert!((*damage - 0.5).abs() < f64::EPSILON);
        },
        other => panic!("expected DamageInfra, got {other:?}"),
    }
}

#[test]
fn event_effect_narrative_toml() {
    let toml_str = r#"effect = "Narrative"
text = "The ceasefire collapses."
"#;
    let eff: EventEffect =
        toml::from_str(toml_str).expect("failed to deserialize Narrative effect");
    match &eff {
        EventEffect::Narrative { text } => {
            assert_eq!(text, "The ceasefire collapses.");
        },
        other => panic!("expected Narrative, got {other:?}"),
    }
}

#[test]
fn event_effect_resource_change_toml() {
    let toml_str = r#"effect = "ResourceChange"
faction = "bravo"
delta = -25.0
"#;
    let eff: EventEffect = toml::from_str(toml_str).expect("failed to deserialize ResourceChange");
    match &eff {
        EventEffect::ResourceChange { faction, delta } => {
            assert_eq!(faction.0, "bravo");
            assert!((*delta - (-25.0)).abs() < f64::EPSILON);
        },
        other => panic!("expected ResourceChange, got {other:?}"),
    }
}

// ============================================================================
// 10. Partial scenario parsing — missing required fields
// ============================================================================

/// Helper: a minimal valid scenario TOML (all required fields present).
fn minimal_scenario_toml() -> String {
    String::from(
        r#"[meta]
name = "minimal"
description = "minimal test"
author = "test"
version = "0.1.0"
tags = []

[map]
terrain = []
[map.source]
type = "Grid"
width = 1
height = 1
[map.regions]
[map.infrastructure]

[factions]
[technology]
[events]

[political_climate]
tension = 0.5
institutional_trust = 0.5
population_segments = []
global_modifiers = []
[political_climate.media_landscape]
fragmentation = 0.0
disinformation_susceptibility = 0.0
state_control = 0.0
social_media_penetration = 0.0
internet_availability = 0.0

[simulation]
max_ticks = 10
monte_carlo_runs = 1
fog_of_war = false
snapshot_interval = 1
attrition_model = "LanchesterLinear"
[simulation.tick_duration]
Days = 1

[victory_conditions]
"#,
    )
}

#[test]
fn minimal_scenario_parses_successfully() {
    let scenario: Scenario =
        toml::from_str(&minimal_scenario_toml()).expect("minimal scenario should parse");
    assert_eq!(scenario.meta.name, "minimal");
    assert!(scenario.factions.is_empty());
    assert!(scenario.victory_conditions.is_empty());
}

#[test]
fn missing_meta_produces_error() {
    // Remove the [meta] section entirely
    let toml_str = minimal_scenario_toml().replace(
        "[meta]\nname = \"minimal\"\n\
             description = \"minimal test\"\n\
             author = \"test\"\nversion = \"0.1.0\"\ntags = []\n",
        "",
    );
    let result = toml::from_str::<Scenario>(&toml_str);
    assert!(result.is_err(), "expected error for missing [meta]");
    let err_msg = format!("{}", result.expect_err("should be Err"));
    assert!(
        err_msg.contains("meta"),
        "error should mention 'meta', got: {err_msg}"
    );
}

#[test]
fn missing_simulation_produces_error() {
    // Remove the entire [simulation] block
    let base = minimal_scenario_toml();
    let sim_start = base.find("[simulation]").expect("should find [simulation]");
    let vc_start = base
        .find("[victory_conditions]")
        .expect("should find [victory_conditions]");
    let toml_str = format!("{}{}", &base[..sim_start], &base[vc_start..]);
    let result = toml::from_str::<Scenario>(&toml_str);
    assert!(result.is_err(), "expected error for missing [simulation]");
    let err_msg = format!("{}", result.expect_err("should be Err"));
    assert!(
        err_msg.contains("simulation"),
        "error should mention 'simulation', got: {err_msg}"
    );
}

#[test]
fn missing_map_source_produces_error() {
    // Remove the [map.source] section
    let toml_str = minimal_scenario_toml()
        .replace("[map.source]\ntype = \"Grid\"\nwidth = 1\nheight = 1\n", "");
    let result = toml::from_str::<Scenario>(&toml_str);
    assert!(result.is_err(), "expected error for missing map.source");
    let err_msg = format!("{}", result.expect_err("should be Err"));
    assert!(
        err_msg.contains("source"),
        "error should mention 'source', got: {err_msg}"
    );
}

#[test]
fn completely_empty_toml_produces_error() {
    let result = toml::from_str::<Scenario>("");
    assert!(result.is_err(), "expected error for empty input");
}

#[test]
fn invalid_toml_syntax_produces_error() {
    let result = toml::from_str::<Scenario>("{{{{ not valid");
    assert!(result.is_err(), "expected error for invalid TOML");
}

#[test]
fn wrong_type_for_field_produces_error() {
    // Replace the name field value with an integer
    let toml_str = minimal_scenario_toml().replace("name = \"minimal\"", "name = 42");
    let result = toml::from_str::<Scenario>(&toml_str);
    assert!(
        result.is_err(),
        "expected error when name is integer instead of string"
    );
}

// ============================================================================
// 10. Confidence tag fields (PR 1 — uncertainty foundation)
// ============================================================================
//
// Exercises the optional author-confidence tags on `CampaignPhase` and
// `PhaseCost`. Three behaviours matter and are covered below:
//
//   1. TOML input WITHOUT confidence tags continues to parse — the
//      fields are `Option<ConfidenceLevel>` with `#[serde(default)]`.
//   2. TOML input WITH explicit tags rounds-trips through serde.
//   3. JSON output omits `None` values (the `skip_serializing_if`
//      attribute) so old consumers don't see spurious keys.

/// Suffix that appends a kill-chain with two phases — one carrying
/// explicit confidence tags, one without — to the minimal TOML.
const CONFIDENCE_CHAIN_SUFFIX: &str = r##"
[kill_chains.alpha]
id = "alpha"
name = "Alpha"
attacker = "red"
target = "blue"
entry_phase = "tagged"

[kill_chains.alpha.phases.tagged]
id = "tagged"
name = "Tagged Phase"
base_success_probability = 0.5
min_duration = 1
max_duration = 1
detection_probability_per_tick = 0.1
attribution_difficulty = 0.5
parameter_confidence = "Low"

[kill_chains.alpha.phases.tagged.cost]
attacker_dollars = 100.0
defender_dollars = 10000.0
attacker_resources = 0.0
confidence = "High"

[kill_chains.alpha.phases.untagged]
id = "untagged"
name = "Untagged Phase"
base_success_probability = 0.5
min_duration = 1
max_duration = 1
detection_probability_per_tick = 0.1
attribution_difficulty = 0.5

[kill_chains.alpha.phases.untagged.cost]
attacker_dollars = 1.0
defender_dollars = 1.0
attacker_resources = 0.0
"##;

fn confidence_chain_toml() -> String {
    let mut s = minimal_scenario_toml();
    s.push_str(CONFIDENCE_CHAIN_SUFFIX);
    s
}

#[test]
fn parameter_confidence_absent_parses_as_none() {
    // Tutorial scenarios have no confidence tags and must continue to
    // load as `None` rather than producing a parse error.
    let scenario: Scenario =
        toml::from_str(TUTORIAL_TOML).expect("tutorial should still parse after schema additions");
    // No kill chains in the tutorial — just ensure deserialization
    // succeeded and the default `None` variant is in effect where the
    // schema touches it.
    assert!(scenario.kill_chains.is_empty());
}

#[test]
fn parameter_confidence_tags_roundtrip_through_toml() {
    use crate::stats::ConfidenceLevel;

    let scenario: Scenario =
        toml::from_str(&confidence_chain_toml()).expect("tagged scenario should parse");
    let chain = scenario
        .kill_chains
        .get(&crate::ids::KillChainId::from("alpha"))
        .expect("alpha chain should exist");

    let tagged = chain
        .phases
        .get(&crate::ids::PhaseId::from("tagged"))
        .expect("tagged phase");
    assert_eq!(
        tagged.parameter_confidence,
        Some(ConfidenceLevel::Low),
        "explicit 'Low' tag should deserialize"
    );
    assert_eq!(
        tagged.cost.confidence,
        Some(ConfidenceLevel::High),
        "explicit cost 'High' tag should deserialize"
    );

    let untagged = chain
        .phases
        .get(&crate::ids::PhaseId::from("untagged"))
        .expect("untagged phase");
    assert!(
        untagged.parameter_confidence.is_none(),
        "absent tag must deserialize as None"
    );
    assert!(
        untagged.cost.confidence.is_none(),
        "absent cost tag must deserialize as None"
    );

    // Roundtrip once through TOML and re-verify — catches asymmetric
    // serialize/deserialize bugs.
    let serialized = toml::to_string(&scenario).expect("serialize");
    let reparsed: Scenario = toml::from_str(&serialized).expect("reparse");
    let chain2 = reparsed
        .kill_chains
        .get(&crate::ids::KillChainId::from("alpha"))
        .expect("chain survives roundtrip");
    let tagged2 = chain2
        .phases
        .get(&crate::ids::PhaseId::from("tagged"))
        .expect("phase survives roundtrip");
    assert_eq!(tagged2.parameter_confidence, Some(ConfidenceLevel::Low));
    assert_eq!(tagged2.cost.confidence, Some(ConfidenceLevel::High));
}

#[test]
fn untagged_phase_json_omits_confidence_keys() {
    // `skip_serializing_if = "Option::is_none"` on the new fields means
    // downstream JSON consumers never see a `"parameter_confidence":
    // null` key when the author didn't tag the phase.
    let scenario: Scenario =
        toml::from_str(&confidence_chain_toml()).expect("tagged scenario should parse");
    let json = serde_json::to_string(&scenario).expect("serialize");
    // Extract the "untagged" phase substring to inspect — we only want
    // to assert on that phase, not the whole doc.
    let needle = "\"untagged\":";
    let idx = json.find(needle).expect("untagged phase should be present");
    // Take up to the next phase or end, whichever comes first.
    let slice = &json[idx..(idx + 400).min(json.len())];
    assert!(
        !slice.contains("parameter_confidence"),
        "absent parameter_confidence must not serialize; got:\n{slice}"
    );
    // The cost block for the untagged phase also should lack its tag.
    // This slice is small enough to check in one go.
    assert!(
        !slice.contains("\"confidence\":"),
        "absent cost confidence must not serialize; got:\n{slice}"
    );
}

// ============================================================================
// 11. MonteCarloSummary CI fields serde roundtrip
// ============================================================================

#[test]
fn monte_carlo_summary_ci_fields_json_roundtrip() {
    use crate::ids::{FactionId, KillChainId};
    use crate::stats::{
        ConfidenceInterval, ConfidenceLevel, FeasibilityCIs, FeasibilityConfidence, FeasibilityRow,
        MonteCarloSummary,
    };
    use std::collections::BTreeMap;

    let fid = FactionId::from("gov");
    let chain_id = KillChainId::from("alpha");
    let mut win_rates = BTreeMap::new();
    win_rates.insert(fid.clone(), 0.625);
    let mut win_rate_cis = BTreeMap::new();
    win_rate_cis.insert(
        fid.clone(),
        ConfidenceInterval::new(0.625, 0.525, 0.715, 100),
    );
    let row = FeasibilityRow {
        chain_id: chain_id.clone(),
        chain_name: "Alpha".into(),
        technology_readiness: 0.7,
        operational_complexity: 0.3,
        detection_probability: 0.4,
        success_probability: 0.8,
        consequence_severity: 0.5,
        attribution_difficulty: 0.2,
        cost_asymmetry_ratio: 1000.0,
        confidence: FeasibilityConfidence {
            technology_readiness: ConfidenceLevel::High,
            operational_complexity: ConfidenceLevel::Medium,
            detection_probability: ConfidenceLevel::High,
            success_probability: ConfidenceLevel::Medium,
            consequence_severity: ConfidenceLevel::Low,
        },
        ci_95: FeasibilityCIs {
            detection_probability: Some(ConfidenceInterval::new(0.4, 0.32, 0.48, 100)),
            success_probability: Some(ConfidenceInterval::new(0.8, 0.72, 0.86, 100)),
            consequence_severity: None,
        },
    };
    let summary = MonteCarloSummary {
        total_runs: 100,
        win_rates,
        win_rate_cis,
        average_duration: 42.0,
        metric_distributions: BTreeMap::new(),
        regional_control: BTreeMap::new(),
        event_probabilities: BTreeMap::new(),
        campaign_summaries: BTreeMap::new(),
        feasibility_matrix: vec![row],
        seam_scores: BTreeMap::new(),
    };

    let json = serde_json::to_string(&summary).expect("serialize");
    let reparsed: MonteCarloSummary = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(reparsed.total_runs, 100);
    let ci = reparsed
        .win_rate_cis
        .get(&fid)
        .expect("win rate CI survives JSON roundtrip");
    assert!((ci.lower - 0.525).abs() < 1e-12);
    assert!((ci.upper - 0.715).abs() < 1e-12);
    assert_eq!(ci.n, 100);

    let fr = reparsed
        .feasibility_matrix
        .first()
        .expect("feasibility row survives");
    let dc = fr
        .ci_95
        .detection_probability
        .as_ref()
        .expect("detection CI survives");
    assert!((dc.lower - 0.32).abs() < 1e-12);
    assert!(
        fr.ci_95.consequence_severity.is_none(),
        "None CI must survive roundtrip as None"
    );
}

#[test]
fn old_summary_without_ci_fields_deserializes() {
    // Simulate a JSON document produced by a pre-PR1 build: no
    // `win_rate_cis` key, no `ci_95` on feasibility rows. This must
    // still deserialize into the current `MonteCarloSummary` shape
    // with the new fields populated from their defaults.
    use crate::stats::MonteCarloSummary;

    let legacy_json = r#"{
        "total_runs": 4,
        "win_rates": {"gov": 0.75},
        "average_duration": 12.5,
        "metric_distributions": {},
        "regional_control": {},
        "event_probabilities": {},
        "feasibility_matrix": []
    }"#;
    let parsed: MonteCarloSummary =
        serde_json::from_str(legacy_json).expect("legacy summary should parse");
    assert_eq!(parsed.total_runs, 4);
    assert!(
        parsed.win_rate_cis.is_empty(),
        "absent field should default to empty map"
    );
    assert!(parsed.feasibility_matrix.is_empty());
}
