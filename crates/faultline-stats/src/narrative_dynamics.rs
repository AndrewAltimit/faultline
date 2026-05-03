//! Cross-run narrative-competition analytics (Epic D round-three item 4).
//!
//! Pure post-processing of [`RunResult.narrative_events`] plus the
//! engine's ground-truth per-faction dominance counters carried on
//! [`RunResult.narrative_dominance_ticks`] /
//! [`RunResult.narrative_peak_dominance`]. The engine's narrative phase
//! attributes information-dominance after applying decay each tick, so
//! the live counter reflects the post-decay leader. This module reads
//! those counters directly rather than re-deriving them from the event
//! log; the prior event-log approximation over-attributed to early
//! reinforcers because their stale pressure never decayed in the
//! stream-level scan.

use std::collections::BTreeMap;

use faultline_types::ids::FactionId;
use faultline_types::stats::{
    FactionNarrativeSummary, NarrativeDynamics, NarrativeKeySummary, RunResult,
};

/// Compute the cross-run narrative-dynamics summary.
///
/// Returns `None` when no run produced any narrative events. The report
/// renderer treats that as "scenario does not engage the mechanic" and
/// emits no section.
pub fn compute_narrative_dynamics(runs: &[RunResult]) -> Option<NarrativeDynamics> {
    if runs.is_empty() {
        return None;
    }
    let any_event = runs.iter().any(|r| !r.narrative_events.is_empty());
    if !any_event {
        return None;
    }

    // Per-narrative aggregation.
    struct PerNarrativeAgg {
        firing_runs: u32,
        firings_total: u32,
        peak_strength_sum: f64,
        first_tick_sum: u64,
        first_tick_runs: u32,
        favor_counts: BTreeMap<Option<FactionId>, u32>,
    }
    let mut per_narrative: BTreeMap<String, PerNarrativeAgg> = BTreeMap::new();

    // Per-faction aggregation.
    struct PerFactionAgg {
        dominance_ticks_sum: u64,
        max_dominance_ticks: u32,
        peak_dominance_sum: f64,
        peak_dominance_runs: u32,
        total_firings: u32,
    }
    let mut per_faction: BTreeMap<FactionId, PerFactionAgg> = BTreeMap::new();

    for run in runs {
        // Per-run per-narrative tracking.
        struct RunNarr {
            firings: u32,
            peak_strength: f64,
            first_tick: u32,
            modal_favors: Option<FactionId>,
        }
        let mut run_narrs: BTreeMap<String, RunNarr> = BTreeMap::new();

        // Per-run per-faction firing count and dominance proxy.
        let mut run_faction_firings: BTreeMap<FactionId, u32> = BTreeMap::new();

        for ev in &run.narrative_events {
            let entry = run_narrs.entry(ev.narrative.clone()).or_insert(RunNarr {
                firings: 0,
                peak_strength: 0.0,
                first_tick: ev.tick,
                modal_favors: ev.favors.clone(),
            });
            entry.firings += 1;
            if ev.strength_after > entry.peak_strength {
                entry.peak_strength = ev.strength_after;
            }
            if let Some(fid) = &ev.favors {
                *run_faction_firings.entry(fid.clone()).or_insert(0) += 1;
            }
        }

        // Roll into per-narrative cross-run aggregator.
        for (narr_key, rn) in run_narrs {
            let agg = per_narrative
                .entry(narr_key.clone())
                .or_insert_with(|| PerNarrativeAgg {
                    firing_runs: 0,
                    firings_total: 0,
                    peak_strength_sum: 0.0,
                    first_tick_sum: 0,
                    first_tick_runs: 0,
                    favor_counts: BTreeMap::new(),
                });
            agg.firing_runs += 1;
            agg.firings_total += rn.firings;
            agg.peak_strength_sum += rn.peak_strength;
            agg.first_tick_sum += u64::from(rn.first_tick);
            agg.first_tick_runs += 1;
            *agg.favor_counts.entry(rn.modal_favors).or_insert(0) += 1;
        }

        // Per-faction dominance: consume the engine's ground-truth
        // counters directly. The engine's narrative phase applies decay
        // before attribution each tick, so this captures the post-decay
        // leader (events from earlier ticks whose narratives have since
        // decayed below threshold are correctly excluded).
        for (fid, ticks) in &run.narrative_dominance_ticks {
            let agg = per_faction
                .entry(fid.clone())
                .or_insert_with(|| PerFactionAgg {
                    dominance_ticks_sum: 0,
                    max_dominance_ticks: 0,
                    peak_dominance_sum: 0.0,
                    peak_dominance_runs: 0,
                    total_firings: 0,
                });
            agg.dominance_ticks_sum += u64::from(*ticks);
            if *ticks > agg.max_dominance_ticks {
                agg.max_dominance_ticks = *ticks;
            }
        }
        for (fid, peak) in &run.narrative_peak_dominance {
            let agg = per_faction
                .entry(fid.clone())
                .or_insert_with(|| PerFactionAgg {
                    dominance_ticks_sum: 0,
                    max_dominance_ticks: 0,
                    peak_dominance_sum: 0.0,
                    peak_dominance_runs: 0,
                    total_firings: 0,
                });
            agg.peak_dominance_sum += peak.clamp(0.0, 1.0);
            agg.peak_dominance_runs += 1;
        }
        for (fid, count) in run_faction_firings {
            let agg = per_faction.entry(fid).or_insert_with(|| PerFactionAgg {
                dominance_ticks_sum: 0,
                max_dominance_ticks: 0,
                peak_dominance_sum: 0.0,
                peak_dominance_runs: 0,
                total_firings: 0,
            });
            agg.total_firings += count;
        }
    }

    let n_runs = runs.len() as u32;
    let n_runs_f = f64::from(n_runs.max(1));

    let faction_summaries: BTreeMap<FactionId, FactionNarrativeSummary> = per_faction
        .into_iter()
        .map(|(fid, agg)| {
            let mean_dominance_ticks = (agg.dominance_ticks_sum as f64) / n_runs_f;
            let mean_peak = if agg.peak_dominance_runs > 0 {
                agg.peak_dominance_sum / f64::from(agg.peak_dominance_runs)
            } else {
                0.0
            };
            (
                fid.clone(),
                FactionNarrativeSummary {
                    faction: fid,
                    mean_dominance_ticks,
                    max_dominance_ticks: agg.max_dominance_ticks,
                    mean_peak_information_dominance: mean_peak,
                    total_firings: agg.total_firings,
                },
            )
        })
        .collect();

    let narrative_summaries: BTreeMap<String, NarrativeKeySummary> = per_narrative
        .into_iter()
        .map(|(narr_key, agg)| {
            let firing_runs_f = f64::from(agg.firing_runs.max(1));
            let mean_firings_per_run = f64::from(agg.firings_total) / firing_runs_f;
            let mean_peak_strength = agg.peak_strength_sum / firing_runs_f;
            let mean_first_tick = if agg.first_tick_runs > 0 {
                (agg.first_tick_sum as f64) / f64::from(agg.first_tick_runs)
            } else {
                0.0
            };
            // Modal favors: max-count entry in favor_counts. Ties broken
            // toward `Some(_)` (we want to attribute to a faction when
            // possible) and then lexicographically by FactionId.
            let modal_favors = agg
                .favor_counts
                .iter()
                .max_by(|a, b| {
                    a.1.cmp(b.1).then_with(|| match (a.0, b.0) {
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(x), Some(y)) => x.cmp(y),
                        (None, None) => std::cmp::Ordering::Equal,
                    })
                })
                .and_then(|(k, _)| k.clone());
            (
                narr_key.clone(),
                NarrativeKeySummary {
                    narrative: narr_key,
                    firing_runs: agg.firing_runs,
                    mean_firings_per_run,
                    mean_peak_strength,
                    mean_first_tick,
                    modal_favors,
                },
            )
        })
        .collect();

    Some(NarrativeDynamics {
        n_runs,
        faction_summaries,
        narrative_summaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::{NarrativeEvent, Outcome, RunResult, StateSnapshot};

    fn empty_run() -> RunResult {
        RunResult {
            run_index: 0,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 0,
            final_state: StateSnapshot {
                tick: 0,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: vec![],
            },
            snapshots: vec![],
            event_log: vec![],
            campaign_reports: BTreeMap::new(),
            defender_queue_reports: vec![],
            network_reports: BTreeMap::new(),
            fracture_events: vec![],
            supply_pressure_reports: BTreeMap::new(),
            civilian_activations: vec![],
            tech_costs: BTreeMap::new(),
            narrative_events: vec![],
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: BTreeMap::new(),
        }
    }

    #[test]
    fn empty_runs_produce_none() {
        assert!(compute_narrative_dynamics(&[]).is_none());
        assert!(compute_narrative_dynamics(&[empty_run()]).is_none());
    }

    #[test]
    fn single_narrative_aggregates_correctly() {
        let mut run = empty_run();
        let alpha = FactionId::from("alpha");
        run.narrative_events.push(NarrativeEvent {
            tick: 5,
            narrative: "headline".into(),
            favors: Some(alpha.clone()),
            credibility: 0.8,
            reach: 0.6,
            strength_after: 0.4,
            was_new: true,
        });
        run.narrative_events.push(NarrativeEvent {
            tick: 7,
            narrative: "headline".into(),
            favors: Some(alpha.clone()),
            credibility: 0.8,
            reach: 0.6,
            strength_after: 0.7,
            was_new: false,
        });
        // Ground-truth dominance from the engine (would be set by
        // narrative_phase): alpha led for 4 ticks with peak 0.56.
        run.narrative_dominance_ticks.insert(alpha.clone(), 4);
        run.narrative_peak_dominance.insert(alpha.clone(), 0.56);

        let summary = compute_narrative_dynamics(&[run]).expect("non-empty");
        assert_eq!(summary.n_runs, 1);
        let key_summary = summary
            .narrative_summaries
            .get("headline")
            .expect("present");
        assert_eq!(key_summary.firing_runs, 1);
        assert!((key_summary.mean_firings_per_run - 2.0).abs() < f64::EPSILON);
        assert!((key_summary.mean_peak_strength - 0.7).abs() < f64::EPSILON);
        assert!((key_summary.mean_first_tick - 5.0).abs() < f64::EPSILON);
        assert_eq!(key_summary.modal_favors, Some(alpha.clone()));

        let faction_summary = summary
            .faction_summaries
            .get(&alpha)
            .expect("alpha present");
        assert_eq!(faction_summary.total_firings, 2);
        assert!((faction_summary.mean_dominance_ticks - 4.0).abs() < f64::EPSILON);
        assert_eq!(faction_summary.max_dominance_ticks, 4);
        assert!((faction_summary.mean_peak_information_dominance - 0.56).abs() < f64::EPSILON);
    }
}
