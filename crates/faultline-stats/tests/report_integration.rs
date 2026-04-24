//! End-to-end integration tests for the report generator.
//!
//! Builds a minimal but non-trivial scenario, runs the Monte Carlo
//! pipeline, and asserts on the structure of the rendered Markdown
//! report. This catches wiring regressions the unit tests in
//! `report.rs` cannot — specifically that `MonteCarloRunner` populates
//! the new `win_rate_cis` / `FeasibilityRow.ci_95` fields, and that
//! `render_markdown` emits the new sections in response.

use std::collections::BTreeMap;

use faultline_stats::report::render_markdown;
use faultline_stats::{MonteCarloRunner, compute_summary};
use faultline_types::campaign::{
    BranchCondition, CampaignPhase, DefensiveDomain, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
};
use faultline_types::faction::{Faction, FactionType, ForceUnit, MilitaryBranch, UnitType};
use faultline_types::ids::{FactionId, ForceId, KillChainId, PhaseId, RegionId, VictoryId};
use faultline_types::map::{MapConfig, MapSource, Region, TerrainModifier, TerrainType};
use faultline_types::politics::{MediaLandscape, PoliticalClimate};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::stats::{ConfidenceLevel, MonteCarloConfig};
use faultline_types::strategy::Doctrine;
use faultline_types::victory::{VictoryCondition, VictoryType};

fn make_faction(id_str: &str, home: &RegionId) -> Faction {
    let fid = FactionId::from(id_str);
    let force_id = ForceId::from(format!("{id_str}-inf"));
    let mut forces = BTreeMap::new();
    forces.insert(
        force_id.clone(),
        ForceUnit {
            id: force_id,
            name: format!("{id_str} Infantry"),
            unit_type: UnitType::Infantry,
            region: home.clone(),
            strength: 100.0,
            mobility: 1.0,
            force_projection: None,
            upkeep: 1.0,
            morale_modifier: 0.0,
            capabilities: vec![],
        },
    );
    Faction {
        id: fid,
        name: id_str.to_string(),
        faction_type: FactionType::Military {
            branch: MilitaryBranch::Army,
        },
        description: String::new(),
        color: "#888888".into(),
        forces,
        tech_access: vec![],
        initial_morale: 0.8,
        logistics_capacity: 100.0,
        initial_resources: 1000.0,
        resource_rate: 10.0,
        recruitment: None,
        command_resilience: 0.5,
        intelligence: 0.5,
        diplomacy: vec![],
        doctrine: Doctrine::Conventional,
    }
}

/// Build a scenario with one kill chain that carries a low-confidence
/// tag, so the report has every section to exercise.
fn flagged_chain_scenario() -> Scenario {
    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");
    let red = FactionId::from("red");
    let blue = FactionId::from("blue");

    let mut regions = BTreeMap::new();
    regions.insert(
        r1.clone(),
        Region {
            id: r1.clone(),
            name: "R1".into(),
            population: 1,
            urbanization: 0.0,
            initial_control: Some(red.clone()),
            strategic_value: 1.0,
            borders: vec![r2.clone()],
            centroid: None,
        },
    );
    regions.insert(
        r2.clone(),
        Region {
            id: r2.clone(),
            name: "R2".into(),
            population: 1,
            urbanization: 0.0,
            initial_control: Some(blue.clone()),
            strategic_value: 1.0,
            borders: vec![r1.clone()],
            centroid: None,
        },
    );

    let mut factions = BTreeMap::new();
    factions.insert(red.clone(), make_faction("red", &r1));
    factions.insert(blue.clone(), make_faction("blue", &r2));

    let chain_id = KillChainId::from("alpha");
    let recon = PhaseId::from("recon");
    let strike = PhaseId::from("strike");
    let mut phases = BTreeMap::new();
    phases.insert(
        recon.clone(),
        CampaignPhase {
            id: recon.clone(),
            name: "Recon".into(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 0.8,
            min_duration: 2,
            max_duration: 2,
            detection_probability_per_tick: 0.05,
            prerequisite_success_boost: 0.1,
            attribution_difficulty: 0.7,
            cost: PhaseCost {
                attacker_dollars: 500.0,
                defender_dollars: 50_000.0,
                attacker_resources: 0.0,
                // Scenario author flags the cost as low-confidence.
                confidence: Some(ConfidenceLevel::Low),
            },
            targets_domains: vec![
                DefensiveDomain::SignalsIntelligence,
                DefensiveDomain::PhysicalSecurity,
            ],
            outputs: vec![PhaseOutput::TensionDelta { delta: 0.05 }],
            branches: vec![PhaseBranch {
                condition: BranchCondition::OnSuccess,
                next_phase: strike.clone(),
            }],
            parameter_confidence: Some(ConfidenceLevel::Low),
        },
    );
    phases.insert(
        strike.clone(),
        CampaignPhase {
            id: strike.clone(),
            name: "Strike".into(),
            description: String::new(),
            prerequisites: vec![recon.clone()],
            base_success_probability: 0.6,
            min_duration: 1,
            max_duration: 2,
            detection_probability_per_tick: 0.1,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.4,
            cost: PhaseCost {
                attacker_dollars: 1_000.0,
                defender_dollars: 200_000.0,
                attacker_resources: 0.0,
                confidence: None,
            },
            targets_domains: vec![DefensiveDomain::PhysicalSecurity],
            outputs: vec![PhaseOutput::TensionDelta { delta: 0.1 }],
            branches: vec![],
            parameter_confidence: None,
        },
    );

    let mut kill_chains = BTreeMap::new();
    kill_chains.insert(
        chain_id.clone(),
        KillChain {
            id: chain_id,
            name: "Alpha Campaign".into(),
            description: String::new(),
            attacker: red.clone(),
            target: blue.clone(),
            entry_phase: recon,
            phases,
        },
    );

    let mut victory_conditions = BTreeMap::new();
    let vc_id = VictoryId::from("red-wins");
    victory_conditions.insert(
        vc_id.clone(),
        VictoryCondition {
            id: vc_id,
            name: "Red Wins".into(),
            faction: red,
            condition: VictoryType::MilitaryDominance {
                enemy_strength_below: 0.01,
            },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Report Integration".into(),
            description: "Flagged chain for end-to-end report validation".into(),
            author: "test".into(),
            version: "0.0.1".into(),
            tags: vec![],
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
                    terrain_type: TerrainType::Urban,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 0.8,
                },
                TerrainModifier {
                    region: r2,
                    terrain_type: TerrainType::Rural,
                    movement_modifier: 1.0,
                    defense_modifier: 1.0,
                    visibility: 0.9,
                },
            ],
        },
        factions,
        technology: BTreeMap::new(),
        political_climate: PoliticalClimate {
            tension: 0.2,
            institutional_trust: 0.6,
            media_landscape: MediaLandscape {
                fragmentation: 0.5,
                disinformation_susceptibility: 0.3,
                state_control: 0.4,
                social_media_penetration: 0.7,
                internet_availability: 0.8,
            },
            population_segments: vec![],
            global_modifiers: vec![],
        },
        events: BTreeMap::new(),
        simulation: SimulationConfig {
            max_ticks: 30,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 50,
            seed: Some(42),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
        },
        victory_conditions,
        kill_chains,
        defender_budget: None,
        attacker_budget: None,
    }
}

#[test]
fn report_populates_win_rate_and_feasibility_cis() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC should succeed");

    // Invariant: every faction that appears in `win_rates` must also
    // appear in `win_rate_cis` with matching point estimate. The test
    // scenario is not guaranteed to produce a victor (it's a short
    // kill-chain-focused run), so we don't assert on non-emptiness.
    for (fid, rate) in &result.summary.win_rates {
        let ci = result
            .summary
            .win_rate_cis
            .get(fid)
            .unwrap_or_else(|| panic!("missing Wilson CI for faction {fid}"));
        assert!(
            (ci.point - rate).abs() < 1e-9,
            "CI point {} should equal win rate {rate} for {fid}",
            ci.point
        );
        assert_eq!(ci.n, result.summary.total_runs);
        assert!(ci.lower <= rate + 1e-9 && *rate <= ci.upper + 1e-9);
    }

    // Feasibility matrix should exist with at least one row and
    // populated Wilson bounds on the rate-valued fields.
    assert!(
        !result.summary.feasibility_matrix.is_empty(),
        "feasibility matrix should not be empty"
    );
    let row = &result.summary.feasibility_matrix[0];
    let det = row
        .ci_95
        .detection_probability
        .as_ref()
        .expect("Wilson CI present on detection");
    assert!(det.lower <= det.point && det.point <= det.upper);
    assert_eq!(det.n, 50);
    let succ = row
        .ci_95
        .success_probability
        .as_ref()
        .expect("Wilson CI present on success");
    assert!(succ.lower <= succ.upper);
    assert!(succ.upper <= 1.0 && succ.lower >= 0.0);

    // The summary produced inside `MonteCarloRunner::run` should match
    // a fresh call to `compute_summary` on the same runs — catches
    // divergence between the two code paths.
    let recomputed = compute_summary(&result.runs, &scenario);
    assert_eq!(
        recomputed.total_runs, result.summary.total_runs,
        "compute_summary must agree with the runner"
    );
}

#[test]
fn rendered_report_contains_new_sections() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC should succeed");
    let md = render_markdown(&result.summary, &scenario);

    // Methodology is always present.
    assert!(
        md.contains("## Methodology & Confidence"),
        "methodology section missing"
    );
    assert!(
        md.contains("Wilson score interval"),
        "methodology must name Wilson; got:\n{md}"
    );

    // Author-flagged section must appear because the scenario tags
    // `recon.parameter_confidence = Low` and `recon.cost.confidence`
    // — but only the Low-tagged ones (High on cost doesn't flag).
    // NB: the test scenario sets cost = Low on recon and phase = Low,
    // so both bits should surface.
    assert!(
        md.contains("Author-Flagged Low-Confidence Parameters"),
        "flagged section missing; got:\n{md}"
    );
    assert!(
        md.contains("Alpha Campaign"),
        "flagged section should reference chain name"
    );
    assert!(
        md.contains("phase parameters") && md.contains("phase cost"),
        "flagged section should describe both flag kinds; got:\n{md}"
    );

    // Feasibility matrix cell format should include a Wilson range.
    // Look for a `[` + `]` + `(` sequence indicating `value [X] (lo–hi)`
    // somewhere in the feasibility-matrix region of the doc.
    let matrix_section = md
        .split("## Feasibility Matrix")
        .nth(1)
        .expect("feasibility matrix section should exist");
    assert!(
        matrix_section.contains('[') && matrix_section.contains("](")
            || (matrix_section.contains(" (") && matrix_section.contains("–")),
        "feasibility cells should format Wilson bounds like '(lo–hi)'; got:\n{matrix_section}"
    );
}

#[test]
fn report_is_deterministic_across_runs_with_same_seed() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 20,
        seed: Some(99),
        collect_snapshots: false,
        parallel: false,
    };
    let a = MonteCarloRunner::run(&config, &scenario).expect("run a");
    let b = MonteCarloRunner::run(&config, &scenario).expect("run b");
    let md_a = render_markdown(&a.summary, &scenario);
    let md_b = render_markdown(&b.summary, &scenario);
    assert_eq!(
        md_a, md_b,
        "report rendering must be deterministic under identical seed"
    );
}

#[test]
fn report_omits_flagged_section_when_scenario_has_no_flags() {
    let mut scenario = flagged_chain_scenario();
    // Strip flags from every phase.
    for chain in scenario.kill_chains.values_mut() {
        for phase in chain.phases.values_mut() {
            phase.parameter_confidence = None;
            phase.cost.confidence = None;
        }
    }
    let config = MonteCarloConfig {
        num_runs: 20,
        seed: Some(7),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);
    assert!(
        !md.contains("Author-Flagged Low-Confidence Parameters"),
        "section should be elided when nothing is flagged"
    );
    // Methodology still renders.
    assert!(md.contains("## Methodology & Confidence"));
}
