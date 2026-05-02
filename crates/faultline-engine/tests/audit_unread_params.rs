//! Regression tests for the unread-parameter audit.
//!
//! Each scenario-config field these tests exercise was once authored
//! by users (sometimes in many bundled scenarios with non-trivial
//! values) but had zero effect on simulation outcomes. The audit
//! wired them in; these tests pin the wiring so a refactor that
//! reverts the field to a silent no-op fails loudly.
//!
//! Conventions:
//! - Each test holds the scenario constant and varies *only* the
//!   parameter under test, so any divergence in outcome is
//!   attributable to that parameter alone.
//! - Same RNG seed across both arms of a comparison so non-parameter
//!   noise is eliminated by construction.

use std::collections::BTreeMap;

use faultline_engine::{Engine, tick, validate_scenario};
use faultline_types::campaign::{
    BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
};
use faultline_types::faction::{
    Faction, FactionType, ForceUnit, LeadershipCadre, LeadershipRank, MilitaryBranch, UnitType,
};
use faultline_types::ids::{FactionId, ForceId, KillChainId, PhaseId, RegionId, VictoryId};
use faultline_types::map::{
    Activation, EnvironmentSchedule, EnvironmentWindow, MapConfig, MapSource, Region,
    TerrainModifier, TerrainType,
};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::stats::PhaseOutcome;
use faultline_types::strategy::{Doctrine, FactionAction};
use faultline_types::victory::{VictoryCondition, VictoryType};

// ---------------------------------------------------------------------------
// Helpers
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

fn make_force(id: &str, region: &RegionId, strength: f64, morale_modifier: f64) -> ForceUnit {
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: region.clone(),
        strength,
        mobility: 1.0,
        force_projection: None,
        upkeep: 1.0,
        morale_modifier,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn make_faction(
    id: &str,
    forces: BTreeMap<ForceId, ForceUnit>,
    command_resilience: f64,
    leadership: Option<LeadershipCadre>,
) -> Faction {
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
        command_resilience,
        intelligence: 0.5,
        diplomacy: vec![],
        doctrine: Doctrine::Conventional,
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership,
        alliance_fracture: None,
    }
}

fn empty_scenario(seed: u64, max_ticks: u32) -> Scenario {
    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");

    let mut regions = BTreeMap::new();
    regions.insert(r1.clone(), make_region("r1", vec![r2.clone()]));
    regions.insert(r2.clone(), make_region("r2", vec![r1.clone()]));

    Scenario {
        meta: ScenarioMeta {
            name: "audit".into(),
            description: String::new(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 1,
            },
            regions,
            infrastructure: BTreeMap::new(),
            terrain: vec![
                TerrainModifier {
                    region: r1,
                    terrain_type: TerrainType::Rural,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                },
                TerrainModifier {
                    region: r2,
                    terrain_type: TerrainType::Rural,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 1.0,
                },
            ],
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

// ---------------------------------------------------------------------------
// Test 1: command_resilience attenuates leadership-decapitation morale shock
// ---------------------------------------------------------------------------

/// Build a scenario where a single-phase kill chain decapitates the
/// target faction with a fixed `morale_shock`. Run with varying
/// `command_resilience`; the final morale should reflect attenuation.
fn decapitation_scenario(command_resilience: f64) -> Scenario {
    let mut sc = empty_scenario(42, 5);

    let r1 = RegionId::from("r1");
    let attacker_id = FactionId::from("attacker");
    let target_id = FactionId::from("target");

    // Attacker faction (no special config).
    let mut atk_forces = BTreeMap::new();
    atk_forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r2"), 50.0, 0.0),
    );
    sc.factions.insert(
        attacker_id.clone(),
        make_faction("attacker", atk_forces, 0.0, None),
    );

    // Target faction has a 2-rank cadre, with succession_floor=1.0
    // and zero recovery ticks so the morale cap doesn't pile a second
    // adjustment on top of the shock — keeps the test focused on
    // exactly one effect (the shock attenuation).
    let mut tgt_forces = BTreeMap::new();
    tgt_forces.insert(ForceId::from("t"), make_force("t", &r1, 50.0, 0.0));
    let cadre = LeadershipCadre {
        ranks: vec![
            LeadershipRank {
                id: "principal".into(),
                name: "Principal".into(),
                effectiveness: 1.0,
                description: String::new(),
            },
            LeadershipRank {
                id: "deputy".into(),
                name: "Deputy".into(),
                effectiveness: 1.0,
                description: String::new(),
            },
        ],
        succession_recovery_ticks: 0,
        succession_floor: 1.0,
    };
    sc.factions.insert(
        target_id.clone(),
        make_faction("target", tgt_forces, command_resilience, Some(cadre)),
    );

    // Single-phase kill chain that auto-activates on tick 1, succeeds
    // on tick 2, and fires the LeadershipDecapitation output.
    let chain_id = KillChainId::from("decap");
    let strike = PhaseId::from("strike");
    let mut phases = BTreeMap::new();
    phases.insert(
        strike.clone(),
        CampaignPhase {
            id: strike.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost::default(),
            targets_domains: vec![],
            outputs: vec![PhaseOutput::LeadershipDecapitation {
                target_faction: target_id.clone(),
                morale_shock: 0.4,
            }],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    sc.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id,
            name: "Decap".into(),
            description: String::new(),
            attacker: attacker_id,
            target: target_id,
            entry_phase: strike,
            phases,
        },
    );

    sc
}

#[test]
fn command_resilience_zero_takes_full_morale_shock() {
    let sc = decapitation_scenario(0.0);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let final_morale = engine
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .morale;
    // Initial morale 0.8, full shock 0.4 -> expected 0.4.
    assert!(
        (0.35..=0.45).contains(&final_morale),
        "expected ~0.4 morale after full shock (resilience=0.0), got {final_morale}"
    );
}

#[test]
fn command_resilience_one_absorbs_morale_shock_entirely() {
    let sc = decapitation_scenario(1.0);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let final_morale = engine
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .morale;
    // Resilience 1.0 nullifies the shock entirely; morale stays near
    // the initial 0.8.
    assert!(
        (0.75..=0.85).contains(&final_morale),
        "expected ~0.8 morale with full resilience, got {final_morale}"
    );
}

#[test]
fn command_resilience_intermediate_attenuates_partially() {
    let sc = decapitation_scenario(0.5);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let final_morale = engine
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .morale;
    // Half resilience → half shock = 0.2 drop; expected ~0.6.
    assert!(
        (0.55..=0.65).contains(&final_morale),
        "expected ~0.6 morale with 0.5 resilience, got {final_morale}"
    );
}

#[test]
fn leadership_decapitation_advances_rank_regardless_of_resilience() {
    // Resilience attenuates the *morale shock* but must NOT prevent
    // the rank index from advancing — successor still takes the seat
    // even if morale is preserved.
    let sc = decapitation_scenario(1.0);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let rank = engine
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .current_leadership_rank;
    assert_eq!(
        rank, 1,
        "rank index must advance to deputy regardless of resilience"
    );
}

#[test]
fn command_resilience_nan_degrades_to_full_shock() {
    // A hand-built scenario that bypasses validation could inject a
    // non-finite `command_resilience`. The engine treats NaN as 0.0
    // (full shock) rather than letting it propagate through the morale
    // arithmetic and silently corrupt downstream combat values. This
    // mirrors the graceful-degradation pattern used for `morale_modifier`
    // in `tick::find_contested_regions` (where `(1.0 + NaN).max(0.0)`
    // resolves to 0.0 via IEEE-754 `fmax` semantics).
    let sc_nan = decapitation_scenario(f64::NAN);
    let mut engine_nan = Engine::with_seed(sc_nan, 42).expect("engine init");
    engine_nan.run().expect("run");
    let nan_morale = engine_nan
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .morale;
    assert!(
        nan_morale.is_finite(),
        "NaN command_resilience must not poison morale, got {nan_morale}"
    );

    // The post-strike morale under NaN resilience must match the
    // resilience-0.0 (full-shock) arm exactly, since NaN is treated
    // as 0.0.
    let sc_zero = decapitation_scenario(0.0);
    let mut engine_zero = Engine::with_seed(sc_zero, 42).expect("engine init");
    engine_zero.run().expect("run");
    let zero_morale = engine_zero
        .state()
        .faction_states
        .get(&FactionId::from("target"))
        .expect("target state")
        .morale;
    assert!(
        (nan_morale - zero_morale).abs() < 1e-9,
        "NaN resilience must equal 0.0 resilience: nan={nan_morale}, zero={zero_morale}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: ForceUnit.morale_modifier multiplies effective combat strength
// ---------------------------------------------------------------------------

fn combat_scenario(alpha_morale_mod: f64) -> Scenario {
    let mut sc = empty_scenario(42, 30);

    let r1 = RegionId::from("r1");

    let mut alpha_forces = BTreeMap::new();
    alpha_forces.insert(
        ForceId::from("a_inf"),
        make_force("a_inf", &r1, 100.0, alpha_morale_mod),
    );
    let alpha = make_faction("alpha", alpha_forces, 0.0, None);

    let mut bravo_forces = BTreeMap::new();
    bravo_forces.insert(ForceId::from("b_inf"), make_force("b_inf", &r1, 100.0, 0.0));
    let bravo = make_faction("bravo", bravo_forces, 0.0, None);

    sc.factions.insert(FactionId::from("alpha"), alpha);
    sc.factions.insert(FactionId::from("bravo"), bravo);

    sc.victory_conditions.insert(
        VictoryId::from("alpha_dominate"),
        VictoryCondition {
            id: VictoryId::from("alpha_dominate"),
            name: "Alpha Dominates".into(),
            faction: FactionId::from("alpha"),
            condition: VictoryType::MilitaryDominance {
                enemy_strength_below: 1.0,
            },
        },
    );

    sc
}

#[test]
fn morale_modifier_zero_leaves_strength_symmetric() {
    let sc = combat_scenario(0.0);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let alpha = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha");
    let bravo = engine
        .state()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo");
    let alpha_strength: f64 = alpha.forces.values().map(|f| f.strength).sum();
    let bravo_strength: f64 = bravo.forces.values().map(|f| f.strength).sum();
    let ratio = if bravo_strength > 0.01 {
        alpha_strength / bravo_strength
    } else {
        1.0
    };
    assert!(
        (0.85..=1.15).contains(&ratio),
        "expected near-symmetric outcome with 0.0 modifier, got alpha={alpha_strength} bravo={bravo_strength}"
    );
}

#[test]
fn morale_modifier_positive_advantages_combat_outcome() {
    // Alpha gets +0.5 morale_modifier (50% effective-strength boost);
    // it should consistently end up stronger than bravo at run end
    // for the same seed.
    let sc = combat_scenario(0.5);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let alpha = engine
        .state()
        .faction_states
        .get(&FactionId::from("alpha"))
        .expect("alpha");
    let bravo = engine
        .state()
        .faction_states
        .get(&FactionId::from("bravo"))
        .expect("bravo");
    let alpha_strength: f64 = alpha.forces.values().map(|f| f.strength).sum();
    let bravo_strength: f64 = bravo.forces.values().map(|f| f.strength).sum();
    assert!(
        alpha_strength > bravo_strength + 5.0,
        "alpha with 0.5 morale_modifier should outlast bravo by a clear margin; got alpha={alpha_strength} bravo={bravo_strength}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Scenario.defender_budget gates detection past the overrun
// ---------------------------------------------------------------------------

fn defender_budget_scenario(budget: Option<f64>, seed: u64) -> Scenario {
    let mut sc = empty_scenario(seed, 50);

    sc.defender_budget = budget;

    let r1 = RegionId::from("r1");
    let attacker_id = FactionId::from("attacker");
    let target_id = FactionId::from("target");

    let mut attacker_forces = BTreeMap::new();
    attacker_forces.insert(ForceId::from("a"), make_force("a", &r1, 50.0, 0.0));
    sc.factions.insert(
        attacker_id.clone(),
        make_faction("attacker", attacker_forces, 0.0, None),
    );

    let mut target_forces = BTreeMap::new();
    target_forces.insert(ForceId::from("t"), make_force("t", &r1, 50.0, 0.0));
    sc.factions.insert(
        target_id.clone(),
        make_faction("target", target_forces, 0.0, None),
    );

    let chain_id = KillChainId::from("chain");
    let p1 = PhaseId::from("p1_setup");
    let p2 = PhaseId::from("p2_exploit");

    let mut phases = BTreeMap::new();
    phases.insert(
        p1.clone(),
        CampaignPhase {
            id: p1.clone(),
            name: "Setup".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 1.0,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.9,
            cost: PhaseCost {
                attacker_dollars: 100.0,
                // Sized to overrun the budget on its own.
                defender_dollars: 1_000_000.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: p2.clone(),
            }],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );
    phases.insert(
        p2.clone(),
        CampaignPhase {
            id: p2.clone(),
            name: "Exploit".into(),
            description: String::new(),
            prerequisites: vec![p1.clone()],
            base_success_probability: 0.0,
            // Short, modest-detection phase so that no-overrun and
            // overrun arms separate on cumulative detection
            // probability without saturating at 1.0 in either case.
            // Per-run cumulative detection chance:
            //   no-overrun: 1 - (1 - 0.10)^5 ≈ 0.41
            //   overrun:    1 - (1 - 0.05)^5 ≈ 0.23
            // — clearly distinguishable across 32 seeds.
            min_duration: 5,
            max_duration: 5,
            detection_probability_per_tick: 0.1,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 100.0,
                defender_dollars: 100.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        },
    );

    sc.kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id,
            name: "Test Chain".into(),
            description: String::new(),
            attacker: attacker_id,
            target: target_id,
            entry_phase: p1,
            phases,
        },
    );

    sc
}

#[test]
fn defender_budget_unset_does_not_engage_overrun_logic() {
    let sc = defender_budget_scenario(None, 42);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    assert_eq!(
        engine.state().defender_over_budget_tick,
        None,
        "no budget = no overrun"
    );
}

#[test]
fn defender_budget_overrun_latches_first_overrun_tick() {
    // Tight budget -> phase 1's defender_dollars (1M) blows past the
    // 100k cap on the tick phase 1 succeeds. The latch should fire.
    let sc = defender_budget_scenario(Some(100_000.0), 42);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    let tick = engine.state().defender_over_budget_tick;
    assert!(
        tick.is_some(),
        "expected over-budget latch to fire; got None"
    );
}

#[test]
fn defender_budget_within_cap_does_not_engage_overrun_logic() {
    // Generous budget -> phase 1's 1M still fits under 10M, so the
    // latch never fires.
    let sc = defender_budget_scenario(Some(10_000_000.0), 42);
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    engine.run().expect("run");
    assert_eq!(
        engine.state().defender_over_budget_tick,
        None,
        "spend within budget = no overrun"
    );
}

#[test]
fn defender_budget_overrun_reduces_post_overrun_detection_rate() {
    // Statistical regression: across many seeds, an overrun scenario
    // should detect the phase-2 attack later (or not at all) than the
    // identical scenario with a generous budget.
    let mut detected_no_overrun = 0;
    let mut detected_overrun = 0;
    let runs = 32;

    for seed in 0..runs {
        let sc_relaxed = defender_budget_scenario(Some(10_000_000.0), seed);
        let mut e = Engine::with_seed(sc_relaxed, seed).expect("engine init");
        let result = e.run().expect("run");
        if let Some(chain) = result.campaign_reports.get(&KillChainId::from("chain"))
            && let Some(outcome) = chain.phase_outcomes.get(&PhaseId::from("p2_exploit"))
            && matches!(outcome, PhaseOutcome::Detected { .. })
        {
            detected_no_overrun += 1;
        }

        let sc_tight = defender_budget_scenario(Some(100_000.0), seed);
        let mut e = Engine::with_seed(sc_tight, seed).expect("engine init");
        let result = e.run().expect("run");
        if let Some(chain) = result.campaign_reports.get(&KillChainId::from("chain"))
            && let Some(outcome) = chain.phase_outcomes.get(&PhaseId::from("p2_exploit"))
            && matches!(outcome, PhaseOutcome::Detected { .. })
        {
            detected_overrun += 1;
        }
    }

    // Across 32 runs we expect a clear, monotone reduction once the
    // 0.5× over-budget multiplier is applied to detection_probability.
    // We assert the unambiguous direction (strictly fewer detections
    // under overrun) rather than a specific quantity, since per-seed
    // counts can fluctuate around the mean.
    assert!(
        detected_overrun < detected_no_overrun,
        "expected fewer detections when defender is over budget; \
         got overrun={detected_overrun} no_overrun={detected_no_overrun}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: ForceUnit.mobility / TerrainModifier.movement_modifier /
//          EnvironmentWindow.movement_factor gate per-tick movement
// ---------------------------------------------------------------------------
//
// All three fields were previously silent no-ops. They now compose
// multiplicatively into a per-tick "effective mobility" that
// drives a `move_progress` accumulator on the unit; a queued
// `MoveUnit` action only fires when the accumulator reaches `1.0`.
// The tests below pin the rate-gating semantics by counting how many
// queued attempts it takes for a unit to actually move under various
// (mobility, terrain, env) combinations.

/// Build a 4-region chain (r1 - r2 - r3 - r4) with one alpha unit at
/// `r1` and no enemies / kill chains. Caller customizes mobility and
/// the terrain.movement_modifier on r1 (the source region for moves).
fn movement_rate_scenario(
    mobility: f64,
    terrain_movement_modifier: f64,
    environment: EnvironmentSchedule,
) -> Scenario {
    let mut sc = empty_scenario(42, 50);

    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");
    let r3 = RegionId::from("r3");
    let r4 = RegionId::from("r4");

    let mut regions = BTreeMap::new();
    regions.insert(r1.clone(), make_region("r1", vec![r2.clone()]));
    regions.insert(r2.clone(), make_region("r2", vec![r1.clone(), r3.clone()]));
    regions.insert(r3.clone(), make_region("r3", vec![r2.clone(), r4.clone()]));
    regions.insert(r4.clone(), make_region("r4", vec![r3.clone()]));
    sc.map.regions = regions;

    sc.map.terrain = vec![
        TerrainModifier {
            region: r1.clone(),
            terrain_type: TerrainType::Rural,
            movement_modifier: terrain_movement_modifier,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: r2,
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: r3,
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
        TerrainModifier {
            region: r4,
            terrain_type: TerrainType::Rural,
            movement_modifier: 1.0,
            defense_modifier: 1.0,
            visibility: 1.0,
        },
    ];

    sc.environment = environment;

    let mut force = make_force("u", &r1, 50.0, 0.0);
    force.mobility = mobility;
    let mut forces = BTreeMap::new();
    forces.insert(ForceId::from("u"), force);
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    sc
}

/// Initialize an Engine, then call `tick::movement_phase` `attempts`
/// times with a queued `MoveUnit u → r2` action. Returns the number
/// of attempts on which the unit actually changed region (i.e. moves
/// that succeeded). Bypasses the AI to isolate the rate-gate
/// behavior from decision logic.
fn count_successful_moves(scenario: Scenario, attempts: u32) -> u32 {
    let map = faultline_geo::load_map(&scenario.map).expect("map loads");
    let engine = Engine::with_seed(scenario.clone(), 42).expect("engine init");
    let mut state = engine.state().clone();

    let alpha = FactionId::from("alpha");
    let force = ForceId::from("u");
    let dest = RegionId::from("r2");
    let queued = BTreeMap::from([(
        alpha.clone(),
        vec![FactionAction::MoveUnit {
            force: force.clone(),
            destination: dest.clone(),
        }],
    )]);

    let mut moves = 0;
    for tick_n in 1..=attempts {
        // Reset the unit back to r1 between attempts so we keep
        // queueing the same r1→r2 move; the test is about *rate*,
        // not about chained-region traversal.
        if let Some(fs) = state.faction_states.get_mut(&alpha)
            && let Some(unit) = fs.forces.get_mut(&force)
        {
            unit.region = RegionId::from("r1");
        }
        state.tick = tick_n;
        tick::movement_phase(&mut state, &scenario, &map, &queued);
        let after_region = state
            .faction_states
            .get(&alpha)
            .and_then(|fs| fs.forces.get(&force))
            .map(|u| u.region.clone());
        if after_region == Some(dest.clone()) {
            moves += 1;
        }
    }
    moves
}

#[test]
fn mobility_one_moves_every_tick_legacy_default() {
    // mobility=1, terrain=1, no env — the legacy baseline. Every
    // attempt should succeed.
    let sc = movement_rate_scenario(1.0, 1.0, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 10);
    assert_eq!(
        moves, 10,
        "default mobility=1.0 must preserve legacy 1-region-per-tick movement"
    );
}

#[test]
fn mobility_half_halves_movement_rate() {
    // mobility=0.5 — accumulator hits 1.0 every other attempt.
    let sc = movement_rate_scenario(0.5, 1.0, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 10);
    assert_eq!(
        moves, 5,
        "mobility=0.5 should produce one successful move per two attempts"
    );
}

#[test]
fn mobility_zero_freezes_unit() {
    // mobility=0 — accumulator never grows, unit never moves.
    let sc = movement_rate_scenario(0.0, 1.0, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 10);
    assert_eq!(moves, 0, "mobility=0 must freeze the unit entirely");
}

#[test]
fn mobility_above_one_capped_at_one_per_tick() {
    // The accumulator caps at 1.0 to prevent saved-up moves; a
    // mobility>1 unit still moves every tick (no faster).
    let sc = movement_rate_scenario(2.0, 1.0, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 10);
    assert_eq!(
        moves, 10,
        "mobility>1 should still move every tick (capped, not faster)"
    );
}

#[test]
fn terrain_movement_modifier_scales_rate() {
    // terrain modifier 0.5 on the source region — same effect as
    // mobility 0.5: half-rate movement.
    let sc = movement_rate_scenario(1.0, 0.5, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 10);
    assert_eq!(
        moves, 5,
        "terrain.movement_modifier=0.5 should halve the movement rate"
    );
}

#[test]
fn environment_movement_factor_scales_rate() {
    // Active env window with movement_factor=0.5 — same effect as
    // halving mobility or the terrain modifier.
    let env = EnvironmentSchedule {
        windows: vec![EnvironmentWindow {
            id: "storm".into(),
            name: "Storm".into(),
            activation: Activation::Always,
            applies_to: vec![],
            movement_factor: 0.5,
            defense_factor: 1.0,
            visibility_factor: 1.0,
            detection_factor: 1.0,
        }],
    };
    let sc = movement_rate_scenario(1.0, 1.0, env);
    let moves = count_successful_moves(sc, 10);
    assert_eq!(
        moves, 5,
        "active env window with movement_factor=0.5 should halve the movement rate"
    );
}

#[test]
fn movement_factors_compose_multiplicatively() {
    // mobility 0.5 × terrain 0.5 = effective 0.25 → moves every 4
    // attempts. Pins the multiplicative composition contract.
    let sc = movement_rate_scenario(0.5, 0.5, EnvironmentSchedule::default());
    let moves = count_successful_moves(sc, 12);
    assert_eq!(
        moves, 3,
        "mobility=0.5 × terrain=0.5 → 0.25 effective → 1 move per 4 attempts (12 / 4 = 3)"
    );
}

#[test]
fn validate_rejects_negative_mobility() {
    let sc = movement_rate_scenario(-0.5, 1.0, EnvironmentSchedule::default());
    let err = validate_scenario(&sc).expect_err("negative mobility must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("mobility"),
        "expected mobility-related rejection, got `{msg}`"
    );
}

#[test]
fn validate_rejects_nan_mobility() {
    let sc = movement_rate_scenario(f64::NAN, 1.0, EnvironmentSchedule::default());
    let err = validate_scenario(&sc).expect_err("NaN mobility must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("mobility"),
        "expected mobility-related rejection, got `{msg}`"
    );
}

#[test]
fn validate_rejects_negative_terrain_movement_modifier() {
    let sc = movement_rate_scenario(1.0, -0.1, EnvironmentSchedule::default());
    let err =
        validate_scenario(&sc).expect_err("negative terrain.movement_modifier must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("movement_modifier"),
        "expected movement_modifier-related rejection, got `{msg}`"
    );
}

#[test]
fn validate_rejects_authored_move_progress() {
    // `move_progress` is engine runtime state — `#[serde(default)]`
    // keeps legacy TOML loading unchanged, but a non-zero authored
    // value would silently pre-warm the accumulator and cause the
    // first queued move to fire on tick 1 regardless of mobility /
    // terrain / env. Reject loudly at load time.
    let mut sc = movement_rate_scenario(1.0, 1.0, EnvironmentSchedule::default());
    let alpha = FactionId::from("alpha");
    let unit = ForceId::from("u");
    sc.factions
        .get_mut(&alpha)
        .expect("alpha faction")
        .forces
        .get_mut(&unit)
        .expect("unit u")
        .move_progress = 0.9;
    let err = validate_scenario(&sc).expect_err("authored move_progress must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("move_progress"),
        "expected move_progress-related rejection, got `{msg}`"
    );
}

// ---------------------------------------------------------------------------
// Population-segment activation + media-landscape wiring
// ---------------------------------------------------------------------------

/// Build a scenario with one population segment whose top sympathy is
/// already at the activation threshold so the very first political-
/// phase tick latches the segment. The MediaLandscape is parameterised
/// so a single test can exercise both the no-amp baseline and the
/// max-amp arm without two near-identical builders.
fn segment_activation_scenario(
    media: faultline_types::politics::MediaLandscape,
    activation_threshold: f64,
    sympathy: f64,
    volatility: f64,
) -> Scenario {
    use faultline_types::ids::SegmentId;
    use faultline_types::politics::{
        CivilianAction, FactionSympathy, PoliticalClimate, PopulationSegment,
    };

    let mut sc = empty_scenario(7, 5);
    let r1 = RegionId::from("r1");

    // One faction so sympathy has a target.
    let mut forces = BTreeMap::new();
    forces.insert(ForceId::from("a"), make_force("a", &r1, 50.0, 0.0));
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    sc.political_climate = PoliticalClimate {
        tension: 0.5,
        institutional_trust: 0.5,
        media_landscape: media,
        population_segments: vec![PopulationSegment {
            id: SegmentId::from("urban"),
            name: "Urban Core".into(),
            fraction: 0.4,
            concentrated_in: vec![r1],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy,
            }],
            activation_threshold,
            activation_actions: vec![
                CivilianAction::Protest { intensity: 0.3 },
                CivilianAction::Intelligence {
                    target_faction: FactionId::from("alpha"),
                    quality: 0.4,
                },
            ],
            volatility,
            activated: false,
        }],
        global_modifiers: vec![],
    };
    sc
}

#[test]
fn population_segment_activation_records_event() {
    // Sympathy starts safely above the threshold so the small per-tick
    // tension-pull drift (one-active-faction scenarios slowly cool
    // tension toward zero) can't push it below before the activation
    // latch fires. Volatility = 0 zeros the noise term, so on the first
    // political-phase tick the latch trips and records the event with
    // the discriminant names of both authored actions in order.
    let media = MediaLandscape {
        fragmentation: 0.0,
        disinformation_susceptibility: 0.0,
        state_control: 0.0,
        social_media_penetration: 0.0,
        internet_availability: 0.0,
    };
    let sc = segment_activation_scenario(media, 0.5, 0.6, 0.0);
    let mut engine = Engine::with_seed(sc, 7).expect("engine init");
    let result = engine.run().expect("run");

    assert_eq!(
        result.civilian_activations.len(),
        1,
        "exactly one activation expected, got {:?}",
        result.civilian_activations
    );
    let ev = &result.civilian_activations[0];
    assert_eq!(ev.segment.0, "urban");
    assert_eq!(ev.favored_faction.0, "alpha");
    assert_eq!(
        ev.action_kinds,
        vec!["Protest".to_string(), "Intelligence".to_string()],
        "action_kinds should preserve authored order with stable discriminant strings"
    );
}

#[test]
fn media_fragmentation_amplifies_sympathy_drift() {
    // Two scenarios: identical except for `fragmentation`. With high
    // fragmentation the noise term is amplified ~1.5×. Same RNG seed,
    // same volatility, so the only difference between final sympathy
    // values is the fragmentation multiplier.
    use faultline_types::ids::SegmentId;
    let zero_frag = MediaLandscape {
        fragmentation: 0.0,
        disinformation_susceptibility: 0.0,
        state_control: 0.0,
        social_media_penetration: 0.0,
        internet_availability: 0.0,
    };
    let high_frag = MediaLandscape {
        fragmentation: 1.0,
        ..zero_frag
    };
    // Set sympathy below the threshold so the segment doesn't activate
    // (we want to read the post-tick sympathy, not the latch).
    let sc_a = segment_activation_scenario(zero_frag, 0.99, 0.0, 1.0);
    let sc_b = segment_activation_scenario(high_frag, 0.99, 0.0, 1.0);

    let mut a = Engine::with_seed(sc_a, 7).expect("engine init");
    let mut b = Engine::with_seed(sc_b, 7).expect("engine init");
    a.run().expect("run a");
    b.run().expect("run b");

    let sym_a = a
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("urban"))
        .expect("segment a")
        .sympathies[0]
        .sympathy;
    let sym_b = b
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("urban"))
        .expect("segment b")
        .sympathies[0]
        .sympathy;
    // Same seed, but the fragmentation multiplier scales the noise
    // sample by 1.5×, so the two final sympathies cannot be equal.
    // (We can't predict the sign — RNG output drives that — but the
    // magnitudes must differ.)
    assert!(
        (sym_a - sym_b).abs() > 1e-9,
        "fragmentation must affect drift trajectory: sym_a={sym_a}, sym_b={sym_b}"
    );
}

#[test]
fn internet_gates_social_media_amplification() {
    // social_media_penetration=1.0 with internet_availability=0.0
    // must reproduce the no-amp baseline exactly — `effective_social_media`
    // is the product. This pins that internet outages neutralize the
    // social-media wiring (the "lights out" guard).
    use faultline_types::ids::SegmentId;
    let no_internet = MediaLandscape {
        fragmentation: 0.0,
        disinformation_susceptibility: 0.0,
        state_control: 0.0,
        social_media_penetration: 1.0,
        internet_availability: 0.0,
    };
    let baseline = MediaLandscape {
        fragmentation: 0.0,
        disinformation_susceptibility: 0.0,
        state_control: 0.0,
        social_media_penetration: 0.0,
        internet_availability: 0.0,
    };

    let sc_no_inet = segment_activation_scenario(no_internet, 0.99, 0.0, 1.0);
    let sc_base = segment_activation_scenario(baseline, 0.99, 0.0, 1.0);

    let mut a = Engine::with_seed(sc_no_inet, 7).expect("engine init");
    let mut b = Engine::with_seed(sc_base, 7).expect("engine init");
    a.run().expect("run a");
    b.run().expect("run b");

    let sym_a = a
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("urban"))
        .expect("segment a")
        .sympathies[0]
        .sympathy;
    let sym_b = b
        .state()
        .political_climate
        .population_segments
        .iter()
        .find(|s| s.id == SegmentId::from("urban"))
        .expect("segment b")
        .sympathies[0]
        .sympathy;
    assert!(
        (sym_a - sym_b).abs() < 1e-12,
        "internet=0 must zero out social-media amplification: sym_a={sym_a}, sym_b={sym_b}"
    );
}

/// Build a minimally-valid scenario with one faction (so validation
/// gets past the "no factions" check and reaches the media-landscape /
/// population-segment guards under test).
fn populated_scenario_for_validation() -> Scenario {
    let mut sc = empty_scenario(1, 5);
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r1"), 1.0, 0.0),
    );
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );
    sc
}

#[test]
fn validate_rejects_media_landscape_out_of_range() {
    let mut sc = populated_scenario_for_validation();
    sc.political_climate.media_landscape.fragmentation = 1.5;
    let err = validate_scenario(&sc).expect_err("out-of-range fragmentation must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("fragmentation"),
        "expected fragmentation-related rejection, got `{msg}`"
    );
}

#[test]
fn validate_rejects_media_landscape_nan() {
    let mut sc = populated_scenario_for_validation();
    sc.political_climate
        .media_landscape
        .social_media_penetration = f64::NAN;
    let err = validate_scenario(&sc).expect_err("NaN social_media_penetration must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("social_media_penetration"),
        "expected social-media-related rejection, got `{msg}`"
    );
}

#[test]
fn validate_rejects_population_segment_with_unknown_region() {
    use faultline_types::ids::SegmentId;
    use faultline_types::politics::{FactionSympathy, PopulationSegment};

    let mut sc = empty_scenario(1, 5);
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r1"), 1.0, 0.0),
    );
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    sc.political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("ghost"),
            name: "Ghost Region".into(),
            fraction: 0.1,
            concentrated_in: vec![RegionId::from("nowhere")],
            sympathies: vec![FactionSympathy {
                faction: FactionId::from("alpha"),
                sympathy: 0.0,
            }],
            activation_threshold: 0.5,
            activation_actions: vec![],
            volatility: 0.5,
            activated: false,
        });

    let err = validate_scenario(&sc).expect_err("unknown segment region must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("nowhere") && msg.contains("region"),
        "expected unknown-region rejection naming `nowhere`, got `{msg}`"
    );
}

#[test]
fn validate_rejects_duplicate_segment_ids() {
    use faultline_types::ids::SegmentId;
    use faultline_types::politics::{FactionSympathy, PopulationSegment};

    let mut sc = empty_scenario(1, 5);
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r1"), 1.0, 0.0),
    );
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    let seg = |id: &str| PopulationSegment {
        id: SegmentId::from(id),
        name: id.into(),
        fraction: 0.1,
        concentrated_in: vec![],
        sympathies: vec![FactionSympathy {
            faction: FactionId::from("alpha"),
            sympathy: 0.0,
        }],
        activation_threshold: 0.5,
        activation_actions: vec![],
        volatility: 0.5,
        activated: false,
    };
    sc.political_climate.population_segments.push(seg("dup"));
    sc.political_climate.population_segments.push(seg("dup"));

    let err = validate_scenario(&sc).expect_err("duplicate segment ids must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("dup"),
        "expected duplicate-id rejection naming `dup`, got `{msg}`"
    );
}

#[test]
fn validate_rejects_population_segment_with_empty_sympathies() {
    use faultline_types::ids::SegmentId;
    use faultline_types::politics::PopulationSegment;

    let mut sc = empty_scenario(1, 5);
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r1"), 1.0, 0.0),
    );
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    sc.political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("voiceless"),
            name: "Voiceless".into(),
            fraction: 0.1,
            concentrated_in: vec![],
            sympathies: vec![],
            activation_threshold: 0.5,
            activation_actions: vec![],
            volatility: 0.5,
            activated: false,
        });

    let err = validate_scenario(&sc).expect_err("empty sympathies must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("voiceless") && msg.contains("sympathies"),
        "expected empty-sympathies rejection naming `voiceless`, got `{msg}`"
    );
}

#[test]
fn validate_rejects_population_segment_with_duplicate_sympathy_factions() {
    use faultline_types::ids::SegmentId;
    use faultline_types::politics::{FactionSympathy, PopulationSegment};

    let mut sc = empty_scenario(1, 5);
    let mut forces = BTreeMap::new();
    forces.insert(
        ForceId::from("a"),
        make_force("a", &RegionId::from("r1"), 1.0, 0.0),
    );
    sc.factions.insert(
        FactionId::from("alpha"),
        make_faction("alpha", forces, 0.0, None),
    );

    sc.political_climate
        .population_segments
        .push(PopulationSegment {
            id: SegmentId::from("doubled"),
            name: "Doubled".into(),
            fraction: 0.1,
            concentrated_in: vec![],
            sympathies: vec![
                FactionSympathy {
                    faction: FactionId::from("alpha"),
                    sympathy: 0.5,
                },
                FactionSympathy {
                    faction: FactionId::from("alpha"),
                    sympathy: 0.3,
                },
            ],
            activation_threshold: 0.5,
            activation_actions: vec![],
            volatility: 0.5,
            activated: false,
        });

    let err = validate_scenario(&sc).expect_err("duplicate sympathy factions must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("doubled") && msg.contains("alpha"),
        "expected duplicate-sympathy rejection naming `doubled` and `alpha`, got `{msg}`"
    );
}

#[test]
fn segment_activation_determinism_pinned_by_seed() {
    // Two engine runs at the same seed must produce bit-identical
    // civilian_activations logs — the determinism contract for the
    // new RunResult field. The engine state was sized for one segment
    // so the field is non-trivially populated.
    let media = MediaLandscape {
        fragmentation: 0.5,
        disinformation_susceptibility: 0.0,
        state_control: 0.0,
        social_media_penetration: 0.7,
        internet_availability: 0.9,
    };
    let sc = segment_activation_scenario(media, 0.5, 0.5, 0.5);
    let mut a = Engine::with_seed(sc.clone(), 99).expect("engine a");
    let mut b = Engine::with_seed(sc, 99).expect("engine b");
    let ra = a.run().expect("run a");
    let rb = b.run().expect("run b");
    assert_eq!(
        ra.civilian_activations, rb.civilian_activations,
        "civilian_activations must be deterministic across same-seed runs"
    );
}

// ---------------------------------------------------------------------------
// Test 6: TechCard cost wiring (deployment_cost / cost_per_tick / coverage_limit)
// ---------------------------------------------------------------------------
//
// Three previously-silent fields:
// - `deployment_cost`: deducted at engine init from each faction's
//   `initial_resources` for every card the faction can afford.
//   Cards the faction can't afford are *denied* (skipped), recorded
//   for the post-run report.
// - `cost_per_tick`: deducted in the attrition phase per-tech.
//   Cards whose maintenance can't be paid are *decommissioned*
//   (removed from `tech_deployed`), permanent for the rest of the
//   run.
// - `coverage_limit`: caps the per-tick number of (region, opponent)
//   pairs the card touches. Once saturated this tick, additional
//   applications are skipped.
//
// Each test holds the rest of the scenario constant and varies only
// the field under test so any divergence in outcome is attributable
// to the wired field alone.

mod tech_costs {
    use super::*;

    use faultline_types::ids::TechCardId;
    use faultline_types::tech::{TechCard, TechCategory, TechEffect};

    fn make_tech(
        id: &str,
        deployment_cost: f64,
        cost_per_tick: f64,
        coverage_limit: Option<u32>,
        combat_factor: f64,
    ) -> TechCard {
        TechCard {
            id: TechCardId::from(id),
            name: id.into(),
            description: String::new(),
            category: TechCategory::Custom("test".into()),
            effects: vec![TechEffect::CombatModifier {
                factor: combat_factor,
            }],
            cost_per_tick,
            deployment_cost,
            countered_by: vec![],
            terrain_modifiers: vec![],
            coverage_limit,
        }
    }

    /// Two-faction scenario that gives `alpha` the tech roster `cards`
    /// in declaration order, with `initial_resources` and
    /// `resource_rate`. A passive `bravo` faction sits in r2 so the
    /// "last faction standing" victory check doesn't terminate the run
    /// at tick 1. Forces never overlap regions so there is no combat
    /// — the only mutators of alpha's `resources` are upkeep, income,
    /// and tech costs.
    fn scenario_with_techs(
        cards: Vec<TechCard>,
        initial_resources: f64,
        resource_rate: f64,
        max_ticks: u32,
    ) -> Scenario {
        let mut sc = empty_scenario(7, max_ticks);
        let r1 = RegionId::from("r1");
        let r2 = RegionId::from("r2");

        let mut alpha_forces = BTreeMap::new();
        let mut alpha_force = make_force("u", &r1, 50.0, 0.0);
        alpha_force.upkeep = 0.0;
        alpha_forces.insert(ForceId::from("u"), alpha_force);
        let mut alpha = make_faction("alpha", alpha_forces, 0.0, None);
        alpha.initial_resources = initial_resources;
        alpha.resource_rate = resource_rate;
        alpha.tech_access = cards.iter().map(|c| c.id.clone()).collect();
        sc.factions.insert(FactionId::from("alpha"), alpha);

        // Passive opponent in r2 so "last faction standing" doesn't
        // fire on tick 1. Zero upkeep so its survival costs don't
        // matter; we never read its resources.
        let mut bravo_forces = BTreeMap::new();
        let mut bravo_force = make_force("v", &r2, 50.0, 0.0);
        bravo_force.upkeep = 0.0;
        bravo_forces.insert(ForceId::from("v"), bravo_force);
        let bravo = make_faction("bravo", bravo_forces, 0.0, None);
        sc.factions.insert(FactionId::from("bravo"), bravo);

        for card in cards {
            sc.technology.insert(card.id.clone(), card);
        }
        sc
    }

    /// Drive a single combat phase against a scenario and return the
    /// post-combat state. Skips movement / decision / political /
    /// campaign phases so unit positions can't be perturbed by the AI
    /// between init and the combat snapshot — the coverage tests
    /// depend on a known set of contested regions, and the AI is
    /// allowed (under default doctrine) to move units mid-tick before
    /// combat resolves.
    fn run_combat_only(sc: Scenario) -> faultline_engine::SimulationState {
        use rand::SeedableRng;
        let engine = Engine::with_seed(sc.clone(), 1).expect("engine init");
        let mut state = engine.state().clone();
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
        tick::combat_phase(&mut state, &sc, &mut rng);
        state
    }

    // Deployment-cost tests --------------------------------------------------

    #[test]
    fn deployment_cost_deducted_from_initial_resources_at_init() {
        let card = make_tech("alpha_drone", 25.0, 0.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 1);
        let engine = Engine::with_seed(sc, 1).expect("engine init");
        let fs = engine
            .state()
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        // 100 initial - 25 deployment = 75 left.
        assert!(
            (fs.resources - 75.0).abs() < 1e-9,
            "expected 75.0 after deployment, got {}",
            fs.resources
        );
        assert_eq!(fs.tech_deployment_spend, 25.0);
        assert_eq!(fs.tech_deployed.len(), 1, "card deployed");
        assert!(fs.tech_denied_at_deployment.is_empty());
    }

    #[test]
    fn deployment_skipped_when_unaffordable_and_recorded_as_denied() {
        // Initial 30; card costs 50 -> denied. Resources untouched.
        let card = make_tech("expensive", 50.0, 0.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 30.0, 0.0, 1);
        let engine = Engine::with_seed(sc, 1).expect("engine init");
        let fs = engine
            .state()
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        assert!(
            (fs.resources - 30.0).abs() < 1e-9,
            "denial preserves resources, got {}",
            fs.resources
        );
        assert!(fs.tech_deployed.is_empty(), "denied card not deployed");
        assert_eq!(fs.tech_denied_at_deployment.len(), 1);
        assert_eq!(fs.tech_denied_at_deployment[0].0, "expensive");
        assert_eq!(fs.tech_deployment_spend, 0.0);
    }

    #[test]
    fn deployment_iteration_continues_past_denial() {
        // 40 budget. First card 30 (fits), second 50 (denied), third
        // 5 (fits in remaining 10). Iteration must not short-circuit
        // on the denial — the third card is the test.
        let cards = vec![
            make_tech("first", 30.0, 0.0, None, 1.0),
            make_tech("second", 50.0, 0.0, None, 1.0),
            make_tech("third", 5.0, 0.0, None, 1.0),
        ];
        let sc = scenario_with_techs(cards, 40.0, 0.0, 1);
        let engine = Engine::with_seed(sc, 1).expect("engine init");
        let fs = engine
            .state()
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        let deployed_ids: Vec<&str> = fs.tech_deployed.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(deployed_ids, vec!["first", "third"]);
        assert_eq!(fs.tech_denied_at_deployment.len(), 1);
        assert_eq!(fs.tech_denied_at_deployment[0].0, "second");
        // Deployment spend = 30 + 5 = 35; resources = 40 - 35 = 5.
        assert!((fs.resources - 5.0).abs() < 1e-9);
        assert!((fs.tech_deployment_spend - 35.0).abs() < 1e-9);
    }

    // cost_per_tick tests ----------------------------------------------------

    #[test]
    fn cost_per_tick_deducted_in_attrition_phase() {
        // 100 budget, 0 deployment, 5/tick maintenance, 0 income.
        // After 3 ticks: 100 - 3*5 = 85.
        let card = make_tech("steady", 0.0, 5.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 3);
        let mut engine = Engine::with_seed(sc, 1).expect("engine init");
        engine.run().expect("run");
        let fs = engine
            .state()
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        assert!(
            (fs.resources - 85.0).abs() < 1e-9,
            "expected 85.0 after 3 maintenance ticks, got {}",
            fs.resources
        );
        assert!((fs.tech_maintenance_spend - 15.0).abs() < 1e-9);
        assert!(fs.tech_decommissioned.is_empty(), "no decommission");
    }

    #[test]
    fn unaffordable_maintenance_decommissions_card_immediately() {
        // 10 budget, 0 deployment, 8/tick maintenance, 0 income.
        // Tick 1: pay 8, leaves 2. Tick 2: 8 > 2, decommission.
        // Tick 3: card gone, no further charges.
        let card = make_tech("burns_out", 0.0, 8.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 10.0, 0.0, 3);
        let mut engine = Engine::with_seed(sc, 1).expect("engine init");
        engine.run().expect("run");
        let fs = engine
            .state()
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        assert!(
            (fs.resources - 2.0).abs() < 1e-9,
            "expected 2.0 leftover, got {}",
            fs.resources
        );
        assert!(
            fs.tech_deployed.is_empty(),
            "card decommissioned, got {:?}",
            fs.tech_deployed
        );
        assert_eq!(fs.tech_decommissioned.len(), 1);
        assert_eq!(fs.tech_decommissioned[0].1.0, "burns_out");
        // Decommission tick is 2 (the tick the unaffordable check
        // fired).
        assert_eq!(fs.tech_decommissioned[0].0, 2);
        // Maintenance only paid once (tick 1).
        assert!((fs.tech_maintenance_spend - 8.0).abs() < 1e-9);
    }

    // coverage_limit tests ---------------------------------------------------

    /// Build a 2-faction scenario where alpha has a tech card with
    /// `coverage_limit = limit`. Both factions have units in two
    /// regions, producing two contested regions per tick → two
    /// (region, opponent) pairs that the alpha tech could touch each
    /// tick. Returns the engine after one combat tick so callers can
    /// inspect `tech_coverage_used`.
    fn coverage_scenario(limit: Option<u32>) -> Scenario {
        let mut sc = empty_scenario(7, 1);
        let r1 = RegionId::from("r1");
        let r2 = RegionId::from("r2");

        // Alpha forces in r1 and r2.
        let mut alpha_forces = BTreeMap::new();
        let mut a1 = make_force("a1", &r1, 50.0, 0.0);
        a1.upkeep = 0.0;
        let mut a2 = make_force("a2", &r2, 50.0, 0.0);
        a2.upkeep = 0.0;
        alpha_forces.insert(ForceId::from("a1"), a1);
        alpha_forces.insert(ForceId::from("a2"), a2);
        let mut alpha = make_faction("alpha", alpha_forces, 0.0, None);
        alpha.initial_resources = 1000.0;
        alpha.resource_rate = 100.0;
        let card = make_tech("coverage_test", 0.0, 0.0, limit, 1.5);
        alpha.tech_access = vec![card.id.clone()];
        sc.factions.insert(FactionId::from("alpha"), alpha);
        sc.technology.insert(card.id.clone(), card);

        // Bravo forces in r1 and r2 (so both regions become contested).
        let mut bravo_forces = BTreeMap::new();
        let mut b1 = make_force("b1", &r1, 50.0, 0.0);
        b1.upkeep = 0.0;
        let mut b2 = make_force("b2", &r2, 50.0, 0.0);
        b2.upkeep = 0.0;
        bravo_forces.insert(ForceId::from("b1"), b1);
        bravo_forces.insert(ForceId::from("b2"), b2);
        let bravo = make_faction("bravo", bravo_forces, 0.0, None);
        sc.factions.insert(FactionId::from("bravo"), bravo);

        sc
    }

    #[test]
    fn coverage_uncapped_default_applies_in_every_region() {
        let sc = coverage_scenario(None);
        let state = run_combat_only(sc);
        let fs = state
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        // Without a cap, the coverage counter is not even tracked
        // (we skip the gate entirely). The map should be empty.
        assert!(
            fs.tech_coverage_used.is_empty(),
            "no cap → no coverage tracking, got {:?}",
            fs.tech_coverage_used
        );
    }

    #[test]
    fn coverage_limit_one_caps_per_tick_applications() {
        let sc = coverage_scenario(Some(1));
        let state = run_combat_only(sc);
        let fs = state
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        // Two contested regions (r1, r2) → two (region, opponent)
        // pairs alpha is involved in. With limit = 1, only one of
        // those pairs gets the tech contribution; the second is
        // skipped. Counter ends at exactly 1.
        let used = fs
            .tech_coverage_used
            .get(&TechCardId::from("coverage_test"))
            .copied()
            .unwrap_or(0);
        assert_eq!(used, 1, "coverage_limit=1 caps at one application");
    }

    #[test]
    fn coverage_limit_above_demand_still_tracks_actual_usage() {
        // limit = 10 but only 2 (region, opponent) pairs are
        // available → counter ends at 2 (the actual demand).
        let sc = coverage_scenario(Some(10));
        let state = run_combat_only(sc);
        let fs = state
            .faction_states
            .get(&FactionId::from("alpha"))
            .expect("alpha");
        let used = fs
            .tech_coverage_used
            .get(&TechCardId::from("coverage_test"))
            .copied()
            .unwrap_or(0);
        assert_eq!(used, 2, "two contested regions = two applications");
    }

    // Determinism + report-emission ------------------------------------------

    #[test]
    fn tech_cost_pipeline_is_deterministic_across_same_seed_runs() {
        let cards = vec![
            make_tech("a", 10.0, 1.0, Some(2), 1.2),
            make_tech("b", 5.0, 0.5, None, 0.9),
        ];
        let sc = scenario_with_techs(cards, 100.0, 5.0, 5);
        let mut a = Engine::with_seed(sc.clone(), 99).expect("engine a");
        let mut b = Engine::with_seed(sc, 99).expect("engine b");
        let ra = a.run().expect("run a");
        let rb = b.run().expect("run b");
        assert_eq!(
            ra.tech_costs, rb.tech_costs,
            "tech_costs must be deterministic across same-seed runs"
        );
    }

    #[test]
    fn run_result_emits_tech_cost_report_when_cost_is_nonzero() {
        let card = make_tech("paying", 5.0, 1.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 2);
        let mut engine = Engine::with_seed(sc, 1).expect("engine");
        let run = engine.run().expect("run");
        let report = run
            .tech_costs
            .get(&FactionId::from("alpha"))
            .expect("tech_costs entry present");
        assert!((report.total_deployment_spend - 5.0).abs() < 1e-9);
        assert!((report.total_maintenance_spend - 2.0).abs() < 1e-9);
        assert_eq!(report.deployed_techs.len(), 1);
        assert!(report.denied_at_deployment.is_empty());
        assert!(report.decommissioned.is_empty());
    }

    #[test]
    fn run_result_omits_tech_cost_entry_for_zero_cost_roster() {
        // Card with all-zero costs and no coverage limit triggers
        // none of the cost-mechanic codepaths → no report row.
        let card = make_tech("free_lunch", 0.0, 0.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 2);
        let mut engine = Engine::with_seed(sc, 1).expect("engine");
        let run = engine.run().expect("run");
        assert!(
            !run.tech_costs.contains_key(&FactionId::from("alpha")),
            "zero-cost roster should not emit a tech_costs entry, got {:?}",
            run.tech_costs
        );
    }

    // Validation tests -------------------------------------------------------

    #[test]
    fn validate_rejects_negative_deployment_cost() {
        let card = make_tech("bad", -1.0, 0.0, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 1);
        let err = validate_scenario(&sc).expect_err("should reject negative deployment_cost");
        let msg = err.to_string();
        assert!(
            msg.contains("deployment_cost"),
            "diagnostic should name the field, got: {msg}"
        );
    }

    #[test]
    fn validate_rejects_nan_cost_per_tick() {
        let card = make_tech("bad", 0.0, f64::NAN, None, 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 1);
        let err = validate_scenario(&sc).expect_err("should reject NaN cost_per_tick");
        let msg = err.to_string();
        assert!(
            msg.contains("cost_per_tick"),
            "diagnostic should name the field, got: {msg}"
        );
    }

    #[test]
    fn validate_rejects_zero_coverage_limit() {
        // Some(0) is the silent-no-op shape — every application
        // would be skipped because `used >= limit` is true at first
        // attempt. Almost always an authoring error.
        let card = make_tech("bad", 0.0, 0.0, Some(0), 1.0);
        let sc = scenario_with_techs(vec![card], 100.0, 0.0, 1);
        let err = validate_scenario(&sc).expect_err("should reject coverage_limit = 0");
        let msg = err.to_string();
        assert!(
            msg.contains("coverage_limit") && msg.contains("silent no-op"),
            "diagnostic should name the field and explain the no-op, got: {msg}"
        );
    }
}
