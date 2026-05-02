//! Regression tests for diplomatic-stance behavioral coupling.
//!
//! Epic D round-three item 1 (also closes R3-2 round-two item 2):
//! `Faction.diplomacy` was previously authored but unread by combat
//! and AI. These tests pin the now-live wiring so a refactor that
//! reverts it to a silent no-op fails loudly.
//!
//! Conventions follow the round-one audit suite
//! (`audit_unread_params.rs`): each test holds the scenario constant
//! and varies *only* the diplomatic stance, with the same RNG seed
//! across arms. Combat divergence between arms is therefore
//! attributable to the stance change alone.

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_engine::diplomacy::{ai_threat_multiplier, combat_blocked};
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::faction::{
    Diplomacy, DiplomaticStance, Faction, FactionType, ForceUnit, MilitaryBranch, UnitType,
};
use faultline_types::ids::{EventId, FactionId, ForceId, RegionId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;

// ---------------------------------------------------------------------------
// Shared fixture
// ---------------------------------------------------------------------------

fn make_region(id: &str, borders: Vec<RegionId>) -> Region {
    Region {
        id: RegionId::from(id),
        name: id.into(),
        population: 100_000,
        urbanization: 0.5,
        initial_control: None,
        strategic_value: 1.0,
        borders,
        centroid: None,
    }
}

fn make_force(id: &str, region: &RegionId, strength: f64) -> ForceUnit {
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: region.clone(),
        strength,
        mobility: 1.0,
        force_projection: None,
        upkeep: 1.0,
        morale_modifier: 0.0,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn make_faction(id: &str, region: &RegionId, diplomacy: Vec<DiplomaticStance>) -> Faction {
    let mut forces = BTreeMap::new();
    forces.insert(ForceId::from(id), make_force(id, region, 50.0));
    Faction {
        id: FactionId::from(id),
        name: id.into(),
        faction_type: FactionType::Military {
            branch: MilitaryBranch::Army,
        },
        description: String::new(),
        color: "#000000".into(),
        forces,
        tech_access: vec![],
        initial_morale: 0.8,
        logistics_capacity: 50.0,
        initial_resources: 1_000.0,
        resource_rate: 10.0,
        recruitment: None,
        command_resilience: 0.0,
        intelligence: 0.5,
        diplomacy,
        doctrine: Doctrine::Conventional,
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership: None,
        alliance_fracture: None,
    }
}

fn empty_scenario(seed: u64, max_ticks: u32) -> Scenario {
    let r1 = RegionId::from("r1");

    let mut regions = BTreeMap::new();
    regions.insert(r1.clone(), make_region("r1", vec![]));

    Scenario {
        meta: ScenarioMeta {
            name: "diplomacy_behavior".into(),
            description: String::new(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 1,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![TerrainModifier {
                region: r1,
                terrain_type: TerrainType::Rural,
                movement_modifier: 1.0,
                defense_modifier: 1.0,
                visibility: 1.0,
            }],
        },
        factions: BTreeMap::new(),
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
            snapshot_interval: 10,
        },
        victory_conditions: BTreeMap::new(),
        kill_chains: BTreeMap::new(),
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: BTreeMap::new(),
    }
}

/// Two factions co-located in the same region with equal strength.
/// `a_to_b` and `b_to_a` set the directional stances; the rest of
/// the scenario is bare-bones so the only force in play is combat.
fn colocated_scenario(a_to_b: Option<Diplomacy>, b_to_a: Option<Diplomacy>) -> Scenario {
    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");

    let dip_a = match a_to_b {
        Some(stance) => vec![DiplomaticStance {
            target_faction: FactionId::from("b"),
            stance,
        }],
        None => vec![],
    };
    let dip_b = match b_to_a {
        Some(stance) => vec![DiplomaticStance {
            target_faction: FactionId::from("a"),
            stance,
        }],
        None => vec![],
    };

    sc.factions
        .insert(FactionId::from("a"), make_faction("a", &r1, dip_a));
    sc.factions
        .insert(FactionId::from("b"), make_faction("b", &r1, dip_b));
    sc
}

// ---------------------------------------------------------------------------
// Combat semantics
// ---------------------------------------------------------------------------

/// Run engine to completion and return total surviving strength
/// across all factions. With combat happening, attrition reduces
/// this; with combat blocked, both factions retain full strength.
fn run_total_strength(sc: Scenario) -> f64 {
    let mut engine = Engine::with_seed(sc, 7).expect("engine init");
    engine.run().expect("run");
    engine
        .state()
        .faction_states
        .values()
        .map(|fs| fs.total_strength)
        .sum()
}

#[test]
fn neutral_default_pair_takes_combat_attrition() {
    // Empty diplomacy on both sides -> Neutral default -> combat happens.
    // Pins the backward-compat contract for every bundled scenario
    // that ships with `diplomacy = []`.
    let initial_total = 100.0; // 50 + 50
    let final_total = run_total_strength(colocated_scenario(None, None));
    assert!(
        final_total < initial_total,
        "Neutral default must still produce combat attrition; got total {final_total}"
    );
}

#[test]
fn mutually_allied_pair_skips_combat() {
    // Both sides view each other as Allied -> combat blocked ->
    // both factions retain ~full strength.
    let initial_total = 100.0;
    let final_total = run_total_strength(colocated_scenario(
        Some(Diplomacy::Allied),
        Some(Diplomacy::Allied),
    ));
    assert!(
        (final_total - initial_total).abs() < 0.001,
        "mutually-Allied pair must not take attrition; expected {initial_total}, got {final_total}"
    );
}

#[test]
fn cooperative_pair_still_fights() {
    // Cooperative is the soft-friendly tier — AI de-prioritizes,
    // but if forces collide, combat still happens.
    let initial_total = 100.0;
    let final_total = run_total_strength(colocated_scenario(
        Some(Diplomacy::Cooperative),
        Some(Diplomacy::Cooperative),
    ));
    assert!(
        final_total < initial_total,
        "Cooperative pair must still take combat attrition; got total {final_total}"
    );
}

#[test]
fn one_sided_alliance_does_not_block_combat() {
    // A views B as Allied, but B views A as Hostile -> combat happens.
    // Alliance-blocking requires reciprocity.
    let initial_total = 100.0;
    let final_total = run_total_strength(colocated_scenario(
        Some(Diplomacy::Allied),
        Some(Diplomacy::Hostile),
    ));
    assert!(
        final_total < initial_total,
        "one-sided alliance must not block combat; got total {final_total}"
    );
}

#[test]
fn diplomacy_change_event_flips_combat_block() {
    // Baseline: mutually Allied -> combat blocked.
    // A `DiplomacyChange` event fires on tick 1 setting both
    // directions to Hostile -> combat resumes.
    // Verifies the runtime override path consumed by `current_stance`.
    let mut sc = colocated_scenario(Some(Diplomacy::Allied), Some(Diplomacy::Allied));
    let event_id = EventId::from("break_alliance");
    sc.events.insert(
        event_id.clone(),
        EventDefinition {
            id: event_id,
            name: "break".into(),
            description: String::new(),
            earliest_tick: Some(1),
            latest_tick: Some(1),
            conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
            probability: 1.0,
            repeatable: false,
            effects: vec![
                EventEffect::DiplomacyChange {
                    faction_a: FactionId::from("a"),
                    faction_b: FactionId::from("b"),
                    new_stance: Diplomacy::Hostile,
                },
                EventEffect::DiplomacyChange {
                    faction_a: FactionId::from("b"),
                    faction_b: FactionId::from("a"),
                    new_stance: Diplomacy::Hostile,
                },
            ],
            chain: None,
            defender_options: vec![],
        },
    );

    let initial_total = 100.0;
    let final_total = run_total_strength(sc);
    assert!(
        final_total < initial_total,
        "post-event stance flip must resume combat; got total {final_total}"
    );
}

// ---------------------------------------------------------------------------
// Direct helper coverage: combat_blocked / ai_threat_multiplier
// ---------------------------------------------------------------------------

#[test]
fn combat_blocked_helper_reads_overrides() {
    // Engine init populates state.diplomacy_overrides empty, so
    // combat_blocked starts agreeing with the baseline. After we
    // inject an override, the helper picks it up.
    let sc = colocated_scenario(Some(Diplomacy::Allied), Some(Diplomacy::Allied));
    let mut engine = Engine::with_seed(sc.clone(), 7).expect("engine init");

    let a = FactionId::from("a");
    let b = FactionId::from("b");

    // Baseline: mutually Allied -> blocked.
    assert!(combat_blocked(engine.state(), engine.scenario(), &a, &b));

    // We cannot mutate state through the public API, but we can run
    // the engine for a tick and verify the helper agrees with the
    // resolved baseline view. Stronger override coverage lives in the
    // event-driven test above.
    engine.tick().expect("tick");
    assert!(combat_blocked(engine.state(), engine.scenario(), &a, &b));
}

#[test]
fn ai_threat_multiplier_matches_stance_tier() {
    // Build a 3-faction scenario so we can exercise multiple stance
    // values from a single perspective without running the engine.
    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");
    sc.factions.insert(
        FactionId::from("a"),
        make_faction(
            "a",
            &r1,
            vec![
                DiplomaticStance {
                    target_faction: FactionId::from("ally"),
                    stance: Diplomacy::Allied,
                },
                DiplomaticStance {
                    target_faction: FactionId::from("partner"),
                    stance: Diplomacy::Cooperative,
                },
            ],
        ),
    );
    sc.factions
        .insert(FactionId::from("ally"), make_faction("ally", &r1, vec![]));
    sc.factions.insert(
        FactionId::from("partner"),
        make_faction("partner", &r1, vec![]),
    );

    let engine = Engine::with_seed(sc, 7).expect("engine init");

    let a = FactionId::from("a");
    let ally = FactionId::from("ally");
    let partner = FactionId::from("partner");
    let unknown = FactionId::from("unknown"); // unlisted -> Neutral default

    assert_eq!(
        ai_threat_multiplier(engine.state(), engine.scenario(), &a, &ally),
        0.0
    );
    assert_eq!(
        ai_threat_multiplier(engine.state(), engine.scenario(), &a, &partner),
        faultline_engine::diplomacy::COOPERATIVE_AI_FACTOR
    );
    assert_eq!(
        ai_threat_multiplier(engine.state(), engine.scenario(), &a, &unknown),
        1.0
    );
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn validation_rejects_self_stance() {
    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");
    sc.factions.insert(
        FactionId::from("a"),
        make_faction(
            "a",
            &r1,
            vec![DiplomaticStance {
                target_faction: FactionId::from("a"),
                stance: Diplomacy::Allied,
            }],
        ),
    );
    let err = faultline_engine::validate_scenario(&sc).expect_err("self-stance must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("toward itself"),
        "expected diagnostic to mention self-targeting; got {msg}"
    );
}

#[test]
fn validation_rejects_unknown_target_faction() {
    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");
    sc.factions.insert(
        FactionId::from("a"),
        make_faction(
            "a",
            &r1,
            vec![DiplomaticStance {
                target_faction: FactionId::from("ghost"),
                stance: Diplomacy::Allied,
            }],
        ),
    );
    let err =
        faultline_engine::validate_scenario(&sc).expect_err("unknown target_faction must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("ghost"),
        "expected diagnostic to name the unknown faction; got {msg}"
    );
}

#[test]
fn validation_rejects_duplicate_target_factions() {
    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");
    sc.factions
        .insert(FactionId::from("b"), make_faction("b", &r1, vec![]));
    sc.factions.insert(
        FactionId::from("a"),
        make_faction(
            "a",
            &r1,
            vec![
                DiplomaticStance {
                    target_faction: FactionId::from("b"),
                    stance: Diplomacy::Allied,
                },
                DiplomaticStance {
                    target_faction: FactionId::from("b"),
                    stance: Diplomacy::Hostile,
                },
            ],
        ),
    );
    let err = faultline_engine::validate_scenario(&sc).expect_err("duplicate target must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("more than once"),
        "expected diagnostic to flag duplicate; got {msg}"
    );
}
