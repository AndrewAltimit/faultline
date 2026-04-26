//! End-to-end integration tests for the report generator.
//!
//! Builds a minimal but non-trivial scenario, runs the Monte Carlo
//! pipeline, and asserts on the structure of the rendered Markdown
//! report. This catches wiring regressions the unit tests in
//! `report.rs` cannot — specifically that `MonteCarloRunner` populates
//! the new `win_rate_cis` / `FeasibilityRow.ci_95` fields, and that
//! `render_markdown` emits the new sections in response.

use std::collections::BTreeMap;

use faultline_stats::counterfactual::{ParamOverride, run_compare, run_counterfactual};
use faultline_stats::report::{render_comparison_markdown, render_markdown};
use faultline_stats::{MonteCarloRunner, compute_summary};
use faultline_types::campaign::{
    BranchCondition, CampaignPhase, DefensiveDomain, KillChain, ObservableDiscipline, PhaseBranch,
    PhaseCost, PhaseOutput, WarningIndicator,
};
use faultline_types::events::{DefenderOption, EventDefinition, EventEffect};
use faultline_types::faction::{
    EscalationRules, EscalationRung, Faction, FactionType, ForceUnit, MilitaryBranch, UnitType,
};
use faultline_types::ids::{
    EventId, FactionId, ForceId, KillChainId, PhaseId, RegionId, VictoryId,
};
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
        escalation_rules: None,
        defender_capacities: BTreeMap::new(),
        leadership: None,
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
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
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
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
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
        environment: faultline_types::map::EnvironmentSchedule::default(),
        strategy_space: faultline_types::strategy_space::StrategySpace::default(),
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
    // Cells render as `value [X] (lo–hi)`, so the exact pattern `"] ("`
    // only appears in feasibility cells with CIs — not in any other
    // section of the document.
    let matrix_section = md
        .split("## Feasibility Matrix")
        .nth(1)
        .expect("feasibility matrix section should exist");
    assert!(
        matrix_section.contains("] (") && matrix_section.contains("–"),
        "feasibility cells should format Wilson bounds like '[X] (lo–hi)'; got:\n{matrix_section}"
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

#[test]
fn phase_stats_carry_wilson_cis() {
    // Every phase rate must have a matching CI when runs > 0, and the
    // Wilson invariant `lower <= point <= upper` must hold for all four
    // rates — the regression this guards against is the floating-point
    // drift that slipped `lower` above zero at `p_hat = 0`.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");

    let chain_summary = result
        .summary
        .campaign_summaries
        .values()
        .next()
        .expect("should have at least one campaign summary");
    assert!(!chain_summary.phase_stats.is_empty());

    for (pid, ps) in &chain_summary.phase_stats {
        let cis = ps
            .ci_95
            .as_ref()
            .unwrap_or_else(|| panic!("phase {pid} missing CIs at n=50"));
        for (label, rate, ci) in [
            ("success", ps.success_rate, &cis.success_rate),
            ("failure", ps.failure_rate, &cis.failure_rate),
            ("detection", ps.detection_rate, &cis.detection_rate),
            ("not_reached", ps.not_reached_rate, &cis.not_reached_rate),
        ] {
            assert_eq!(ci.n, 50, "phase {pid} {label} CI n mismatch");
            assert!(
                ci.lower <= rate + 1e-9 && rate <= ci.upper + 1e-9,
                "phase {pid} {label}: point {rate} outside CI [{}, {}]",
                ci.lower,
                ci.upper
            );
            assert!(
                (0.0..=1.0).contains(&ci.lower) && (0.0..=1.0).contains(&ci.upper),
                "phase {pid} {label}: CI bounds must stay in [0, 1]"
            );
        }
    }
}

#[test]
fn rendered_phase_breakdown_shows_wilson_bounds() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    // The phase-breakdown section should carry Wilson bounds inline in
    // each rate cell. `"% ("` is a unique-enough fragment: it only
    // appears when a rate cell is printed as `X.X% (lo–hi)`.
    let phase_section = md
        .split("## Kill Chain Phase Breakdown")
        .nth(1)
        .expect("phase breakdown section should exist");
    assert!(
        phase_section.contains("% ("),
        "phase breakdown cells should render `XX.X% (lo–hi)`; got:\n{phase_section}"
    );
}

#[test]
fn rendered_report_includes_continuous_metrics_with_bootstrap() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 50,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("## Continuous Metrics"),
        "continuous metrics section missing; got:\n{md}"
    );
    assert!(
        md.contains("95% bootstrap CI"),
        "continuous metrics header should name bootstrap CIs; got:\n{md}"
    );
    assert!(
        md.contains("Duration (ticks)"),
        "duration metric should render with friendly label; got:\n{md}"
    );

    // Every DistributionStats emitted by the runner must carry a
    // bootstrap CI — the table claims one in its header.
    for (metric, stats) in &result.summary.metric_distributions {
        assert!(
            stats.bootstrap_ci_mean.is_some(),
            "{metric:?} should have a bootstrap CI after runner pipeline"
        );
        let ci = stats.bootstrap_ci_mean.expect("just checked some");
        assert!(
            ci.lower <= ci.upper,
            "{metric:?}: bootstrap CI inverted: lower={} upper={}",
            ci.lower,
            ci.upper
        );
    }
}

#[test]
fn report_renders_meta_confidence_banner() {
    let mut scenario = flagged_chain_scenario();
    scenario.meta.confidence = Some(ConfidenceLevel::Medium);
    let config = MonteCarloConfig {
        num_runs: 10,
        seed: Some(123),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("Scenario confidence:"),
        "banner should appear when meta.confidence is set; got:\n{md}"
    );
    assert!(
        md.contains("Medium"),
        "banner should name the confidence level word; got:\n{md}"
    );
    assert!(
        md.contains("working draft"),
        "banner should include the Medium interpretation phrase; got:\n{md}"
    );
}

#[test]
fn report_omits_meta_confidence_banner_when_unset() {
    let scenario = flagged_chain_scenario();
    assert!(scenario.meta.confidence.is_none());
    let config = MonteCarloConfig {
        num_runs: 5,
        seed: Some(1),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);
    assert!(
        !md.contains("Scenario confidence:"),
        "banner must be elided when meta.confidence is None; got:\n{md}"
    );
}

#[test]
fn continuous_metrics_header_omits_ci_label_when_missing() {
    // Simulates a legacy `MonteCarloSummary` deserialized from a pre-bootstrap
    // build: `bootstrap_ci_mean` defaults to `None` for every metric. The
    // renderer must degrade the table header to plain "Mean" so the header
    // does not mislabel the (bare-mean) cells.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 10,
        seed: Some(7),
        collect_snapshots: false,
        parallel: false,
    };
    let mut result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    for stats in result.summary.metric_distributions.values_mut() {
        stats.bootstrap_ci_mean = None;
    }
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("| Metric | Mean | Median |"),
        "header should fall back to plain 'Mean' when no metric carries a bootstrap CI; got:\n{md}"
    );
    assert!(
        !md.contains("95% bootstrap CI"),
        "CI label must disappear (header + footnote) when no metric carries a CI; got:\n{md}"
    );
}

#[test]
fn continuous_metrics_partial_ci_suppresses_bounds_in_cells() {
    // Simulates a partially-populated `MonteCarloSummary`: some metrics carry
    // a `bootstrap_ci_mean`, others do not (e.g. manual construction, partial
    // migration). Header must degrade to plain "Mean" and, crucially, cells
    // that *do* carry a CI must also drop their `(lo – hi)` suffix — otherwise
    // a plain-"Mean"-header row would still display CI syntax in some cells.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 10,
        seed: Some(11),
        collect_snapshots: false,
        parallel: false,
    };
    let mut result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    // Clear the bootstrap CI on only the first metric; leave the rest populated.
    let first_key = result
        .summary
        .metric_distributions
        .keys()
        .next()
        .expect("at least one metric distribution")
        .clone();
    result
        .summary
        .metric_distributions
        .get_mut(&first_key)
        .expect("first metric stats")
        .bootstrap_ci_mean = None;

    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("| Metric | Mean | Median |"),
        "header should fall back to plain 'Mean' in the partial-CI case; got:\n{md}"
    );
    assert!(
        !md.contains("95% bootstrap CI"),
        "CI label must disappear when not every metric carries a CI; got:\n{md}"
    );
    // The continuous-metrics rows live in the `## Continuous Metrics` section.
    // Slice that section and verify the Mean cell (second column) never
    // contains the `(lo – hi)` CI suffix, even for metrics that retained a
    // populated `bootstrap_ci_mean`. Other columns like `5th – 95th pct`
    // legitimately contain ` – `, so we only inspect column index 1.
    let section_start = md
        .find("## Continuous Metrics")
        .expect("continuous metrics section present");
    let rest = &md[section_start..];
    let section_end = rest[1..].find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
    let section = &rest[..section_end];
    let mut data_rows_checked = 0;
    for line in section.lines() {
        // Skip the header row, the `|---|...|` separator, and non-table lines.
        if !line.starts_with("| ") || line.contains("Metric |") || line.contains("---") {
            continue;
        }
        // Columns are `| metric | mean | median | p5 – p95 | std_dev |`.
        // Splitting on `|` produces an empty leading element, so the mean
        // cell lives at index 2.
        let cells: Vec<&str> = line.split('|').collect();
        let mean_cell = cells.get(2).expect("mean column present").trim();
        assert!(
            !mean_cell.contains('('),
            "mean cell must not carry CI syntax when header is plain 'Mean'; got mean cell {mean_cell:?} in row:\n{line}"
        );
        data_rows_checked += 1;
    }
    assert!(
        data_rows_checked > 0,
        "expected at least one continuous-metrics data row to inspect; section was:\n{section}"
    );
}

// ============================================================================
// Epic B — Policy Implications, Countermeasure Analysis, comparison report
//
// These tests are deliberately driven through the public `render_markdown`
// surface and the public `run_counterfactual` / `run_compare` entry points
// rather than the section-level helpers, so they catch wiring breaks the
// per-section unit tests would miss.
// ============================================================================

/// Augment the existing flagged-chain scenario with one IWI-tagged
/// phase, one defender_options-bearing event, and an escalation_rules
/// block on the attacker. Produces a scenario where every Epic B
/// section has *some* content.
fn epic_b_populated_scenario() -> Scenario {
    let mut scenario = flagged_chain_scenario();

    // Tag every phase with at least one warning indicator.
    for chain in scenario.kill_chains.values_mut() {
        for (idx, (_pid, phase)) in chain.phases.iter_mut().enumerate() {
            phase.warning_indicators.push(WarningIndicator {
                id: format!("ind_{}", idx),
                name: format!("Indicator {}", idx),
                description: "Test observable.".into(),
                observable: if idx % 2 == 0 {
                    ObservableDiscipline::SIGINT
                } else {
                    ObservableDiscipline::HUMINT
                },
                detectability: 0.4,
                time_to_detect_ticks: Some(7),
                monitoring_cost_annual: Some(1_500_000.0),
            });
        }
    }

    // Add an event with two defender options — one stand-down, one
    // costed pre-positioned response.
    let event_id = EventId::from("tripwire");
    scenario.events.insert(
        event_id.clone(),
        EventDefinition {
            id: event_id,
            name: "Adversary Tripwire".into(),
            description: "Adversary crosses a coalition red line.".into(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![],
            // Force the event probability to zero so adding it does not
            // perturb the engine's RNG draws / output for runs that are
            // not exercising the event itself. The Policy Implications
            // section reads `defender_options` declaratively; it does
            // not require the event to fire.
            probability: 0.0,
            repeatable: false,
            effects: vec![EventEffect::TensionShift { delta: 0.05 }],
            chain: None,
            defender_options: vec![
                DefenderOption {
                    key: "stand_down".into(),
                    name: "Diplomatic De-escalation".into(),
                    description: "Defender accepts and negotiates.".into(),
                    preparedness_cost: 0.0,
                    modifier_effects: vec![],
                },
                DefenderOption {
                    key: "pre_positioned".into(),
                    name: "Pre-positioned Response".into(),
                    description: "Costed standing strike package.".into(),
                    preparedness_cost: 2_500_000.0,
                    modifier_effects: vec![EventEffect::TensionShift { delta: -0.05 }],
                },
            ],
        },
    );

    // Tag the attacker with an escalation ladder.
    if let Some(red) = scenario.factions.get_mut(&FactionId::from("red")) {
        red.escalation_rules = Some(EscalationRules {
            posture: "Grey-zone permitted; kinetic strikes need authorization.".into(),
            de_escalation_floor: Some(0.4),
            ladder: vec![
                EscalationRung {
                    id: "grey_zone".into(),
                    name: "Grey Zone".into(),
                    description: "Sabotage, info ops, deniable cyber.".into(),
                    trigger_tension: None,
                    permitted_actions: vec!["Disinformation".into(), "Deniable cyber".into()],
                    prohibited_actions: vec!["Kinetic strikes on coalition soil".into()],
                },
                EscalationRung {
                    id: "kinetic".into(),
                    name: "Kinetic".into(),
                    description: "Open military action.".into(),
                    trigger_tension: Some(0.85),
                    permitted_actions: vec!["Long-range precision strikes".into()],
                    prohibited_actions: vec!["Nuclear signalling".into()],
                },
            ],
        });
    }

    scenario
}

#[test]
fn report_renders_policy_implications_section_with_data() {
    let scenario = epic_b_populated_scenario();
    let config = MonteCarloConfig {
        num_runs: 20,
        seed: Some(123),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("## Policy Implications"),
        "Policy Implications header missing; got:\n{md}"
    );
    assert!(
        md.contains("### Defender Options on Events"),
        "defender options subsection missing"
    );
    assert!(
        md.contains("`tripwire`"),
        "should reference the tripwire event id"
    );
    assert!(
        md.contains("Diplomatic De-escalation"),
        "first defender option missing"
    );
    assert!(
        md.contains("preparedness cost **$2500000**"),
        "second option's preparedness cost should render; got:\n{md}"
    );

    assert!(
        md.contains("### Escalation Rules"),
        "escalation rules subsection missing"
    );
    assert!(
        md.contains("De-escalation floor"),
        "de-escalation floor line should render"
    );
    assert!(md.contains("`kinetic`"), "ladder rung id should render");
    assert!(
        md.contains("@ tension ≥ **0.85**"),
        "rung trigger should render with the tension threshold"
    );
}

#[test]
fn report_renders_countermeasure_analysis_section_with_data() {
    let scenario = epic_b_populated_scenario();
    let config = MonteCarloConfig {
        num_runs: 20,
        seed: Some(123),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        md.contains("## Countermeasure Analysis"),
        "Countermeasure Analysis header missing; got:\n{md}"
    );
    assert!(md.contains("SIGINT"), "SIGINT discipline should render");
    assert!(md.contains("HUMINT"), "HUMINT discipline should render");
    // Detectability formats as percentage.
    assert!(
        md.contains("40%"),
        "detectability should render as a percentage"
    );
    assert!(md.contains("7 ticks"), "time-to-detect should render");
    assert!(
        md.contains("$1500000"),
        "annual monitoring cost should render"
    );

    // Table column count check: the indicator rows must have 6 cells
    // (matching the 6-column header). Each row should have 7 pipes
    // (6 cells = 7 separators including outer pipes).
    let section_start = md
        .find("## Countermeasure Analysis")
        .expect("section present");
    let rest = &md[section_start..];
    let section_end = rest[1..].find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
    let section = &rest[..section_end];
    let mut indicator_rows = 0;
    for line in section.lines() {
        if line.starts_with("| `") && !line.contains("---") && !line.contains("Phase |") {
            let pipes = line.chars().filter(|c| *c == '|').count();
            assert_eq!(
                pipes, 7,
                "indicator row should have 7 pipes (6 cells); got {pipes} in:\n{line}"
            );
            indicator_rows += 1;
        }
    }
    assert!(
        indicator_rows >= 2,
        "should render at least 2 indicator rows; got {indicator_rows}"
    );
}

#[test]
fn report_elides_policy_and_countermeasure_when_empty() {
    // Stock flagged scenario has no defender_options / escalation_rules /
    // warning_indicators — both new sections must elide cleanly so we
    // don't spam empty sections into legacy reports.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 20,
        seed: Some(7),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC");
    let md = render_markdown(&result.summary, &scenario);

    assert!(
        !md.contains("## Policy Implications"),
        "Policy Implications must elide when no data; got:\n{md}"
    );
    assert!(
        !md.contains("## Countermeasure Analysis"),
        "Countermeasure Analysis must elide when no data; got:\n{md}"
    );
    // Methodology must still render.
    assert!(md.contains("## Methodology & Confidence"));
}

#[test]
fn counterfactual_reports_chain_delta_when_phase_param_changed() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 30,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let overrides = vec![
        ParamOverride::parse("kill_chain.alpha.phase.recon.detection_probability_per_tick=0.5")
            .expect("parse"),
    ];
    let report = run_counterfactual(&scenario, &config, &overrides).expect("counterfactual");

    assert_eq!(report.variants.len(), 1);
    assert_eq!(report.deltas.len(), 1);
    let chain_id = KillChainId::from("alpha");
    let chain_delta = report.deltas[0]
        .chain_deltas
        .get(&chain_id)
        .expect("alpha chain delta present");
    // Hardening detection from 0.05 → 0.5 should detectably push the
    // detection rate up. We don't pin the exact magnitude (sampling
    // dependent at n=30), only the sign.
    assert!(
        chain_delta.detection_rate_delta >= 0.0,
        "detection delta should be non-negative when defender hardens; got {}",
        chain_delta.detection_rate_delta
    );
}

#[test]
fn counterfactual_is_deterministic_under_fixed_seed() {
    // Determinism contract — the comparison report (JSON-serialized to
    // strip any in-memory pointer noise) must be bit-identical across
    // two calls with the same seed and overrides. If a future refactor
    // breaks per-run-index seed derivation in MonteCarloRunner, this
    // test catches it before the comparison reports become noisy.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 15,
        seed: Some(2026),
        collect_snapshots: false,
        parallel: false,
    };
    let overrides = vec![ParamOverride::parse("political_climate.tension=0.9").expect("parse")];

    let a = run_counterfactual(&scenario, &config, &overrides).expect("a");
    let b = run_counterfactual(&scenario, &config, &overrides).expect("b");

    let ja = serde_json::to_string(&a).expect("ser a");
    let jb = serde_json::to_string(&b).expect("ser b");
    assert_eq!(
        ja, jb,
        "two run_counterfactual calls with identical inputs must produce bit-identical JSON"
    );
}

#[test]
fn counterfactual_stacks_multiple_overrides_atomically() {
    // Stacking two overrides should produce *one* variant scenario
    // with both applied — not two variants with one override each.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 10,
        seed: Some(1),
        collect_snapshots: false,
        parallel: false,
    };
    let overrides = vec![
        ParamOverride::parse("political_climate.tension=0.9").expect("p1"),
        ParamOverride::parse("kill_chain.alpha.phase.recon.base_success_probability=0.1")
            .expect("p2"),
    ];
    let report = run_counterfactual(&scenario, &config, &overrides).expect("counterfactual");
    assert_eq!(
        report.variants.len(),
        1,
        "stacked overrides must collapse to a single variant"
    );
    assert_eq!(
        report.variants[0].overrides.len(),
        2,
        "the variant's override list should record both overrides"
    );
}

#[test]
fn run_compare_against_self_produces_zero_deltas() {
    // Sanity check: comparing a scenario against itself with the same
    // seed/run-count must yield zero deltas everywhere. Anything else
    // means the engine isn't deterministic across two calls with the
    // same seed.
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 12,
        seed: Some(31),
        collect_snapshots: false,
        parallel: false,
    };
    let report = run_compare(&scenario, &scenario, "self", &config).expect("compare against self");

    let delta = &report.deltas[0];
    assert!(
        delta.mean_duration_delta.abs() < 1e-9,
        "self-compare mean-duration delta should be zero; got {}",
        delta.mean_duration_delta
    );
    for (fid, d) in &delta.win_rate_deltas {
        assert!(
            d.abs() < 1e-9,
            "self-compare win-rate delta for {fid} should be zero; got {d}"
        );
    }
    for (cid, cd) in &delta.chain_deltas {
        assert!(
            cd.overall_success_rate_delta.abs() < 1e-9
                && cd.detection_rate_delta.abs() < 1e-9
                && cd.cost_asymmetry_ratio_delta.abs() < 1e-9
                && cd.attacker_spend_delta.abs() < 1e-9
                && cd.defender_spend_delta.abs() < 1e-9,
            "self-compare chain-delta for {cid} must be all-zero; got {cd:?}"
        );
    }
}

#[test]
fn render_comparison_markdown_contains_expected_sections() {
    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 12,
        seed: Some(13),
        collect_snapshots: false,
        parallel: false,
    };
    let overrides = vec![ParamOverride::parse("political_climate.tension=0.7").expect("parse")];
    let report = run_counterfactual(&scenario, &config, &overrides).expect("counterfactual");

    let md = render_comparison_markdown(&report, &scenario);
    assert!(
        md.contains("# Faultline Counterfactual Report"),
        "comparison report must carry the expected H1; got:\n{md}"
    );
    assert!(
        md.contains("## Variant: counterfactual"),
        "variant section header missing"
    );
    assert!(
        md.contains("Applied overrides"),
        "applied overrides line missing"
    );
    assert!(
        md.contains("`political_climate.tension`"),
        "applied override must list the path"
    );
    assert!(
        md.contains("# Baseline Full Report"),
        "must embed the baseline report below the deltas"
    );
}

#[test]
fn comparison_omits_optional_subsections_when_empty() {
    // The minimal scenario carries no kill chains and no win rates
    // (it stalemates); the variant section should still render the
    // mean-duration line but elide both the win-rate-delta and
    // chain-delta tables instead of emitting empty 0-row tables.
    let mut scenario = flagged_chain_scenario();
    scenario.kill_chains.clear();
    let config = MonteCarloConfig {
        num_runs: 8,
        seed: Some(91),
        collect_snapshots: false,
        parallel: false,
    };
    let overrides = vec![ParamOverride::parse("political_climate.tension=0.6").expect("parse")];
    let report = run_counterfactual(&scenario, &config, &overrides).expect("cf");

    let md = render_comparison_markdown(&report, &scenario);
    assert!(
        !md.contains("### Kill-chain deltas"),
        "kill-chain delta table must elide when no chains; got:\n{md}"
    );
    assert!(
        md.contains("Mean duration delta"),
        "mean-duration line should still render"
    );
}

// ---------------------------------------------------------------------------
// Epic Q — Manifest determinism integration tests
// ---------------------------------------------------------------------------

/// The manifest hash is the citation-grade identity for a Faultline
/// run. Two runs with the same scenario, seed, and run count must
/// produce identical scenario_hash, output_hash, and manifest_hash.
/// If this drifts, every external citation of a Faultline run becomes
/// invalid.
#[test]
fn manifest_hashes_are_deterministic_across_runs() {
    use faultline_stats::manifest::{
        ManifestMcConfig, ManifestMode, build_manifest, scenario_hash, summary_hash,
    };

    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 25,
        seed: Some(2026),
        collect_snapshots: false,
        parallel: false,
    };

    let r1 = MonteCarloRunner::run(&config, &scenario).expect("run 1");
    let r2 = MonteCarloRunner::run(&config, &scenario).expect("run 2");

    let s1 = scenario_hash(&scenario).expect("scenario hash 1");
    let s2 = scenario_hash(&scenario).expect("scenario hash 2");
    assert_eq!(s1, s2, "scenario_hash must be deterministic");

    let o1 = summary_hash(&r1.summary).expect("output hash 1");
    let o2 = summary_hash(&r2.summary).expect("output hash 2");
    assert_eq!(
        o1, o2,
        "output_hash must be identical across two MC runs with the same seed"
    );

    let mc = ManifestMcConfig::from_config(&config, 2026);
    let m1 = build_manifest(
        "test.toml".into(),
        s1.clone(),
        mc.clone(),
        ManifestMode::MonteCarlo,
        o1.clone(),
    )
    .expect("manifest 1");
    let m2 = build_manifest("test.toml".into(), s2, mc, ManifestMode::MonteCarlo, o2)
        .expect("manifest 2");
    assert_eq!(
        m1.manifest_hash, m2.manifest_hash,
        "manifest_hash must be identical across two MC runs with the same seed"
    );
    assert!(
        !m1.manifest_hash.is_empty(),
        "manifest_hash must be a populated hex string"
    );
    // Sanity-check the hash is a 64-char SHA-256 hex digest.
    assert_eq!(
        m1.manifest_hash.len(),
        64,
        "manifest hash should be 64 hex chars"
    );
    assert_eq!(s1.len(), 64, "scenario hash should be 64 hex chars");
    assert_eq!(o1.len(), 64, "output hash should be 64 hex chars");
}

/// Bumping a parameter must flip the scenario_hash, the output_hash,
/// AND the manifest_hash. If any one of these doesn't change, the
/// citation system would silently report "this run is the same as
/// that one" when the inputs differ.
#[test]
fn manifest_hashes_react_to_parameter_changes() {
    use faultline_stats::manifest::{scenario_hash, summary_hash};

    let scenario_a = flagged_chain_scenario();
    let mut scenario_b = scenario_a.clone();
    // Mutate something semantically meaningful.
    scenario_b.political_climate.tension = 0.99;

    let config = MonteCarloConfig {
        num_runs: 10,
        seed: Some(7),
        collect_snapshots: false,
        parallel: false,
    };

    let sh_a = scenario_hash(&scenario_a).expect("hash a");
    let sh_b = scenario_hash(&scenario_b).expect("hash b");
    assert_ne!(
        sh_a, sh_b,
        "mutating a scenario field must flip scenario_hash"
    );

    let r_a = MonteCarloRunner::run(&config, &scenario_a).expect("run a");
    let r_b = MonteCarloRunner::run(&config, &scenario_b).expect("run b");
    let oh_a = summary_hash(&r_a.summary).expect("output hash a");
    let oh_b = summary_hash(&r_b.summary).expect("output hash b");
    assert_ne!(
        oh_a, oh_b,
        "different scenarios should produce different output hashes (with this seed)"
    );
}

/// `verify_manifest` must accept identical replays (the happy path).
#[test]
fn verify_accepts_identical_replay() {
    use faultline_stats::manifest::{
        ManifestMcConfig, ManifestMode, VerifyResult, build_manifest, scenario_hash, summary_hash,
        verify_manifest,
    };

    let scenario = flagged_chain_scenario();
    let config = MonteCarloConfig {
        num_runs: 12,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };

    let r1 = MonteCarloRunner::run(&config, &scenario).expect("run 1");
    let r2 = MonteCarloRunner::run(&config, &scenario).expect("run 2");
    let sh = scenario_hash(&scenario).expect("scenario hash");
    let mc = ManifestMcConfig::from_config(&config, 42);

    let m1 = build_manifest(
        "test.toml".into(),
        sh.clone(),
        mc.clone(),
        ManifestMode::MonteCarlo,
        summary_hash(&r1.summary).expect("oh1"),
    )
    .expect("manifest 1");
    let m2 = build_manifest(
        "test.toml".into(),
        sh,
        mc,
        ManifestMode::MonteCarlo,
        summary_hash(&r2.summary).expect("oh2"),
    )
    .expect("manifest 2");

    assert_eq!(verify_manifest(&m1, &m2), VerifyResult::Match);
}
