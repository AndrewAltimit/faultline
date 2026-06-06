//! Non-fatal advisory checks over a [`Scenario`].
//!
//! Distinct from [`faultline_engine::validate_scenario`], which returns
//! *hard* errors that block loading (empty scenario, dangling border,
//! out-of-range scalar, silent-no-op shapes). The checks here are
//! *advisory*: a scenario that trips them still loads and runs, but the
//! author has very likely made a modelling mistake — a faction with no
//! way to win, a region nobody ever touches, a kill-chain phase no
//! branch can reach.
//!
//! This is a pure function over [`Scenario`] — no engine invocation, no
//! RNG, no I/O. Output is fully determined by the input, so it is safe to
//! call repeatedly (e.g. on every editor keystroke in the browser). It is
//! deliberately *separate* from the deterministic Markdown report whose
//! hash is part of the manifest: nothing here is injected into that
//! report, so adding or refining a check never flips a bundled scenario's
//! `output_hash`.
//!
//! The browser surfaces these as an inline validation panel; the CLI can
//! reuse them via [`collect_warnings`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use faultline_types::campaign::{KillChain, PhaseOutput};
use faultline_types::ids::{PhaseId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::victory::VictoryType;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The advisory category a [`Warning`] belongs to. Stable string-tagged
/// for the JS panel to group / icon by category without parsing the
/// human-readable message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// A faction declares no victory condition, so it has no modelled
    /// path to win the scenario.
    FactionNoObjective,
    /// A region is declared on the map but referenced by nothing —
    /// no force occupies it, no victory condition holds it, no
    /// neighbour borders it, no kill-chain output targets it.
    UnreferencedRegion,
    /// A kill-chain phase exists but is unreachable from the chain's
    /// `entry_phase` via the branch graph.
    UnreachablePhase,
}

impl WarningKind {
    /// A short, stable, human-readable label for the category.
    pub fn label(self) -> &'static str {
        match self {
            WarningKind::FactionNoObjective => "Faction has no objective",
            WarningKind::UnreferencedRegion => "Unreferenced region",
            WarningKind::UnreachablePhase => "Unreachable kill-chain phase",
        }
    }
}

/// A single advisory finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    /// The primary scenario entity the finding is about (faction id,
    /// region id, or `kill_chain/phase`), so a UI can deep-link.
    pub subject: String,
    /// Full human-readable description, safe to render verbatim.
    pub message: String,
}

/// The advisory report: the ordered list of findings.
///
/// Serializes cleanly to JSON for the WASM frontend. An empty `warnings`
/// vector means the scenario passed every advisory check.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WarningReport {
    pub warnings: Vec<Warning>,
}

impl WarningReport {
    /// Whether any advisory finding was raised.
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Number of findings.
    pub fn len(&self) -> usize {
        self.warnings.len()
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run every advisory check over `scenario` and collect the findings.
///
/// Findings are returned in a deterministic order: grouped by check, and
/// within a check by the natural sort order of the subject id (regions /
/// factions / phases all live in `BTreeMap`s, so iteration is already
/// sorted).
pub fn collect_warnings(scenario: &Scenario) -> WarningReport {
    let mut warnings = Vec::new();
    check_factions_without_objective(scenario, &mut warnings);
    check_unreferenced_regions(scenario, &mut warnings);
    check_unreachable_phases(scenario, &mut warnings);
    WarningReport { warnings }
}

// ---------------------------------------------------------------------------
// Check: faction with no objective (no victory condition)
// ---------------------------------------------------------------------------

/// A faction's "objective" is expressed as the victory conditions that
/// name it. A faction that appears in no victory condition can never win
/// — it can only lose, stalemate, or be a passive third party. That is
/// occasionally intentional (a civilian segment, a foreign observer), so
/// this is advisory, not an error.
fn check_factions_without_objective(scenario: &Scenario, out: &mut Vec<Warning>) {
    let factions_with_victory: BTreeSet<&str> = scenario
        .victory_conditions
        .values()
        .map(|vc| vc.faction.0.as_str())
        .collect();

    for (fid, faction) in &scenario.factions {
        if !factions_with_victory.contains(fid.0.as_str()) {
            out.push(Warning {
                kind: WarningKind::FactionNoObjective,
                subject: fid.0.clone(),
                message: format!(
                    "Faction `{}` ({}) is named by no victory condition, so it has no \
                     modelled path to win. If this is intentional (a passive / civilian \
                     / observer faction) you can ignore this; otherwise add a victory \
                     condition with `faction = \"{}\"`.",
                    fid.0, faction.name, fid.0
                ),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Check: region declared but never referenced
// ---------------------------------------------------------------------------

/// Collect the set of region ids that are referenced by *something* other
/// than their own declaration, then flag any declared region not in it.
///
/// A region counts as referenced if any of the following name it:
/// a deployed force, a `HoldRegions` victory condition, an infrastructure
/// node, a terrain modifier, a kill-chain `InfraDamage` output, or the
/// `borders` list of another region. (A region that only borders *itself*
/// does not count — that would be a degenerate self-loop, not a real
/// reference.)
fn check_unreferenced_regions(scenario: &Scenario, out: &mut Vec<Warning>) {
    let mut referenced: BTreeSet<RegionId> = BTreeSet::new();

    // Borders of *other* regions.
    for (rid, region) in &scenario.map.regions {
        for neighbour in &region.borders {
            if neighbour != rid {
                referenced.insert(neighbour.clone());
            }
        }
    }

    // Deployed forces.
    for faction in scenario.factions.values() {
        for force in faction.forces.values() {
            referenced.insert(force.region.clone());
        }
    }

    // Infrastructure placement.
    for infra in scenario.map.infrastructure.values() {
        referenced.insert(infra.region.clone());
    }

    // Terrain modifiers.
    for terrain in &scenario.map.terrain {
        referenced.insert(terrain.region.clone());
    }

    // Victory conditions that hold named regions.
    for vc in scenario.victory_conditions.values() {
        if let VictoryType::HoldRegions { regions, .. } = &vc.condition {
            for r in regions {
                referenced.insert(r.clone());
            }
        }
    }

    // Kill-chain phase outputs that target a region.
    for kc in scenario.kill_chains.values() {
        for phase in kc.phases.values() {
            for output in &phase.outputs {
                // Exhaustive on purpose: `InfraDamage` is the only output
                // that names a `RegionId` today. Listing the rest forces a
                // compile error here the moment a new region-bearing variant
                // is added, so it can never silently produce a false-positive
                // `UnreferencedRegion` advisory.
                match output {
                    PhaseOutput::InfraDamage { region, .. } => {
                        referenced.insert(region.clone());
                    },
                    PhaseOutput::IntelligenceGain { .. }
                    | PhaseOutput::TensionDelta { .. }
                    | PhaseOutput::MoraleDelta { .. }
                    | PhaseOutput::InformationDominance { .. }
                    | PhaseOutput::InstitutionalErosion { .. }
                    | PhaseOutput::CoercionPressure { .. }
                    | PhaseOutput::PoliticalCost { .. }
                    | PhaseOutput::Custom { .. }
                    | PhaseOutput::LeadershipDecapitation { .. } => {},
                }
            }
        }
    }

    for rid in scenario.map.regions.keys() {
        if !referenced.contains(rid) {
            out.push(Warning {
                kind: WarningKind::UnreferencedRegion,
                subject: rid.0.clone(),
                message: format!(
                    "Region `{}` is declared on the map but referenced by nothing — no \
                     force occupies it, no victory condition holds it, no other region \
                     borders it, and no kill-chain output targets it. It will sit inert \
                     for the whole run. Add it to a neighbour's `borders`, deploy a force \
                     there, or remove it.",
                    rid.0
                ),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Check: kill-chain phase unreachable from entry_phase
// ---------------------------------------------------------------------------

/// Walk the branch graph from `entry_phase` and flag any declared phase
/// the walk never reaches. An unreachable phase is dead configuration:
/// the engine can never activate it, so its parameters never matter.
fn check_unreachable_phases(scenario: &Scenario, out: &mut Vec<Warning>) {
    for (kcid, kc) in &scenario.kill_chains {
        let reachable = reachable_phases(kc);
        for pid in kc.phases.keys() {
            if !reachable.contains(pid) {
                out.push(Warning {
                    kind: WarningKind::UnreachablePhase,
                    subject: format!("{}/{}", kcid.0, pid.0),
                    message: format!(
                        "Kill chain `{}`: phase `{}` is unreachable from the entry phase \
                         `{}` — no branch leads to it. It can never activate, so its \
                         parameters have no effect. Wire a `next_phase = \"{}\"` branch \
                         into a reachable phase, or remove it.",
                        kcid.0, pid.0, kc.entry_phase.0, pid.0
                    ),
                });
            }
        }
    }
}

/// The set of phase ids reachable from `kc.entry_phase` by following
/// `branches[].next_phase` edges. Robust to a dangling `entry_phase` (one
/// not present in `phases`): the BFS only enqueues neighbours that exist
/// in `phases`, and seeds only if the entry phase itself exists — a
/// dangling entry is a separate (hard-validation) concern, and treating
/// it here as "reaches nothing" would spuriously flag every real phase.
fn reachable_phases(kc: &KillChain) -> BTreeSet<PhaseId> {
    let mut reachable: BTreeSet<PhaseId> = BTreeSet::new();
    if !kc.phases.contains_key(&kc.entry_phase) {
        // Entry phase is itself dangling; don't manufacture unreachable
        // warnings for every phase. Mark all declared phases reachable so
        // this check stays silent and the hard validator owns the error.
        return kc.phases.keys().cloned().collect();
    }
    let mut frontier = vec![kc.entry_phase.clone()];
    reachable.insert(kc.entry_phase.clone());
    while let Some(pid) = frontier.pop() {
        let Some(phase) = kc.phases.get(&pid) else {
            continue;
        };
        for branch in &phase.branches {
            let next = &branch.next_phase;
            if kc.phases.contains_key(next) && reachable.insert(next.clone()) {
                frontier.push(next.clone());
            }
        }
    }
    reachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::campaign::{BranchCondition, CampaignPhase, KillChain, PhaseBranch};
    use faultline_types::ids::{FactionId, KillChainId, RegionId, VictoryId};
    use faultline_types::scenario::Scenario;
    use faultline_types::victory::{VictoryCondition, VictoryType};

    const TUTORIAL_TOML: &str = include_str!("../../../scenarios/tutorial_symmetric.toml");

    fn tutorial() -> Scenario {
        toml::from_str(TUTORIAL_TOML).expect("tutorial scenario should parse")
    }

    #[test]
    fn tutorial_scenario_is_clean() {
        // The bundled tutorial should trip no advisory checks; if it
        // does, either the scenario or a check has regressed.
        let report = collect_warnings(&tutorial());
        assert!(
            report.is_empty(),
            "tutorial should be advisory-clean, got: {:?}",
            report.warnings
        );
    }

    #[test]
    fn faction_without_victory_is_flagged() {
        let mut s = tutorial();
        // Drop every victory condition naming `bravo`.
        s.victory_conditions
            .retain(|_, vc| vc.faction != FactionId("bravo".into()));
        let report = collect_warnings(&s);
        let found = report
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::FactionNoObjective)
            .map(|w| w.subject.as_str())
            .collect::<Vec<_>>();
        assert_eq!(found, vec!["bravo"], "only bravo should be flagged");
    }

    #[test]
    fn faction_with_victory_is_not_flagged() {
        let s = tutorial();
        let report = collect_warnings(&s);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::FactionNoObjective),
            "tutorial factions all have victory conditions"
        );
    }

    #[test]
    fn unreferenced_region_is_flagged() {
        let mut s = tutorial();
        // Add a fresh region referenced by nobody and not bordered.
        let orphan = RegionId("orphan_region".into());
        let template = s
            .map
            .regions
            .values()
            .next()
            .expect("at least one region")
            .clone();
        let mut new_region = template;
        new_region.id = orphan.clone();
        new_region.borders = vec![];
        s.map.regions.insert(orphan.clone(), new_region);

        let report = collect_warnings(&s);
        let flagged: Vec<&str> = report
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::UnreferencedRegion)
            .map(|w| w.subject.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["orphan_region"],
            "only the orphan region should be flagged"
        );
    }

    #[test]
    fn region_referenced_only_by_border_is_not_flagged() {
        let mut s = tutorial();
        // A region with no force / victory / infra reference but that a
        // neighbour borders should NOT be flagged.
        let orphan = RegionId("border_only".into());
        let template = s.map.regions.values().next().expect("region").clone();
        let mut new_region = template;
        new_region.id = orphan.clone();
        new_region.borders = vec![];
        s.map.regions.insert(orphan.clone(), new_region);
        // Make an existing region border it.
        let first_key = s
            .map
            .regions
            .keys()
            .find(|k| **k != orphan)
            .expect("another region")
            .clone();
        s.map
            .regions
            .get_mut(&first_key)
            .expect("region")
            .borders
            .push(orphan.clone());

        let report = collect_warnings(&s);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreferencedRegion && w.subject == "border_only"),
            "a region another region borders is referenced"
        );
    }

    fn simple_phase(id: &str) -> CampaignPhase {
        CampaignPhase {
            id: PhaseId(id.into()),
            name: id.to_string(),
            description: String::new(),
            prerequisites: vec![],
            base_success_probability: 0.5,
            min_duration: 1,
            max_duration: 1,
            detection_probability_per_tick: 0.0,
            prerequisite_success_boost: 0.0,
            attribution_difficulty: 0.5,
            cost: Default::default(),
            targets_domains: vec![],
            outputs: vec![],
            branches: vec![],
            parameter_confidence: None,
            warning_indicators: vec![],
            defender_noise: vec![],
            gated_by_defender: None,
        }
    }

    fn kill_chain_with(phases: Vec<CampaignPhase>, entry: &str) -> KillChain {
        let mut map = std::collections::BTreeMap::new();
        for p in phases {
            map.insert(p.id.clone(), p);
        }
        KillChain {
            id: KillChainId("kc".into()),
            name: "kc".into(),
            description: String::new(),
            attacker: FactionId("alpha".into()),
            target: FactionId("bravo".into()),
            entry_phase: PhaseId(entry.into()),
            phases: map,
        }
    }

    #[test]
    fn unreachable_phase_is_flagged() {
        let mut entry = simple_phase("entry");
        entry.branches = vec![PhaseBranch {
            condition: BranchCondition::OnSuccess,
            next_phase: PhaseId("middle".into()),
        }];
        let middle = simple_phase("middle");
        let island = simple_phase("island"); // no branch reaches it
        let kc = kill_chain_with(vec![entry, middle, island], "entry");

        let mut s = tutorial();
        s.kill_chains.insert(kc.id.clone(), kc);

        let report = collect_warnings(&s);
        let flagged: Vec<&str> = report
            .warnings
            .iter()
            .filter(|w| w.kind == WarningKind::UnreachablePhase)
            .map(|w| w.subject.as_str())
            .collect();
        assert_eq!(flagged, vec!["kc/island"], "only island is unreachable");
    }

    #[test]
    fn all_reachable_phases_clean() {
        let mut entry = simple_phase("entry");
        entry.branches = vec![PhaseBranch {
            condition: BranchCondition::Always,
            next_phase: PhaseId("tail".into()),
        }];
        let tail = simple_phase("tail");
        let kc = kill_chain_with(vec![entry, tail], "entry");

        let mut s = tutorial();
        s.kill_chains.insert(kc.id.clone(), kc);

        let report = collect_warnings(&s);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreachablePhase),
            "every phase reachable, none should flag"
        );
    }

    #[test]
    fn dangling_entry_phase_does_not_flag_everything() {
        // A bogus entry phase is a hard-validation concern; the advisory
        // check must not spam an unreachable warning for every real phase.
        let entry = simple_phase("real_entry");
        let other = simple_phase("other");
        let kc = kill_chain_with(vec![entry, other], "does_not_exist");

        let mut s = tutorial();
        s.kill_chains.insert(kc.id.clone(), kc);

        let report = collect_warnings(&s);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::UnreachablePhase),
            "dangling entry should stay silent, not flag every phase"
        );
    }

    #[test]
    fn victory_named_faction_seeds_objective_set() {
        // Direct unit: a faction named only by a HoldRegions victory
        // still counts as having an objective.
        let mut s = tutorial();
        let fid = s.factions.keys().next().expect("faction").clone();
        s.victory_conditions.clear();
        s.victory_conditions.insert(
            VictoryId("v".into()),
            VictoryCondition {
                id: VictoryId("v".into()),
                name: "v".into(),
                faction: fid.clone(),
                condition: VictoryType::HoldRegions {
                    regions: vec![],
                    duration: 1,
                },
            },
        );
        let report = collect_warnings(&s);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.kind == WarningKind::FactionNoObjective && w.subject == fid.0),
            "faction with a HoldRegions victory has an objective"
        );
    }
}
