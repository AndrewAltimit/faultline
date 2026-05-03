//! Cross-run narrative-competition analytics (Epic D round-three item 4).
//!
//! Pure post-processing of [`RunResult.narrative_events`]: walks every
//! run's narrative log to count firings, capture peak strengths, and
//! identify the modal `favors` faction per narrative key. Per-faction
//! dominance ticks aren't carried on `RunResult` (they're an aggregate
//! of how many ticks the narrative phase attributed
//! information-dominance to each faction over the run); this module
//! re-derives them from the event log + `final_state.tension` is *not*
//! used — instead, the per-run dominance counter lives on
//! `SimulationState.narrative_dominance_ticks`. To keep `RunResult`
//! schema-stable for now, we approximate dominance ticks here by
//! counting events where the favored faction was the leader (highest
//! `strength_after × credibility` among events visible up to that
//! tick). That's an *approximation*; the engine's actual per-tick
//! attribution uses live narrative state. For the report's purposes,
//! the counter remains directionally correct.

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

        // Derive per-faction "dominance ticks" proxy: the number of
        // events for which this faction had the highest cumulative
        // (strength × credibility) in the event stream up to that tick.
        // It's a stream-level approximation, not the engine's live
        // attribution, but produces the right ranking for the report.
        let mut leader_counter: BTreeMap<FactionId, f64> = BTreeMap::new();
        let mut tick_dominance: BTreeMap<FactionId, u32> = BTreeMap::new();
        let mut peak_dominance: BTreeMap<FactionId, f64> = BTreeMap::new();
        for ev in &run.narrative_events {
            if let Some(fid) = &ev.favors {
                let pressure = ev.strength_after * ev.credibility;
                let acc = leader_counter.entry(fid.clone()).or_insert(0.0);
                if pressure > *acc {
                    *acc = pressure;
                }
                let leader = leader_counter
                    .iter()
                    .max_by(|a, b| {
                        a.1.partial_cmp(b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| b.0.cmp(a.0))
                    })
                    .map(|(fid, _)| fid.clone());
                if let Some(lead) = leader {
                    *tick_dominance.entry(lead.clone()).or_insert(0) += 1;
                    let p = peak_dominance.entry(lead).or_insert(0.0);
                    if pressure > *p {
                        *p = pressure.min(1.0);
                    }
                }
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

        // Roll into per-faction cross-run aggregator.
        for (fid, ticks) in tick_dominance {
            let agg = per_faction
                .entry(fid.clone())
                .or_insert_with(|| PerFactionAgg {
                    dominance_ticks_sum: 0,
                    max_dominance_ticks: 0,
                    peak_dominance_sum: 0.0,
                    peak_dominance_runs: 0,
                    total_firings: 0,
                });
            agg.dominance_ticks_sum += u64::from(ticks);
            if ticks > agg.max_dominance_ticks {
                agg.max_dominance_ticks = ticks;
            }
            if let Some(peak) = peak_dominance.get(&fid) {
                agg.peak_dominance_sum += *peak;
                agg.peak_dominance_runs += 1;
            }
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
    }
}
