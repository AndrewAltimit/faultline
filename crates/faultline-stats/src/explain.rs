//! Structured "what does this scenario actually model?" summary.
//!
//! Epic P sub-item: a pure function over [`Scenario`] producing a
//! purposeful subset — factions, victory conditions, kill chains,
//! decision-variable surface, low-confidence parameters — rendered as
//! human-readable Markdown or as JSON for downstream tooling.
//!
//! The intent is to force every scenario to answer the same question
//! R3-2 asks of the engine: *which parameters does this scenario
//! actually move?* The strategy-space variables, the kill-chain
//! attribution / detection / cost knobs, and the author-flagged
//! low-confidence cells are surfaced together so an analyst can see
//! at a glance which assumptions a counterfactual would have to push
//! on.
//!
//! Pure function — no engine invocation, no RNG, no I/O. Safe to call
//! repeatedly on the same scenario; output is fully determined by the
//! inputs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use faultline_types::campaign::{CampaignPhase, KillChain};
use faultline_types::faction::{Diplomacy, Faction, FactionType, MilitaryBranch};
use faultline_types::ids::{FactionId, KillChainId, NetworkId, PhaseId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::ConfidenceLevel;
use faultline_types::strategy::Doctrine;
use faultline_types::strategy_space::{Domain, StrategySpace};
use faultline_types::victory::{NonKineticMetric, VictoryType};

use crate::markdown::escape_md_cell;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Top-level explain output.
///
/// Serializes cleanly to JSON for tooling. Renders to Markdown via
/// [`render_markdown`] for the CLI's text output mode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainReport {
    pub meta: ExplainMeta,
    pub scale: ExplainScale,
    pub factions: Vec<ExplainFaction>,
    pub kill_chains: Vec<ExplainKillChain>,
    pub victory_conditions: Vec<ExplainVictory>,
    pub networks: Vec<ExplainNetwork>,
    pub strategy_space: ExplainStrategySpace,
    pub low_confidence: Vec<ExplainLowConfidence>,
}

/// Author-supplied scenario metadata, lightly distilled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainMeta {
    pub name: String,
    pub author: String,
    pub version: String,
    pub schema_version: u32,
    pub tags: Vec<String>,
    pub description: String,
    pub confidence: Option<ConfidenceLevel>,
}

/// Counts that summarize scenario size at a glance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainScale {
    pub regions: usize,
    pub factions: usize,
    pub kill_chains: usize,
    pub events: usize,
    pub tech_cards: usize,
    pub networks: usize,
    pub victory_conditions: usize,
    pub max_ticks: u32,
    pub monte_carlo_runs: u32,
    pub attacker_budget: Option<f64>,
    pub defender_budget: Option<f64>,
}

/// Per-faction summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainFaction {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub doctrine: String,
    pub force_count: usize,
    pub initial_morale: f64,
    pub initial_resources: f64,
    pub has_leadership_cadre: bool,
    pub leadership_rank_count: usize,
    pub defender_role_count: usize,
    pub alliance_fracture_rule_count: usize,
    pub diplomacy: Vec<ExplainDiplomacy>,
}

/// One declared diplomatic relationship, in `from -> to: stance` form.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainDiplomacy {
    pub target: String,
    pub stance: String,
}

/// Per-kill-chain summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainKillChain {
    pub id: String,
    pub name: String,
    pub attacker: String,
    pub target: String,
    pub entry_phase: String,
    pub phase_count: usize,
    pub min_total_ticks: u32,
    pub max_total_ticks: u32,
    /// Phases the author flagged as Low confidence on parameter
    /// quality. Surfaced separately in the low-confidence section
    /// too; included here so the chain summary stands alone.
    pub low_confidence_phases: Vec<String>,
}

/// Per-victory-condition summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainVictory {
    pub id: String,
    pub name: String,
    pub faction: String,
    pub kind: String,
}

/// Per-network summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainNetwork {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub owner: Option<String>,
    pub node_count: usize,
    pub edge_count: usize,
}

/// Decision-variable surface — the parameters this scenario *actually
/// moves* under `--search`, `--coevolve`, etc.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExplainStrategySpace {
    pub variable_count: usize,
    pub variables: Vec<ExplainDecisionVariable>,
    pub objectives: Vec<String>,
    pub attacker_profiles: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainDecisionVariable {
    pub path: String,
    pub owner: Option<String>,
    pub domain: String,
}

/// One author-flagged low-confidence cell. The location identifies
/// which knob the analyst should probe under counterfactual.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainLowConfidence {
    pub location: String,
    pub level: ConfidenceLevel,
    pub note: String,
}

// ---------------------------------------------------------------------------
// Producer
// ---------------------------------------------------------------------------

/// Build a structured explain report from a validated scenario.
pub fn explain(scenario: &Scenario) -> ExplainReport {
    ExplainReport {
        meta: build_meta(scenario),
        scale: build_scale(scenario),
        factions: build_factions(scenario),
        kill_chains: build_kill_chains(scenario),
        victory_conditions: build_victory_conditions(scenario),
        networks: build_networks(scenario),
        strategy_space: build_strategy_space(&scenario.strategy_space),
        low_confidence: build_low_confidence(scenario),
    }
}

fn build_meta(scenario: &Scenario) -> ExplainMeta {
    ExplainMeta {
        name: scenario.meta.name.clone(),
        author: scenario.meta.author.clone(),
        version: scenario.meta.version.clone(),
        schema_version: scenario.meta.schema_version,
        tags: scenario.meta.tags.clone(),
        description: scenario.meta.description.clone(),
        confidence: scenario.meta.confidence.clone(),
    }
}

fn build_scale(scenario: &Scenario) -> ExplainScale {
    ExplainScale {
        regions: scenario.map.regions.len(),
        factions: scenario.factions.len(),
        kill_chains: scenario.kill_chains.len(),
        events: scenario.events.len(),
        tech_cards: scenario.technology.len(),
        networks: scenario.networks.len(),
        victory_conditions: scenario.victory_conditions.len(),
        max_ticks: scenario.simulation.max_ticks,
        monte_carlo_runs: scenario.simulation.monte_carlo_runs,
        attacker_budget: scenario.attacker_budget,
        defender_budget: scenario.defender_budget,
    }
}

fn build_factions(scenario: &Scenario) -> Vec<ExplainFaction> {
    scenario
        .factions
        .iter()
        .map(|(id, f)| build_faction(id, f))
        .collect()
}

fn build_faction(id: &FactionId, f: &Faction) -> ExplainFaction {
    let kind = faction_type_label(&f.faction_type);
    let doctrine = doctrine_label(&f.doctrine).to_string();
    let leadership_rank_count = f.leadership.as_ref().map(|c| c.ranks.len()).unwrap_or(0);
    let alliance_fracture_rule_count = f
        .alliance_fracture
        .as_ref()
        .map(|a| a.rules.len())
        .unwrap_or(0);
    let diplomacy = f
        .diplomacy
        .iter()
        .map(|d| ExplainDiplomacy {
            target: d.target_faction.0.clone(),
            stance: diplomacy_label(&d.stance).to_string(),
        })
        .collect();
    ExplainFaction {
        id: id.0.clone(),
        name: f.name.clone(),
        kind,
        doctrine,
        force_count: f.forces.len(),
        initial_morale: f.initial_morale,
        initial_resources: f.initial_resources,
        has_leadership_cadre: f.leadership.is_some(),
        leadership_rank_count,
        defender_role_count: f.defender_capacities.len(),
        alliance_fracture_rule_count,
        diplomacy,
    }
}

fn faction_type_label(t: &FactionType) -> String {
    match t {
        FactionType::Government { institutions } => {
            format!("Government ({} institutions)", institutions.len())
        },
        FactionType::Military { branch } => format!("Military ({})", military_branch_label(branch)),
        FactionType::Insurgent => "Insurgent".to_string(),
        FactionType::Civilian => "Civilian".to_string(),
        FactionType::PrivateMilitary => "PrivateMilitary".to_string(),
        FactionType::Foreign { is_proxy } => {
            if *is_proxy {
                "Foreign (proxy)".to_string()
            } else {
                "Foreign".to_string()
            }
        },
    }
}

fn military_branch_label(b: &MilitaryBranch) -> String {
    match b {
        MilitaryBranch::Army => "Army".to_string(),
        MilitaryBranch::Navy => "Navy".to_string(),
        MilitaryBranch::AirForce => "AirForce".to_string(),
        MilitaryBranch::Marines => "Marines".to_string(),
        MilitaryBranch::SpaceForce => "SpaceForce".to_string(),
        MilitaryBranch::CoastGuard => "CoastGuard".to_string(),
        MilitaryBranch::Combined => "Combined".to_string(),
        MilitaryBranch::Custom(s) => format!("Custom({s})"),
    }
}

fn diplomacy_label(d: &Diplomacy) -> &'static str {
    match d {
        Diplomacy::War => "War",
        Diplomacy::Hostile => "Hostile",
        Diplomacy::Neutral => "Neutral",
        Diplomacy::Cooperative => "Cooperative",
        Diplomacy::Allied => "Allied",
    }
}

fn doctrine_label(d: &Doctrine) -> &'static str {
    match d {
        Doctrine::Conventional => "Conventional",
        Doctrine::Guerrilla => "Guerrilla",
        Doctrine::Defensive => "Defensive",
        Doctrine::Disruption => "Disruption",
        Doctrine::CounterInsurgency => "CounterInsurgency",
        Doctrine::Blitzkrieg => "Blitzkrieg",
        Doctrine::Adaptive => "Adaptive",
    }
}

fn build_kill_chains(scenario: &Scenario) -> Vec<ExplainKillChain> {
    scenario
        .kill_chains
        .iter()
        .map(|(id, kc)| build_kill_chain(id, kc))
        .collect()
}

fn build_kill_chain(id: &KillChainId, kc: &KillChain) -> ExplainKillChain {
    let phase_count = kc.phases.len();
    let (min_total, max_total) = kill_chain_path_bounds(kc);
    // `kc.phases` is a `BTreeMap<PhaseId, _>` and `PhaseId`'s derived `Ord`
    // delegates to its inner `String`, so the filtered Vec is already in
    // sorted order — no explicit `.sort()` needed.
    let low_confidence_phases: Vec<String> = kc
        .phases
        .iter()
        .filter_map(|(pid, p)| match p.parameter_confidence {
            Some(ConfidenceLevel::Low) => Some(format_phase_id(pid)),
            _ => None,
        })
        .collect();
    ExplainKillChain {
        id: id.0.clone(),
        name: kc.name.clone(),
        attacker: kc.attacker.0.clone(),
        target: kc.target.0.clone(),
        entry_phase: format_phase_id(&kc.entry_phase),
        phase_count,
        min_total_ticks: min_total,
        max_total_ticks: max_total,
        low_confidence_phases,
    }
}

/// Compute `(min, max)` total ticks across the executable paths from
/// `entry_phase`. Each phase contributes its own duration plus the
/// best/worst contribution of any of its branches; a phase with no
/// branches is terminal (zero further contribution). For DAG kill
/// chains this is exact. If the chain contains a cycle (rare —
/// validation should reject most authoring mistakes that produce
/// one), the cycle is broken at the second visit so the recursion
/// terminates; the resulting bounds describe a single-traversal
/// estimate rather than an unbounded loop.
fn kill_chain_path_bounds(kc: &KillChain) -> (u32, u32) {
    let mut memo: BTreeMap<PhaseId, (u32, u32)> = BTreeMap::new();
    let mut visiting: BTreeSet<PhaseId> = BTreeSet::new();
    phase_path_bounds(&kc.phases, &kc.entry_phase, &mut memo, &mut visiting)
}

fn phase_path_bounds(
    phases: &BTreeMap<PhaseId, CampaignPhase>,
    pid: &PhaseId,
    memo: &mut BTreeMap<PhaseId, (u32, u32)>,
    visiting: &mut BTreeSet<PhaseId>,
) -> (u32, u32) {
    if let Some(cached) = memo.get(pid) {
        return *cached;
    }
    if visiting.contains(pid) {
        // Cycle: break at the re-entry point. The traversal contributes
        // 0 from this branch, which gives a defensible single-pass
        // estimate without diverging.
        return (0, 0);
    }
    let Some(phase) = phases.get(pid) else {
        // Branch points at an unknown next_phase id. Validation
        // should reject this; treat defensively as terminal so the
        // explain renderer can still produce a report.
        return (0, 0);
    };
    visiting.insert(pid.clone());

    let (mut branch_min, mut branch_max): (Option<u32>, Option<u32>) = (None, None);
    for branch in &phase.branches {
        let (b_min, b_max) = phase_path_bounds(phases, &branch.next_phase, memo, visiting);
        branch_min = Some(branch_min.map_or(b_min, |m| m.min(b_min)));
        branch_max = Some(branch_max.map_or(b_max, |m| m.max(b_max)));
    }

    let phase_min = phase.min_duration.saturating_add(branch_min.unwrap_or(0));
    let phase_max = phase.max_duration.saturating_add(branch_max.unwrap_or(0));

    visiting.remove(pid);
    memo.insert(pid.clone(), (phase_min, phase_max));
    (phase_min, phase_max)
}

fn format_phase_id(p: &PhaseId) -> String {
    p.0.clone()
}

fn build_victory_conditions(scenario: &Scenario) -> Vec<ExplainVictory> {
    scenario
        .victory_conditions
        .iter()
        .map(|(id, v)| ExplainVictory {
            id: id.0.clone(),
            name: v.name.clone(),
            faction: v.faction.0.clone(),
            kind: victory_kind_label(&v.condition),
        })
        .collect()
}

fn victory_kind_label(t: &VictoryType) -> String {
    match t {
        VictoryType::StrategicControl { threshold } => {
            format!("StrategicControl(>= {threshold:.2})")
        },
        VictoryType::MilitaryDominance {
            enemy_strength_below,
        } => format!("MilitaryDominance(enemy < {enemy_strength_below:.0})"),
        VictoryType::HoldRegions { regions, duration } => {
            format!("HoldRegions({} regions, {} ticks)", regions.len(), duration)
        },
        VictoryType::InstitutionalCollapse { trust_below } => {
            format!("InstitutionalCollapse(trust < {trust_below:.2})")
        },
        VictoryType::PeaceSettlement => "PeaceSettlement".to_string(),
        VictoryType::NonKineticThreshold { metric, threshold } => format!(
            "NonKineticThreshold({} >= {threshold:.2})",
            non_kinetic_label(metric)
        ),
        VictoryType::Custom {
            variable,
            threshold,
            above,
        } => {
            let direction = if *above { ">=" } else { "<=" };
            format!("Custom({variable} {direction} {threshold:.2})")
        },
    }
}

fn non_kinetic_label(m: &NonKineticMetric) -> &'static str {
    match m {
        NonKineticMetric::InformationDominance => "InformationDominance",
        NonKineticMetric::InstitutionalErosion => "InstitutionalErosion",
        NonKineticMetric::CoercionPressure => "CoercionPressure",
        NonKineticMetric::PoliticalCost => "PoliticalCost",
    }
}

fn build_networks(scenario: &Scenario) -> Vec<ExplainNetwork> {
    scenario
        .networks
        .iter()
        .map(|(id, n)| build_network(id, n))
        .collect()
}

fn build_network(id: &NetworkId, n: &faultline_types::network::Network) -> ExplainNetwork {
    ExplainNetwork {
        id: id.0.clone(),
        name: n.name.clone(),
        kind: n.kind.clone(),
        owner: n.owner.as_ref().map(|o| o.0.clone()),
        node_count: n.nodes.len(),
        edge_count: n.edges.len(),
    }
}

fn build_strategy_space(s: &StrategySpace) -> ExplainStrategySpace {
    let variables = s
        .variables
        .iter()
        .map(|v| ExplainDecisionVariable {
            path: v.path.clone(),
            owner: v.owner.as_ref().map(|o| o.0.clone()),
            domain: domain_label(&v.domain),
        })
        .collect::<Vec<_>>();
    let variable_count = variables.len();
    ExplainStrategySpace {
        variable_count,
        variables,
        objectives: s.objectives.iter().map(|o| o.label()).collect(),
        attacker_profiles: s.attacker_profiles.iter().map(|p| p.name.clone()).collect(),
    }
}

fn domain_label(d: &Domain) -> String {
    match d {
        Domain::Continuous { low, high, steps } => {
            format!("Continuous [{low:.4}, {high:.4}] / {steps} steps")
        },
        Domain::Discrete { values } => {
            let rendered: Vec<String> = values.iter().map(|v| format!("{v:.4}")).collect();
            format!("Discrete {{{}}}", rendered.join(", "))
        },
    }
}

fn build_low_confidence(scenario: &Scenario) -> Vec<ExplainLowConfidence> {
    let mut out: Vec<ExplainLowConfidence> = Vec::new();
    if matches!(scenario.meta.confidence, Some(ConfidenceLevel::Low)) {
        out.push(ExplainLowConfidence {
            location: "scenario".to_string(),
            level: ConfidenceLevel::Low,
            note: "Author flagged the scenario overall as Low confidence.".to_string(),
        });
    }
    for (cid, kc) in &scenario.kill_chains {
        for (pid, phase) in &kc.phases {
            if let Some(level @ ConfidenceLevel::Low) = phase.parameter_confidence.clone() {
                out.push(ExplainLowConfidence {
                    location: format!("kill_chain.{}.phase.{}", cid.0, pid.0),
                    level,
                    note: phase_low_confidence_note(phase),
                });
            }
            if let Some(level @ ConfidenceLevel::Low) = phase.cost.confidence.clone() {
                out.push(ExplainLowConfidence {
                    location: format!("kill_chain.{}.phase.{}.cost", cid.0, pid.0),
                    level,
                    note: format!(
                        "Cost figures (attacker ${:.0} / defender ${:.0}) are author estimates with wide uncertainty.",
                        phase.cost.attacker_dollars, phase.cost.defender_dollars
                    ),
                });
            }
        }
    }
    out
}

fn phase_low_confidence_note(phase: &faultline_types::campaign::CampaignPhase) -> String {
    format!(
        "base_success={:.2}, detection/tick={:.3}, attribution_difficulty={:.2} — author flagged parameter quality as Low.",
        phase.base_success_probability,
        phase.detection_probability_per_tick,
        phase.attribution_difficulty,
    )
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

/// Render a structured explain report as human-readable Markdown.
///
/// Section ordering and gating is stable: empty sections collapse to a
/// one-line "no entries" note rather than disappearing, so a reader who
/// expects (e.g.) a Networks section knows it was considered and found
/// empty rather than silently omitted.
pub fn render_markdown(report: &ExplainReport) -> String {
    let mut s = String::new();
    render_meta(&mut s, &report.meta);
    render_scale(&mut s, &report.scale);
    render_factions(&mut s, &report.factions);
    render_kill_chains(&mut s, &report.kill_chains);
    render_victory_conditions(&mut s, &report.victory_conditions);
    render_networks(&mut s, &report.networks);
    render_strategy_space(&mut s, &report.strategy_space);
    render_low_confidence(&mut s, &report.low_confidence);
    s
}

fn render_meta(s: &mut String, m: &ExplainMeta) {
    // Heading text is single-line by definition: a literal newline in
    // an authored name would split the `#` heading across two lines
    // and contaminate the next paragraph. Collapse line breaks so the
    // heading layout is stable regardless of author input.
    let name_line = collapse_newlines(non_empty_or(&m.name, "(unnamed scenario)"));
    s.push_str(&format!("# {name_line}\n\n"));
    s.push_str(&format!(
        "**Author:** {}  \n**Version:** {}  \n**Schema:** v{}\n",
        collapse_newlines(non_empty_or(&m.author, "—")),
        collapse_newlines(non_empty_or(&m.version, "—")),
        m.schema_version,
    ));
    if !m.tags.is_empty() {
        s.push_str(&format!(
            "**Tags:** {}\n",
            collapse_newlines(&m.tags.join(", "))
        ));
    }
    if let Some(level) = &m.confidence {
        s.push_str(&format!(
            "**Author confidence:** {}\n",
            confidence_label(level)
        ));
    }
    s.push('\n');
    if !m.description.trim().is_empty() {
        // The description is free-form prose intended for Markdown
        // rendering, so we deliberately preserve interior newlines
        // here rather than collapsing them — they're meaningful as
        // paragraph breaks.
        s.push_str(m.description.trim());
        s.push_str("\n\n");
    }
}

fn collapse_newlines(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

fn render_scale(s: &mut String, sc: &ExplainScale) {
    s.push_str("## Scale\n\n");
    s.push_str(&format!(
        "- Regions: {}\n- Factions: {}\n- Kill chains: {}\n- Events: {}\n- Tech cards: {}\n- Networks: {}\n- Victory conditions: {}\n- Max ticks: {}\n- Monte Carlo runs (scenario default): {}\n",
        sc.regions,
        sc.factions,
        sc.kill_chains,
        sc.events,
        sc.tech_cards,
        sc.networks,
        sc.victory_conditions,
        sc.max_ticks,
        sc.monte_carlo_runs,
    ));
    if let Some(b) = sc.attacker_budget {
        s.push_str(&format!("- Attacker budget cap: ${b:.0}\n"));
    }
    if let Some(b) = sc.defender_budget {
        s.push_str(&format!("- Defender budget cap: ${b:.0}\n"));
    }
    s.push('\n');
}

fn render_factions(s: &mut String, factions: &[ExplainFaction]) {
    s.push_str("## Factions\n\n");
    if factions.is_empty() {
        s.push_str("_No factions declared._\n\n");
        return;
    }
    s.push_str("| ID | Name | Type | Doctrine | Forces | Morale | Cadre | Defender roles | Fracture rules |\n");
    s.push_str("| --- | --- | --- | --- | ---: | ---: | :---: | ---: | ---: |\n");
    for f in factions {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} | {} | {} | {} |\n",
            escape_md_cell(&f.id),
            escape_md_cell(&f.name),
            escape_md_cell(&f.kind),
            escape_md_cell(&f.doctrine),
            f.force_count,
            f.initial_morale,
            cadre_marker(f.has_leadership_cadre, f.leadership_rank_count),
            f.defender_role_count,
            f.alliance_fracture_rule_count,
        ));
    }
    s.push('\n');

    let any_diplomacy = factions.iter().any(|f| !f.diplomacy.is_empty());
    if any_diplomacy {
        s.push_str("### Declared diplomacy\n\n");
        s.push_str("| From | Target | Stance |\n| --- | --- | --- |\n");
        for f in factions {
            for d in &f.diplomacy {
                s.push_str(&format!(
                    "| {} | {} | {} |\n",
                    escape_md_cell(&f.id),
                    escape_md_cell(&d.target),
                    escape_md_cell(&d.stance),
                ));
            }
        }
        s.push('\n');
    }
}

fn cadre_marker(has: bool, ranks: usize) -> String {
    if has {
        format!("{ranks}-rank")
    } else {
        "—".to_string()
    }
}

fn render_kill_chains(s: &mut String, chains: &[ExplainKillChain]) {
    s.push_str("## Kill chains\n\n");
    if chains.is_empty() {
        s.push_str("_No kill chains declared._\n\n");
        return;
    }
    s.push_str(
        "| ID | Name | Attacker | Target | Entry phase | Phases | Min ticks | Max ticks | Low-conf phases |\n",
    );
    s.push_str("| --- | --- | --- | --- | --- | ---: | ---: | ---: | --- |\n");
    for c in chains {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            escape_md_cell(&c.id),
            escape_md_cell(&c.name),
            escape_md_cell(&c.attacker),
            escape_md_cell(&c.target),
            escape_md_cell(&c.entry_phase),
            c.phase_count,
            c.min_total_ticks,
            c.max_total_ticks,
            if c.low_confidence_phases.is_empty() {
                "—".to_string()
            } else {
                escape_md_cell(&c.low_confidence_phases.join(", "))
            },
        ));
    }
    s.push('\n');
}

fn render_victory_conditions(s: &mut String, vs: &[ExplainVictory]) {
    s.push_str("## Victory conditions\n\n");
    if vs.is_empty() {
        s.push_str("_No victory conditions declared._\n\n");
        return;
    }
    s.push_str("| ID | Name | Faction | Kind |\n| --- | --- | --- | --- |\n");
    for v in vs {
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            escape_md_cell(&v.id),
            escape_md_cell(&v.name),
            escape_md_cell(&v.faction),
            escape_md_cell(&v.kind),
        ));
    }
    s.push('\n');
}

fn render_networks(s: &mut String, ns: &[ExplainNetwork]) {
    s.push_str("## Networks\n\n");
    if ns.is_empty() {
        s.push_str("_No networks declared._\n\n");
        return;
    }
    s.push_str(
        "| ID | Name | Kind | Owner | Nodes | Edges |\n| --- | --- | --- | --- | ---: | ---: |\n",
    );
    for n in ns {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            escape_md_cell(&n.id),
            escape_md_cell(&n.name),
            escape_md_cell(&n.kind),
            escape_md_cell(n.owner.as_deref().unwrap_or("—")),
            n.node_count,
            n.edge_count,
        ));
    }
    s.push('\n');
}

fn render_strategy_space(s: &mut String, ss: &ExplainStrategySpace) {
    s.push_str("## Decision-variable surface\n\n");
    s.push_str(
        "Parameters this scenario actually moves under `--search` / `--coevolve` / `--robustness`. \
         Empty = the scenario does not declare a `[strategy_space]` block; counterfactual analysis \
         must use ad-hoc `--counterfactual` paths.\n\n",
    );
    if ss.variable_count == 0 {
        s.push_str("_No decision variables declared._\n\n");
    } else {
        s.push_str("| Path | Owner | Domain |\n| --- | --- | --- |\n");
        for v in &ss.variables {
            s.push_str(&format!(
                "| {} | {} | {} |\n",
                escape_md_cell(&v.path),
                escape_md_cell(v.owner.as_deref().unwrap_or("—")),
                escape_md_cell(&v.domain),
            ));
        }
        s.push('\n');
    }
    if !ss.objectives.is_empty() {
        s.push_str("**Embedded objectives:** ");
        let escaped: Vec<String> = ss.objectives.iter().map(|o| escape_md_cell(o)).collect();
        s.push_str(&escaped.join(", "));
        s.push_str("\n\n");
    }
    if !ss.attacker_profiles.is_empty() {
        s.push_str("**Attacker profiles for robustness:** ");
        let escaped: Vec<String> = ss
            .attacker_profiles
            .iter()
            .map(|p| escape_md_cell(p))
            .collect();
        s.push_str(&escaped.join(", "));
        s.push_str("\n\n");
    }
}

fn render_low_confidence(s: &mut String, items: &[ExplainLowConfidence]) {
    s.push_str("## Low-confidence parameters\n\n");
    if items.is_empty() {
        s.push_str("_No author-flagged Low-confidence parameters._\n\n");
        return;
    }
    s.push_str(
        "Parameters the author marked as Low quality. These are the first \
         knobs to push on under `--counterfactual` or `--sensitivity` before \
         drawing conclusions from the run.\n\n",
    );
    s.push_str("| Location | Level | Note |\n| --- | --- | --- |\n");
    for item in items {
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            escape_md_cell(&item.location),
            confidence_label(&item.level),
            escape_md_cell(&item.note),
        ));
    }
    s.push('\n');
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn confidence_label(level: &ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::High => "High",
        ConfidenceLevel::Medium => "Medium",
        ConfidenceLevel::Low => "Low",
    }
}

fn non_empty_or<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.trim().is_empty() { fallback } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::campaign::{CampaignPhase, KillChain, PhaseBranch, PhaseCost};
    use faultline_types::faction::{Faction, FactionType};
    use faultline_types::ids::{FactionId, KillChainId, PhaseId, VictoryId};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::strategy_space::{
        DecisionVariable, Domain, SearchObjective, StrategySpace,
    };
    use faultline_types::victory::{VictoryCondition, VictoryType};

    fn minimal_scenario() -> Scenario {
        let alpha_id = FactionId::from("alpha");
        let mut s = Scenario {
            meta: ScenarioMeta {
                name: "Test Scenario".to_string(),
                description: "A test.".to_string(),
                author: "T".to_string(),
                version: "0.1".to_string(),
                tags: vec!["unit-test".to_string()],
                confidence: Some(ConfidenceLevel::Medium),
                schema_version: 1,
            },
            ..Default::default()
        };
        s.factions.insert(
            alpha_id.clone(),
            Faction {
                id: alpha_id,
                name: "Alpha".to_string(),
                faction_type: FactionType::Civilian,
                ..Default::default()
            },
        );
        s
    }

    #[test]
    fn explain_minimal_scenario_yields_meta_and_scale() {
        let s = minimal_scenario();
        let report = explain(&s);
        assert_eq!(report.meta.name, "Test Scenario");
        assert_eq!(report.scale.factions, 1);
        assert_eq!(report.scale.kill_chains, 0);
        assert!(report.kill_chains.is_empty());
        assert!(report.low_confidence.is_empty());
    }

    #[test]
    fn explain_renders_factions_table_when_factions_present() {
        let s = minimal_scenario();
        let report = explain(&s);
        let md = render_markdown(&report);
        assert!(md.contains("# Test Scenario"));
        assert!(md.contains("## Factions"));
        assert!(md.contains("| alpha "));
        assert!(md.contains("Civilian"));
        assert!(md.contains("## Decision-variable surface"));
        assert!(md.contains("_No decision variables declared._"));
    }

    #[test]
    fn empty_factions_collapses_to_placeholder() {
        let s = Scenario::default();
        let report = explain(&s);
        let md = render_markdown(&report);
        assert!(md.contains("_No factions declared._"));
        assert!(md.contains("_No kill chains declared._"));
        assert!(md.contains("_No victory conditions declared._"));
        assert!(md.contains("_No networks declared._"));
    }

    #[test]
    fn low_confidence_phases_surface_in_report() {
        let mut s = minimal_scenario();
        let chain_id = KillChainId::from("c1");
        let mut chain = KillChain {
            id: chain_id.clone(),
            name: "Chain 1".to_string(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("alpha"),
            entry_phase: PhaseId::from("p1"),
            phases: Default::default(),
        };
        chain.phases.insert(
            PhaseId::from("p1"),
            CampaignPhase {
                id: PhaseId::from("p1"),
                name: "P1".to_string(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 0.5,
                min_duration: 1,
                max_duration: 3,
                detection_probability_per_tick: 0.1,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    confidence: Some(ConfidenceLevel::Low),
                    ..Default::default()
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![],
                parameter_confidence: Some(ConfidenceLevel::Low),
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        s.kill_chains.insert(chain_id, chain);

        let report = explain(&s);
        assert_eq!(report.kill_chains.len(), 1);
        assert_eq!(report.kill_chains[0].low_confidence_phases, vec!["p1"]);
        // One phase param-confidence flag + one cost-confidence flag.
        assert_eq!(report.low_confidence.len(), 2);

        let md = render_markdown(&report);
        assert!(md.contains("kill_chain.c1.phase.p1"));
        assert!(md.contains("kill_chain.c1.phase.p1.cost"));
    }

    #[test]
    fn strategy_space_surfaces_decision_variables() {
        let mut s = minimal_scenario();
        s.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.alpha.initial_morale".to_string(),
                owner: Some(FactionId::from("alpha")),
                domain: Domain::Continuous {
                    low: 0.1,
                    high: 0.9,
                    steps: 5,
                },
            }],
            objectives: vec![SearchObjective::MinimizeDuration],
            attacker_profiles: vec![],
        };
        let report = explain(&s);
        assert_eq!(report.strategy_space.variable_count, 1);
        assert_eq!(report.strategy_space.objectives, vec!["minimize_duration"]);
        let md = render_markdown(&report);
        assert!(md.contains("faction.alpha.initial_morale"));
        assert!(md.contains("Continuous [0.1000, 0.9000] / 5 steps"));
        assert!(md.contains("**Embedded objectives:** minimize_duration"));
    }

    #[test]
    fn victory_kind_label_renders_each_variant() {
        // Pin label format so a future serialization tweak doesn't
        // silently change the explain output.
        assert_eq!(
            victory_kind_label(&VictoryType::StrategicControl { threshold: 0.6 }),
            "StrategicControl(>= 0.60)"
        );
        assert_eq!(
            victory_kind_label(&VictoryType::PeaceSettlement),
            "PeaceSettlement"
        );
        assert_eq!(
            victory_kind_label(&VictoryType::HoldRegions {
                regions: vec![],
                duration: 5,
            }),
            "HoldRegions(0 regions, 5 ticks)"
        );
    }

    #[test]
    fn escape_md_cell_neutralizes_pipes_and_newlines() {
        // Authored fields can contain pipes / newlines / backticks; the
        // table renderer must not let them break row layout. Mirrors
        // the test in `crate::markdown` so a local regression here is
        // caught even if the upstream test still passes.
        assert_eq!(escape_md_cell("a|b"), r"a\|b");
        assert_eq!(escape_md_cell("line1\nline2"), "line1 line2");
        assert_eq!(escape_md_cell("a`b"), r"a\`b");
        assert_eq!(escape_md_cell(r"x\y"), r"x\\y");
    }

    #[test]
    fn victory_conditions_surface_in_report() {
        let mut s = minimal_scenario();
        s.victory_conditions.insert(
            VictoryId::from("v1"),
            VictoryCondition {
                id: VictoryId::from("v1"),
                name: "Capture all regions".to_string(),
                faction: FactionId::from("alpha"),
                condition: VictoryType::StrategicControl { threshold: 0.75 },
            },
        );
        let report = explain(&s);
        assert_eq!(report.victory_conditions.len(), 1);
        let md = render_markdown(&report);
        assert!(md.contains("Capture all regions"));
        assert!(md.contains("StrategicControl(>= 0.75)"));
    }

    /// Helper for tests that need a phase with non-default duration
    /// and an explicit branch list.
    fn make_phase(id: &str, min: u32, max: u32, branches: Vec<PhaseBranch>) -> CampaignPhase {
        CampaignPhase {
            id: PhaseId::from(id),
            name: id.to_string(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 0.5,
            min_duration: min,
            max_duration: max,
            detection_probability_per_tick: 0.1,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: PhaseCost::default(),
            targets_domains: vec![],
            outputs: vec![],
            branches,
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        }
    }

    #[test]
    fn kill_chain_path_bounds_follow_branches_not_phase_sum() {
        // A → (B | C) with min/max durations:
        //   A: 1..2
        //   B: 5..6
        //   C: 1..1
        // Old (buggy) code: min = 1+5+1 = 7, max = 2+6+1 = 9.
        // Correct critical-path: min = 1+min(5,1) = 2, max = 2+max(6,1) = 8.
        use faultline_types::campaign::{BranchCondition, PhaseBranch};

        let mut s = minimal_scenario();
        let chain_id = KillChainId::from("c1");
        let mut chain = KillChain {
            id: chain_id.clone(),
            name: "Chain 1".to_string(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("alpha"),
            entry_phase: PhaseId::from("a"),
            phases: Default::default(),
        };
        chain.phases.insert(
            PhaseId::from("a"),
            make_phase(
                "a",
                1,
                2,
                vec![
                    PhaseBranch {
                        condition: BranchCondition::OnSuccess,
                        next_phase: PhaseId::from("b"),
                    },
                    PhaseBranch {
                        condition: BranchCondition::OnFailure,
                        next_phase: PhaseId::from("c"),
                    },
                ],
            ),
        );
        chain
            .phases
            .insert(PhaseId::from("b"), make_phase("b", 5, 6, vec![]));
        chain
            .phases
            .insert(PhaseId::from("c"), make_phase("c", 1, 1, vec![]));
        s.kill_chains.insert(chain_id, chain);

        let report = explain(&s);
        let kc = &report.kill_chains[0];
        assert_eq!(
            kc.min_total_ticks, 2,
            "min path should follow shortest branch"
        );
        assert_eq!(
            kc.max_total_ticks, 8,
            "max path should follow longest branch"
        );
    }

    #[test]
    fn kill_chain_path_bounds_break_cycles_safely() {
        // A → B → A — with min/max bounds, a naive recursion would
        // diverge. The traversal must terminate and produce finite
        // bounds.
        use faultline_types::campaign::{BranchCondition, PhaseBranch};

        let mut s = minimal_scenario();
        let chain_id = KillChainId::from("c-cycle");
        let mut chain = KillChain {
            id: chain_id.clone(),
            name: "Cycle".to_string(),
            description: String::new(),
            attacker: FactionId::from("alpha"),
            target: FactionId::from("alpha"),
            entry_phase: PhaseId::from("a"),
            phases: Default::default(),
        };
        chain.phases.insert(
            PhaseId::from("a"),
            make_phase(
                "a",
                1,
                2,
                vec![PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: PhaseId::from("b"),
                }],
            ),
        );
        chain.phases.insert(
            PhaseId::from("b"),
            make_phase(
                "b",
                3,
                4,
                vec![PhaseBranch {
                    condition: BranchCondition::Always,
                    next_phase: PhaseId::from("a"),
                }],
            ),
        );
        s.kill_chains.insert(chain_id, chain);

        let report = explain(&s);
        let kc = &report.kill_chains[0];
        // Cycle is broken at B's re-entry to A — both branches
        // contribute (a.min + b.min, a.max + b.max).
        assert_eq!(kc.min_total_ticks, 4);
        assert_eq!(kc.max_total_ticks, 6);
    }

    #[test]
    fn render_meta_collapses_newlines_in_heading() {
        // An author-supplied scenario name with a literal newline
        // would split the `# heading` line and contaminate the next
        // paragraph. The renderer must collapse line breaks.
        let mut s = minimal_scenario();
        s.meta.name = "Multi\nLine\rName".to_string();
        let md = render_markdown(&explain(&s));
        // The heading line is the first line of the rendered output.
        let first_line = md.lines().next().expect("at least one line");
        assert_eq!(first_line, "# Multi Line Name");
    }

    #[test]
    fn confidence_level_renders_when_set() {
        let mut s = minimal_scenario();
        s.meta.confidence = Some(ConfidenceLevel::Low);
        let report = explain(&s);
        let md = render_markdown(&report);
        assert!(md.contains("**Author confidence:** Low"));
        // Scenario-level Low also pushes one entry into the
        // low-confidence table so the analyst doesn't miss it when
        // skimming for parameter caveats.
        assert!(
            report
                .low_confidence
                .iter()
                .any(|i| i.location == "scenario")
        );
    }
}
