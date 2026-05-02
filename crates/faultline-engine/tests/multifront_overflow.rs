//! Multi-front resource contention (Epic D round-three item 3) tests.
//!
//! Pins the cross-role `overflow_to` / `overflow_threshold` mechanic
//! end-to-end — both the engine spillover semantics and the
//! load-time validation rejections. The end-to-end test is structured
//! as an A/B comparison: same scenario, same seed, with and without
//! `overflow_to`. Differences in queue utilisation are therefore
//! attributable to the spillover wiring alone.

use std::collections::BTreeMap;

use faultline_engine::{Engine, validate_scenario};
use faultline_types::campaign::{
    BranchCondition, CampaignPhase, DefenderNoise, KillChain, PhaseBranch, PhaseCost,
};
use faultline_types::faction::{
    DefenderCapacity, Faction, FactionType, ForceUnit, MilitaryBranch, OverflowPolicy, UnitType,
};
use faultline_types::ids::{DefenderRoleId, FactionId, ForceId, KillChainId, PhaseId, RegionId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::strategy::Doctrine;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn region(id: &str) -> Region {
    Region {
        id: RegionId::from(id),
        name: id.into(),
        population: 1_000,
        urbanization: 0.5,
        initial_control: Some(FactionId::from("blue")),
        strategic_value: 1.0,
        borders: vec![],
        centroid: None,
    }
}

fn force(id: &str, region: &str, faction: &str, strength: f64) -> ForceUnit {
    let _ = faction;
    ForceUnit {
        id: ForceId::from(id),
        name: id.into(),
        unit_type: UnitType::Infantry,
        region: RegionId::from(region),
        strength,
        mobility: 1.0,
        force_projection: None,
        upkeep: 0.0,
        morale_modifier: 0.0,
        capabilities: vec![],
        move_progress: 0.0,
    }
}

fn capacity(
    id: &str,
    queue_depth: u32,
    service_rate: f64,
    overflow_to: Option<&str>,
    overflow_threshold: Option<f64>,
) -> DefenderCapacity {
    DefenderCapacity {
        id: DefenderRoleId::from(id),
        name: id.into(),
        description: String::new(),
        queue_depth,
        service_rate,
        overflow: OverflowPolicy::Backlog,
        saturated_detection_factor: 0.5,
        overflow_to: overflow_to.map(DefenderRoleId::from),
        overflow_threshold,
    }
}

fn faction(id: &str, capacities: Vec<DefenderCapacity>) -> Faction {
    let mut forces = BTreeMap::new();
    forces.insert(ForceId::from(id), force(id, "r1", id, 50.0));
    let mut caps = BTreeMap::new();
    for cap in capacities {
        caps.insert(cap.id.clone(), cap);
    }
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
        diplomacy: vec![],
        doctrine: Doctrine::Defensive,
        escalation_rules: None,
        defender_capacities: caps,
        leadership: None,
        alliance_fracture: None,
    }
}

/// One-phase chain that pumps a deterministic mean of `noise` items
/// per tick into `(faction, role)`. Mean is high enough that even with
/// Poisson variance the queue will saturate quickly.
fn one_phase_chain(faction: &str, role: &str, noise: f64) -> KillChain {
    let phase_id = PhaseId::from("noise_phase");
    let mut phases = BTreeMap::new();
    phases.insert(
        phase_id.clone(),
        CampaignPhase {
            id: phase_id.clone(),
            name: "noise".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 0.0,
            min_duration: 100,
            max_duration: 100,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost {
                attacker_dollars: 0.0,
                defender_dollars: 0.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            outputs: vec![],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: phase_id.clone(),
            }],
            defender_noise: vec![DefenderNoise {
                defender: FactionId::from(faction),
                role: DefenderRoleId::from(role),
                items_per_tick: noise,
            }],
            gated_by_defender: None,
            targets_domains: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
        },
    );
    KillChain {
        id: KillChainId::from("noise_chain"),
        name: "noise_chain".into(),
        description: String::new(),
        attacker: FactionId::from("red"),
        target: FactionId::from(faction),
        entry_phase: phase_id,
        phases,
    }
}

fn base_scenario(seed: u64, max_ticks: u32, defender: Faction, chain: KillChain) -> Scenario {
    let r1 = RegionId::from("r1");
    let mut regions = BTreeMap::new();
    regions.insert(r1.clone(), region("r1"));

    let mut factions = BTreeMap::new();
    let red = faction("red", vec![]);
    factions.insert(FactionId::from("red"), red);
    factions.insert(defender.id.clone(), defender);

    let mut chains = BTreeMap::new();
    chains.insert(chain.id.clone(), chain);

    Scenario {
        meta: ScenarioMeta {
            name: "multifront_test".into(),
            description: String::new(),
            author: "test".into(),
            version: "0.1.0".into(),
            tags: vec![],
            confidence: None,
            schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            historical_analogue: None,
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
            snapshot_interval: 10,
        },
        victory_conditions: BTreeMap::new(),
        kill_chains: chains,
        defender_budget: None,
        attacker_budget: None,
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        networks: BTreeMap::new(),
    }
}

/// Run a scenario and return the per-(faction, role) queue reports
/// from the single run.
fn run_and_collect_queue_reports(sc: Scenario) -> Vec<faultline_types::stats::DefenderQueueReport> {
    let mut engine = Engine::with_seed(sc, 42).expect("engine init");
    let result = engine.run().expect("run");
    result.defender_queue_reports
}

// ---------------------------------------------------------------------------
// Engine spillover semantics
// ---------------------------------------------------------------------------

#[test]
fn no_overflow_to_preserves_legacy_single_queue_behavior() {
    // Without overflow_to, a saturated queue accumulates depth past
    // capacity (Backlog policy) and reports zero spillover. This is
    // the Epic K shape; pinning it ensures the new code path is opt-in
    // and the legacy code path is unchanged.
    let blue = faction("blue", vec![capacity("solo", 10, 1.0, None, None)]);
    let chain = one_phase_chain("blue", "solo", 30.0);
    let sc = base_scenario(7, 20, blue, chain);

    let reports = run_and_collect_queue_reports(sc);
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!(r.role.0, "solo");
    assert!(
        r.total_enqueued > 0,
        "phase noise should have enqueued work"
    );
    assert_eq!(r.spillover_in, 0, "no overflow_to => no inbound spillover");
    assert_eq!(
        r.spillover_out, 0,
        "no overflow_to => no outbound spillover"
    );
    assert!(
        r.max_depth > r.capacity,
        "Backlog policy should grow past capacity ({} vs {})",
        r.max_depth,
        r.capacity,
    );
}

#[test]
fn overflow_routes_excess_to_target_role() {
    // tier1 -> tier2 chain. tier1 declares overflow_to=tier2 with the
    // default 1.0 threshold, so spillover engages only at full
    // saturation. Validates that:
    //   (a) tier1.spillover_out > 0 once tier1 saturates,
    //   (b) tier1.spillover_out exactly equals tier2.spillover_in
    //       (the conservation invariant),
    //   (c) tier2.total_enqueued is non-zero despite the chain only
    //       declaring direct noise on tier1.
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 10, 1.0, Some("tier2"), None),
            capacity("tier2", 50, 0.5, None, None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 50.0);
    let sc = base_scenario(7, 20, blue, chain);

    let reports = run_and_collect_queue_reports(sc);
    let r1 = reports.iter().find(|r| r.role.0 == "tier1").expect("tier1");
    let r2 = reports.iter().find(|r| r.role.0 == "tier2").expect("tier2");

    assert!(
        r1.spillover_out > 0,
        "tier1 should have spilled excess to tier2; got {}",
        r1.spillover_out
    );
    assert_eq!(
        r1.spillover_out, r2.spillover_in,
        "chain conservation: tier1 spillover_out ({}) must equal tier2 spillover_in ({})",
        r1.spillover_out, r2.spillover_in,
    );
    assert!(
        r2.total_enqueued >= r2.spillover_in,
        "tier2 total_enqueued ({}) must include spillover_in ({})",
        r2.total_enqueued,
        r2.spillover_in,
    );
    assert_eq!(
        r2.spillover_out, 0,
        "tier2 has no overflow_to, so spillover_out must be 0"
    );

    // total_enqueued on the spilling role must NOT include the
    // spillover_out portion — those items never entered tier1's
    // queue. The doc on `spillover_out` pins this contract; pin it
    // here too so a future regression that re-charges spillover to
    // total_enqueued (the bug fixed in the PR feedback round) is
    // caught by the suite. With Backlog policy and no drops the
    // queue conservation is `total_enqueued == depth + total_serviced`,
    // so `total_enqueued <= max_depth + total_serviced` is the
    // upper-bound check.
    assert!(
        r1.total_enqueued <= u64::from(r1.max_depth) + r1.total_serviced,
        "tier1 total_enqueued ({}) cannot exceed max_depth ({}) + total_serviced ({}) — that \
         would imply spillover_out items were charged to total_enqueued",
        r1.total_enqueued,
        r1.max_depth,
        r1.total_serviced,
    );
    assert!(
        r1.total_enqueued < r1.spillover_out,
        "with high noise relative to capacity tier1 should spill far more than it enqueues; \
         got total_enqueued={} vs spillover_out={}",
        r1.total_enqueued,
        r1.spillover_out,
    );
}

#[test]
fn three_tier_chain_propagates_spillover_end_to_end() {
    // tier1 -> tier2 -> tier3. Confirms recursion (the spillover from
    // tier1 keeps cascading once tier2 itself saturates) and that the
    // terminal node (tier3, no overflow_to) absorbs residual without
    // further spillover.
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 5, 0.5, Some("tier2"), Some(0.8)),
            capacity("tier2", 5, 0.5, Some("tier3"), Some(0.8)),
            capacity("tier3", 50, 0.5, None, None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 30.0);
    let sc = base_scenario(7, 30, blue, chain);

    let reports = run_and_collect_queue_reports(sc);
    let r1 = reports.iter().find(|r| r.role.0 == "tier1").expect("tier1");
    let r2 = reports.iter().find(|r| r.role.0 == "tier2").expect("tier2");
    let r3 = reports.iter().find(|r| r.role.0 == "tier3").expect("tier3");

    assert!(r1.spillover_out > 0, "tier1 must spill to tier2");
    assert!(r2.spillover_out > 0, "tier2 must spill to tier3");
    assert_eq!(r3.spillover_out, 0, "tier3 is terminal");
    assert_eq!(r1.spillover_out, r2.spillover_in);
    assert_eq!(r2.spillover_out, r3.spillover_in);
}

#[test]
fn overflow_threshold_governs_spillover_engagement_depth() {
    // tier1 capacity = 20, threshold = 0.25 -> spill above depth 5.
    // tier2 capacity = 100 -> deep enough to absorb everything.
    // Compare against threshold = 1.0 (full saturation) baseline:
    // a lower threshold should produce strictly more spillover.
    let chain = one_phase_chain("blue", "tier1", 40.0);

    let blue_low = faction(
        "blue",
        vec![
            capacity("tier1", 20, 1.0, Some("tier2"), Some(0.25)),
            capacity("tier2", 100, 0.5, None, None),
        ],
    );
    let sc_low = base_scenario(7, 20, blue_low, chain.clone());
    let reports_low = run_and_collect_queue_reports(sc_low);
    let low = reports_low
        .iter()
        .find(|r| r.role.0 == "tier1")
        .expect("tier1");

    let blue_high = faction(
        "blue",
        vec![
            capacity("tier1", 20, 1.0, Some("tier2"), Some(1.0)),
            capacity("tier2", 100, 0.5, None, None),
        ],
    );
    let sc_high = base_scenario(7, 20, blue_high, chain);
    let reports_high = run_and_collect_queue_reports(sc_high);
    let high = reports_high
        .iter()
        .find(|r| r.role.0 == "tier1")
        .expect("tier1");

    assert!(
        low.spillover_out > high.spillover_out,
        "lower threshold (0.25) should yield more spillover than 1.0 baseline; got low={} vs high={}",
        low.spillover_out,
        high.spillover_out,
    );
    assert!(
        low.max_depth <= 6,
        "tier1 max_depth must stay near the spillover threshold (5); got {}",
        low.max_depth,
    );
}

#[test]
fn same_seed_produces_identical_spillover_counts() {
    // Determinism contract: spillover counters are fully derived from
    // (scenario, seed). Two engines built from the same inputs must
    // produce bit-identical reports — the determinism property
    // `verify-bundled` relies on.
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 10, 0.5, Some("tier2"), Some(0.5)),
            capacity("tier2", 50, 0.5, None, None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 25.0);
    let sc1 = base_scenario(13, 20, blue.clone(), chain.clone());
    let sc2 = base_scenario(13, 20, blue, chain);

    let r1 = run_and_collect_queue_reports(sc1);
    let r2 = run_and_collect_queue_reports(sc2);

    assert_eq!(
        serde_json::to_string(&r1).expect("serialize r1"),
        serde_json::to_string(&r2).expect("serialize r2"),
        "same (scenario, seed) must produce bit-identical queue reports"
    );
}

// ---------------------------------------------------------------------------
// Validation rejections (silent-no-op shapes that must fail loud)
// ---------------------------------------------------------------------------

#[test]
fn validation_rejects_unknown_overflow_to_role() {
    let blue = faction(
        "blue",
        vec![capacity("tier1", 10, 1.0, Some("nonexistent"), None)],
    );
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("unknown overflow target must reject");
    assert!(
        format!("{err}").contains("nonexistent"),
        "error should name the unknown role: {err}"
    );
}

#[test]
fn validation_rejects_self_overflow() {
    let blue = faction(
        "blue",
        vec![capacity("tier1", 10, 1.0, Some("tier1"), None)],
    );
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("self-overflow must reject");
    assert!(
        format!("{err}").contains("itself"),
        "error should explain the self-loop: {err}"
    );
}

#[test]
fn validation_rejects_overflow_cycle() {
    // tier1 -> tier2 -> tier1: a 2-role cycle. Walking forward from
    // either entry must catch the revisit.
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 10, 1.0, Some("tier2"), None),
            capacity("tier2", 10, 1.0, Some("tier1"), None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("cycle must reject");
    assert!(
        format!("{err}").contains("cycles"),
        "error should mention the cycle: {err}"
    );
}

#[test]
fn validation_rejects_threshold_outside_unit_interval() {
    // 1.5 is outside [0, 1] — would push spillover only on overflow,
    // which is what OverflowPolicy already covers and is therefore
    // an authoring mistake.
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 10, 1.0, Some("tier2"), Some(1.5)),
            capacity("tier2", 10, 1.0, None, None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("threshold > 1 must reject");
    assert!(
        format!("{err}").contains("overflow_threshold"),
        "error should mention threshold: {err}"
    );
}

#[test]
fn validation_rejects_threshold_without_overflow_to() {
    // A threshold without a target is meaningless — the spillover
    // path is gated on `overflow_to.is_some()`. An author who set
    // `overflow_threshold = 0.5` without a target almost certainly
    // forgot to fill in the target.
    let blue = faction("blue", vec![capacity("tier1", 10, 1.0, None, Some(0.5))]);
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("orphan threshold must reject");
    let msg = format!("{err}");
    assert!(
        msg.contains("overflow_threshold") && msg.contains("overflow_to"),
        "error should mention both fields: {err}"
    );
}

#[test]
fn validation_rejects_nan_threshold() {
    let blue = faction(
        "blue",
        vec![
            capacity("tier1", 10, 1.0, Some("tier2"), Some(f64::NAN)),
            capacity("tier2", 10, 1.0, None, None),
        ],
    );
    let chain = one_phase_chain("blue", "tier1", 1.0);
    let sc = base_scenario(7, 5, blue, chain);
    let err = validate_scenario(&sc).expect_err("NaN threshold must reject");
    assert!(
        format!("{err}").contains("overflow_threshold"),
        "error should mention threshold: {err}"
    );
}
