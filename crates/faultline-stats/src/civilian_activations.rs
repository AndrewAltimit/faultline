//! Cross-run civilian-segment activation rollup.
//!
//! Pure post-processing of `RunResult.civilian_activations` and
//! `Scenario.political_climate.population_segments`. No engine re-runs,
//! no RNG draws — determinism follows directly from the input.
//!
//! Returns an empty map when no scenario declares any
//! `population_segments`; the report renderer reads `BTreeMap::is_empty()`
//! as "elide the section entirely" so legacy scenarios produce no
//! output.

use std::collections::BTreeMap;

use faultline_types::ids::{FactionId, SegmentId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{RunResult, SegmentActivationSummary};

/// Build the cross-run civilian-activation rollup.
///
/// Iterates the scenario's declared segments in the order
/// `political_climate.population_segments` was authored — that's a
/// `Vec`, not a map, so order is stable and deterministic. For each
/// segment we walk every run's `civilian_activations` log to count
/// fires, capture fire ticks, and accumulate per-action firing counts.
///
/// `favored_faction` is the **modal** favored faction observed across
/// the run set — when sympathy drift drives different runs to different
/// favored factions, the most-common one wins, with ties resolved to
/// the lexicographically largest `FactionId` (deterministic consequence
/// of `Iterator::max_by_key` keeping the last maximum on a
/// `BTreeMap`-ordered iteration). This matches the report contract of
/// "show one representative faction per row" without losing the signal
/// that drift produced.
pub fn compute_civilian_activation_summaries(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<SegmentId, SegmentActivationSummary> {
    let mut out = BTreeMap::new();
    if scenario.political_climate.population_segments.is_empty() {
        return out;
    }

    let n_runs = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");

    for segment in &scenario.political_climate.population_segments {
        // Collect every activation event across the run set targeting
        // this segment. The engine's `activated` latch makes activation
        // one-shot per run, so each run contributes at most one row.
        let mut fire_ticks: Vec<u32> = Vec::new();
        let mut faction_counts: BTreeMap<FactionId, u32> = BTreeMap::new();
        let mut action_kind_counts: BTreeMap<String, u32> = BTreeMap::new();

        for run in runs {
            if let Some(ev) = run
                .civilian_activations
                .iter()
                .find(|ev| ev.segment == segment.id)
            {
                fire_ticks.push(ev.tick);
                *faction_counts
                    .entry(ev.favored_faction.clone())
                    .or_insert(0) += 1;
                for kind in &ev.action_kinds {
                    *action_kind_counts.entry(kind.clone()).or_insert(0) += 1;
                }
            }
        }

        let activation_count = u32::try_from(fire_ticks.len())
            .expect("fires per segment fit u32 (bounded by run count)");
        let activation_rate = if n_runs == 0 {
            0.0
        } else {
            f64::from(activation_count) / f64::from(n_runs)
        };
        let mean_activation_tick = if activation_count == 0 {
            None
        } else {
            let sum: u64 = fire_ticks.iter().map(|t| u64::from(*t)).sum();
            Some(sum as f64 / f64::from(activation_count))
        };

        // Modal favored faction. Ties resolve to the lexicographically
        // largest `FactionId` — `max_by_key` on a `BTreeMap`-ordered
        // iteration keeps the *last* maximum, which after ascending-
        // key iteration is the largest key. Deterministic either way;
        // the direction is documented for future readers. When no run
        // activated the segment we fall back to the highest-sympathy
        // faction declared in the scenario. That's the one closest to
        // crossing the activation threshold; reporting it as the
        // prospective beneficiary is more informative than reporting
        // the first declared faction (which might be the segment's
        // *opponent* under the scenario's authoring).
        let favored_faction = faction_counts
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(fid, _)| fid.clone())
            .unwrap_or_else(|| baseline_favored_faction(segment));

        out.insert(
            segment.id.clone(),
            SegmentActivationSummary {
                segment: segment.id.clone(),
                name: segment.name.clone(),
                favored_faction,
                n_runs,
                activation_count,
                activation_rate,
                mean_activation_tick,
                action_kind_counts,
            },
        );
    }

    out
}

/// Highest-sympathy faction declared on a segment. Used as the
/// fallback `favored_faction` for segments that never activated in
/// the run set. Scenario validation rejects empty `sympathies` lists
/// so the `unwrap_or_else` branch is unreachable on validated input;
/// it stays as a defense-in-depth fallback for callers (e.g. unit
/// tests) that construct `PopulationSegment` directly without going
/// through the validator.
fn baseline_favored_faction(segment: &faultline_types::politics::PopulationSegment) -> FactionId {
    segment
        .sympathies
        .iter()
        .max_by(|a, b| a.sympathy.total_cmp(&b.sympathy))
        .map(|s| s.faction.clone())
        .unwrap_or_else(|| FactionId::from("(none)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::SegmentId;
    use faultline_types::politics::{
        CivilianAction, FactionSympathy, PoliticalClimate, PopulationSegment,
    };
    use faultline_types::scenario::Scenario;
    use faultline_types::stats::{CivilianActivationEvent, Outcome, RunResult, StateSnapshot};

    fn empty_run() -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 10,
            final_state: StateSnapshot {
                tick: 10,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: Vec::new(),
            },
            snapshots: Vec::new(),
            event_log: Vec::new(),
            campaign_reports: BTreeMap::new(),
            defender_queue_reports: Vec::new(),
            network_reports: BTreeMap::new(),
            fracture_events: Vec::new(),
            supply_pressure_reports: BTreeMap::new(),
            civilian_activations: Vec::new(),
            tech_costs: BTreeMap::new(),
            narrative_events: Vec::new(),
            displacement_reports: BTreeMap::new(),
        }
    }

    fn scenario_with_segment(seg_id: &str) -> (Scenario, SegmentId) {
        let mut s = Scenario::default();
        let sid = SegmentId::from(seg_id);
        let segment = PopulationSegment {
            id: sid.clone(),
            name: "Urban Middle Class".into(),
            fraction: 0.4,
            concentrated_in: vec![],
            sympathies: vec![
                FactionSympathy {
                    faction: FactionId::from("gov"),
                    sympathy: 0.6,
                },
                FactionSympathy {
                    faction: FactionId::from("rebel"),
                    sympathy: -0.2,
                },
            ],
            activation_threshold: 0.75,
            activation_actions: vec![CivilianAction::Protest { intensity: 0.5 }],
            volatility: 0.5,
            activated: false,
        };
        s.political_climate = PoliticalClimate {
            tension: 0.5,
            institutional_trust: 0.5,
            media_landscape: Default::default(),
            population_segments: vec![segment],
            global_modifiers: vec![],
        };
        (s, sid)
    }

    #[test]
    fn returns_empty_when_no_segments_declared() {
        let s = Scenario::default();
        assert!(compute_civilian_activation_summaries(&[], &s).is_empty());
    }

    #[test]
    fn empty_runs_emit_zero_activation_summary() {
        // A segment is declared but no runs fired — the row still
        // emits with rate 0 and mean None so the report has a place
        // to surface "yes, this segment exists; no, it never tripped"
        // without disappearing.
        let (s, sid) = scenario_with_segment("urban");
        let out = compute_civilian_activation_summaries(&[], &s);
        let row = out.get(&sid).expect("row emitted for declared segment");
        assert_eq!(row.activation_count, 0);
        assert_eq!(row.n_runs, 0);
        assert!(row.mean_activation_tick.is_none());
        assert_eq!(
            row.favored_faction,
            FactionId::from("gov"),
            "highest-sympathy faction is the baseline favored"
        );
    }

    #[test]
    fn aggregates_fires_and_action_counts() {
        let (s, sid) = scenario_with_segment("urban");
        let mut runs = vec![empty_run(), empty_run(), empty_run()];
        runs[0].civilian_activations.push(CivilianActivationEvent {
            tick: 6,
            segment: sid.clone(),
            favored_faction: FactionId::from("gov"),
            action_kinds: vec!["Protest".into(), "NonCooperation".into()],
        });
        runs[1].civilian_activations.push(CivilianActivationEvent {
            tick: 10,
            segment: sid.clone(),
            favored_faction: FactionId::from("gov"),
            action_kinds: vec!["Protest".into()],
        });
        // run[2] does not activate.

        let out = compute_civilian_activation_summaries(&runs, &s);
        let row = out.get(&sid).expect("rule declared");
        assert_eq!(row.n_runs, 3);
        assert_eq!(row.activation_count, 2);
        assert!((row.activation_rate - 2.0 / 3.0).abs() < 1e-9);
        assert!((row.mean_activation_tick.expect("fires") - 8.0).abs() < 1e-9);
        assert_eq!(row.action_kind_counts.get("Protest"), Some(&2));
        assert_eq!(row.action_kind_counts.get("NonCooperation"), Some(&1));
    }

    #[test]
    fn modal_favored_faction_wins_when_drift_diverges() {
        // Two runs activate with `gov` as favored, one with `rebel` —
        // modal is `gov`. This pins the "drift across runs" contract:
        // the rollup picks the most-common favored faction rather
        // than e.g. the first observed.
        let (s, sid) = scenario_with_segment("urban");
        let mut runs = vec![empty_run(), empty_run(), empty_run()];
        runs[0].civilian_activations.push(CivilianActivationEvent {
            tick: 5,
            segment: sid.clone(),
            favored_faction: FactionId::from("rebel"),
            action_kinds: vec![],
        });
        runs[1].civilian_activations.push(CivilianActivationEvent {
            tick: 7,
            segment: sid.clone(),
            favored_faction: FactionId::from("gov"),
            action_kinds: vec![],
        });
        runs[2].civilian_activations.push(CivilianActivationEvent {
            tick: 9,
            segment: sid.clone(),
            favored_faction: FactionId::from("gov"),
            action_kinds: vec![],
        });
        let out = compute_civilian_activation_summaries(&runs, &s);
        let row = out.get(&sid).expect("row emitted");
        assert_eq!(row.favored_faction, FactionId::from("gov"));
    }
}
