//! Integration tests for the force-projection (standoff-strike) phase.
//!
//! Pins the high-leverage observable behaviors:
//! - a `StandoffStrike` removes strength from a hostile force in an
//!   in-range region the firing unit never enters;
//! - reach is bounded by the hop budget — a 1-hop reach does not touch a
//!   2-hop region;
//! - mutually-allied factions are not struck (diplomacy coupling);
//! - a scenario with no `force_projection` produces no strike report
//!   (the gating / legacy bit-identity contract);
//! - same-seed runs produce identical strike reports (determinism);
//! - validation rejects malformed `force_projection` shapes.

use std::collections::BTreeMap;

use faultline_engine::{Engine, validate_scenario};
use faultline_types::faction::{
    Diplomacy, DiplomaticStance, Faction, FactionType, ForceProjection, ForceUnit, MilitaryBranch,
    UnitType,
};
use faultline_types::ids::{FactionId, ForceId, RegionId, VictoryId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Three-region linear corridor: r0 -- r1 -- r2.
fn make_region(id: &str, borders: Vec<&str>, controller: &str) -> Region {
    Region {
        id: RegionId::from(id),
        name: id.into(),
        population: 100_000,
        urbanization: 0.5,
        initial_control: Some(FactionId::from(controller)),
        strategic_value: 1.0,
        borders: borders.into_iter().map(RegionId::from).collect(),
        centroid: None,
    }
}

fn make_force(id: &str, region: &str, strength: f64) -> ForceUnit {
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: RegionId::from(region),
        strength,
        mobility: 0.0, // never moves on its own
        force_projection: None,
        upkeep: 0.0,
        morale_modifier: 0.0,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn make_faction(id: &str, forces: Vec<ForceUnit>, diplomacy: Vec<DiplomaticStance>) -> Faction {
    let mut fmap = BTreeMap::new();
    for f in forces {
        fmap.insert(f.id.clone(), f);
    }
    Faction {
        id: FactionId::from(id),
        name: id.into(),
        faction_type: FactionType::Military {
            branch: MilitaryBranch::Army,
        },
        description: String::new(),
        color: "#000".into(),
        forces: fmap,
        tech_access: vec![],
        initial_morale: 0.8,
        logistics_capacity: 0.0,
        initial_resources: 0.0,
        resource_rate: 0.0,
        recruitment: None,
        command_resilience: 0.0,
        intelligence: 0.5,
        diplomacy,
        doctrine: Doctrine::Conventional,
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership: None,
        alliance_fracture: None,
        utility: None,
    }
}

/// Build a scenario with `red` (a standoff battery in r0) versus `blue`
/// (forces in r1 / r2). `reach` is red's standoff range; `diplomacy`
/// lets a test wire mutual alliance. `red_projection = false` removes
/// red's `force_projection` entirely (the gating / legacy case).
fn build_scenario(
    seed: u64,
    max_ticks: u32,
    reach: f64,
    red_projection: bool,
    mutual_alliance: bool,
) -> Scenario {
    let mut battery = make_force("battery", "r0", 500.0);
    if red_projection {
        battery.force_projection = Some(ForceProjection::StandoffStrike {
            range: reach,
            damage: 100.0,
        });
    }

    let red_diplomacy = if mutual_alliance {
        vec![DiplomaticStance {
            target_faction: FactionId::from("blue"),
            stance: Diplomacy::Allied,
        }]
    } else {
        vec![]
    };
    let blue_diplomacy = if mutual_alliance {
        vec![DiplomaticStance {
            target_faction: FactionId::from("red"),
            stance: Diplomacy::Allied,
        }]
    } else {
        vec![]
    };

    let red = make_faction("red", vec![battery], red_diplomacy);
    let blue = make_faction(
        "blue",
        vec![
            make_force("ridge", "r1", 1000.0),
            make_force("basin", "r2", 1000.0),
        ],
        blue_diplomacy,
    );

    let mut factions = BTreeMap::new();
    factions.insert(red.id.clone(), red);
    factions.insert(blue.id.clone(), blue);

    let mut regions = BTreeMap::new();
    regions.insert(RegionId::from("r0"), make_region("r0", vec!["r1"], "red"));
    regions.insert(
        RegionId::from("r1"),
        make_region("r1", vec!["r0", "r2"], "blue"),
    );
    regions.insert(RegionId::from("r2"), make_region("r2", vec!["r1"], "blue"));

    let terrain = ["r0", "r1", "r2"]
        .iter()
        .map(|r| TerrainModifier {
            region: RegionId::from(*r),
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        })
        .collect();

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("hold"),
        VictoryCondition {
            id: VictoryId::from("hold"),
            name: "Hold".into(),
            faction: FactionId::from("blue"),
            // Duration > max_ticks so the run uses its full budget.
            condition: VictoryType::HoldRegions {
                regions: vec![RegionId::from("r1"), RegionId::from("r2")],
                duration: max_ticks + 1,
            },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "force projection test".into(),
            description: String::new(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            historical_analogue: None,
            analytical_purpose: None,
            scenario_type: None,
            osint_sources: vec![],
            red_team_profile: None,
            blue_team_posture: None,
            sensitivity_parameters: vec![],
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 3,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain,
        },
        factions,
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.0,
            institutional_trust: 0.5,
            media_landscape: MediaLandscape {
                fragmentation: 0.0,
                disinformation_susceptibility: 0.0,
                state_control: 0.0,
                social_media_penetration: 0.0,
                internet_availability: 0.0,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(seed),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
            belief_model: None,
        },
        victory_conditions,
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Behavior tests
// ---------------------------------------------------------------------------

#[test]
fn standoff_strike_hits_in_range_hostile() {
    // reach 300 km => 2 hops; r0's battery covers r1 (1 hop) and r2 (2
    // hops). Both blue forces should take attrition without red ever
    // moving into r1 / r2.
    let scenario = build_scenario(1, 3, 300.0, true, false);
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");

    let report = result
        .force_projection_reports
        .get(&FactionId::from("red"))
        .expect("red should have landed strikes");
    assert!(report.strikes > 0, "expected at least one strike");
    assert!(
        report.total_strength_removed > 0.0,
        "expected strength removed"
    );
    // Both forward regions struck.
    assert!(report.regions_struck.contains(&RegionId::from("r1")));
    assert!(report.regions_struck.contains(&RegionId::from("r2")));

    // Blue's terminal strength should be below its starting 2000.
    let blue_strength = result
        .final_state
        .faction_states
        .get(&FactionId::from("blue"))
        .map(|fs| fs.total_strength)
        .expect("blue state present");
    assert!(
        blue_strength < 2000.0,
        "blue should have lost strength to standoff fires; got {blue_strength}"
    );
}

#[test]
fn reach_is_bounded_by_hop_budget() {
    // reach 150 km => 1 hop. r0's battery covers r1 (1 hop) but NOT r2
    // (2 hops). Only r1 should be struck.
    let scenario = build_scenario(2, 3, 150.0, true, false);
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");

    let report = result
        .force_projection_reports
        .get(&FactionId::from("red"))
        .expect("red should have landed strikes");
    assert!(
        report.regions_struck.contains(&RegionId::from("r1")),
        "1-hop region must be struck"
    );
    assert!(
        !report.regions_struck.contains(&RegionId::from("r2")),
        "2-hop region must be out of a 1-hop reach; got {:?}",
        report.regions_struck
    );
}

#[test]
fn allied_factions_are_not_struck() {
    // Same reach as the hit test, but red and blue are mutually Allied.
    // The standoff strike must respect the same coupling combat does —
    // no strike should land.
    let scenario = build_scenario(3, 3, 300.0, true, true);
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");
    assert!(
        result.force_projection_reports.is_empty(),
        "mutually-allied target must not be struck; got: {:?}",
        result.force_projection_reports
    );
    // Blue keeps its full strength.
    let blue_strength = result
        .final_state
        .faction_states
        .get(&FactionId::from("blue"))
        .map(|fs| fs.total_strength)
        .expect("blue state present");
    assert!(
        (blue_strength - 2000.0).abs() < 1e-9,
        "allied blue should be untouched; got {blue_strength}"
    );
}

#[test]
fn no_projection_produces_no_strike_report() {
    // Identical map / factions, but red declares no force_projection.
    // The strike report must be empty — the gating / legacy-bit-identity
    // contract. (The cross-run report section elides on this signal.)
    let scenario = build_scenario(4, 5, 300.0, false, false);
    let mut engine = Engine::new(scenario).expect("scenario should validate");
    let result = engine.run().expect("run should succeed");
    assert!(
        result.force_projection_reports.is_empty(),
        "no force_projection → no strike report; got: {:?}",
        result.force_projection_reports
    );
}

#[test]
fn strike_reports_are_deterministic_across_runs() {
    let run = || {
        let scenario = build_scenario(99, 4, 300.0, true, false);
        let mut engine = Engine::new(scenario).expect("scenario should validate");
        engine
            .run()
            .expect("run should succeed")
            .force_projection_reports
    };
    let a = run();
    let b = run();
    assert_eq!(a, b, "same-seed runs must produce identical strike reports");
}

#[test]
fn validation_accepts_well_formed_projection() {
    let scenario = build_scenario(1, 3, 300.0, true, false);
    assert!(
        validate_scenario(&scenario).is_ok(),
        "well-formed standoff-strike projection must validate"
    );
}

#[test]
fn validation_rejects_zero_damage() {
    let mut scenario = build_scenario(1, 3, 300.0, true, false);
    if let Some(red) = scenario.factions.get_mut(&FactionId::from("red"))
        && let Some(battery) = red.forces.get_mut(&ForceId::from("battery"))
    {
        battery.force_projection = Some(ForceProjection::StandoffStrike {
            range: 300.0,
            damage: 0.0,
        });
    }
    validate_scenario(&scenario).expect_err("zero damage must reject");
}

#[test]
fn validation_rejects_negative_range() {
    let mut scenario = build_scenario(1, 3, 300.0, true, false);
    if let Some(red) = scenario.factions.get_mut(&FactionId::from("red"))
        && let Some(battery) = red.forces.get_mut(&ForceId::from("battery"))
    {
        battery.force_projection = Some(ForceProjection::StandoffStrike {
            range: -1.0,
            damage: 100.0,
        });
    }
    validate_scenario(&scenario).expect_err("negative range must reject");
}

#[test]
fn validation_rejects_nan_naval_range() {
    // Reserved variant, but still validated so a malformed reserved
    // declaration can't silently ship.
    let mut scenario = build_scenario(1, 3, 300.0, true, false);
    if let Some(red) = scenario.factions.get_mut(&FactionId::from("red"))
        && let Some(battery) = red.forces.get_mut(&ForceId::from("battery"))
    {
        battery.force_projection = Some(ForceProjection::Naval { range: f64::NAN });
    }
    validate_scenario(&scenario).expect_err("NaN naval range must reject");
}

#[test]
fn validation_accepts_reserved_airlift() {
    let mut scenario = build_scenario(1, 3, 300.0, true, false);
    if let Some(red) = scenario.factions.get_mut(&FactionId::from("red"))
        && let Some(battery) = red.forces.get_mut(&ForceId::from("battery"))
    {
        battery.force_projection = Some(ForceProjection::Airlift { capacity: 250.0 });
    }
    assert!(
        validate_scenario(&scenario).is_ok(),
        "well-formed reserved airlift must validate"
    );
}
