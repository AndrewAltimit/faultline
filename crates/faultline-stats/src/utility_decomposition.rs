//! Cross-run utility-decomposition aggregator (Epic J round-one).
//!
//! Pure post-processor over [`RunResult::utility_decisions`] —
//! produces per-faction `UtilityDecompositionSummary` rows for the
//! Monte Carlo report's `## Utility Decomposition` section.
//!
//! Determinism: pure function over `(runs, scenario)` — no RNG, no
//! allocation in the hot path beyond the output map. `BTreeMap`-ordered
//! iteration. Same `(runs, scenario)` ⇒ bit-identical output.

use std::collections::BTreeMap;

use faultline_types::faction::UtilityTerm;
use faultline_types::ids::FactionId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{RunResult, UtilityDecompositionSummary};

/// Compute per-faction utility-decomposition summaries across runs.
///
/// Empty result means either no faction declares `Faction.utility` or
/// no contribution ever fired. The report section gates on the empty-
/// map signal so legacy scenarios pay zero report-rendering cost.
///
/// **Per-faction contract:** any faction declaring `[utility]` is
/// represented in the output, even if zero runs produced a
/// contributing decision (the row will have all means at `0.0` and
/// `runs_with_contribution = 0`). This makes "I declared a profile
/// but it never contributed" visible to the analyst rather than
/// hidden — the same contract `Faction.alliance_fracture` rules
/// honor in `compute_alliance_dynamics`.
pub fn compute_utility_decompositions(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<FactionId, UtilityDecompositionSummary> {
    let mut out: BTreeMap<FactionId, UtilityDecompositionSummary> = BTreeMap::new();

    // Seed entries for every faction that *declares* a profile, even
    // if no run contributed. Captures the "declared but never fired"
    // case explicitly so the analyst sees an explanation rather than
    // a silent omission.
    for (fid, faction) in &scenario.factions {
        let Some(profile) = &faction.utility else {
            continue;
        };
        let mut row = UtilityDecompositionSummary {
            faction: fid.clone(),
            runs_with_contribution: 0,
            mean_tick_count: 0.0,
            mean_decision_count: 0.0,
            mean_contributions_per_decision: BTreeMap::new(),
            trigger_fire_rates: BTreeMap::new(),
        };
        // Pre-populate trigger ids with rate 0 so the renderer shows
        // declared-but-never-fired explicitly.
        for trigger in &profile.triggers {
            row.trigger_fire_rates.insert(trigger.id.clone(), 0.0);
        }
        // Pre-populate every authored term with mean 0 so the renderer
        // surfaces "term declared, no contribution" the same way.
        for term in profile.terms.keys() {
            row.mean_contributions_per_decision
                .insert(term.as_key().to_string(), 0.0);
        }
        out.insert(fid.clone(), row);
    }

    if out.is_empty() {
        // No faction declares `[utility]`. Skip the per-run loop —
        // any utility_decisions on RunResult would be from a stale
        // input (the engine only writes when a profile contributes).
        return out;
    }

    // Aggregate per-run reports. Two passes per run-set:
    // - tick_total (cumulative `tick_count` across runs) is the
    //   denominator for trigger fire rates and per-term means.
    // - decision_total (cumulative `decision_count`) is the
    //   denominator for per-term means.
    let mut tick_totals: BTreeMap<FactionId, u64> = BTreeMap::new();
    let mut decision_totals: BTreeMap<FactionId, u64> = BTreeMap::new();
    let mut term_sums_total: BTreeMap<FactionId, BTreeMap<String, f64>> = BTreeMap::new();
    let mut trigger_sums_total: BTreeMap<FactionId, BTreeMap<String, u64>> = BTreeMap::new();
    let mut runs_with_contribution: BTreeMap<FactionId, u32> = BTreeMap::new();

    for run in runs {
        for (fid, report) in &run.utility_decisions {
            // Filter to only declared profiles — in case a stale
            // RunResult carries entries for factions whose profile was
            // removed mid-batch (unlikely in practice but cheap to
            // guard).
            if !out.contains_key(fid) {
                continue;
            }
            *tick_totals.entry(fid.clone()).or_insert(0) += u64::from(report.tick_count);
            *decision_totals.entry(fid.clone()).or_insert(0) += report.decision_count;
            *runs_with_contribution.entry(fid.clone()).or_insert(0) += 1;
            let entry = term_sums_total.entry(fid.clone()).or_default();
            for (term_key, sum) in &report.term_sums {
                *entry.entry(term_key.clone()).or_insert(0.0) += sum;
            }
            let trig_entry = trigger_sums_total.entry(fid.clone()).or_default();
            for (trigger_id, count) in &report.trigger_fires {
                *trig_entry.entry(trigger_id.clone()).or_insert(0) += u64::from(*count);
            }
        }
    }

    let runs_count = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");
    for (fid, row) in out.iter_mut() {
        let tick_total = tick_totals.get(fid).copied().unwrap_or(0);
        let decision_total = decision_totals.get(fid).copied().unwrap_or(0);
        row.runs_with_contribution = runs_with_contribution.get(fid).copied().unwrap_or(0);
        row.mean_tick_count = if runs_count == 0 {
            0.0
        } else {
            tick_total as f64 / f64::from(runs_count)
        };
        row.mean_decision_count = if runs_count == 0 {
            0.0
        } else {
            decision_total as f64 / f64::from(runs_count)
        };
        if let Some(sums) = term_sums_total.get(fid)
            && decision_total > 0
        {
            // Update the pre-seeded entries; new terms (e.g. a term
            // that only appeared post-seeding because the engine
            // produced it from an action mapping despite the author
            // not declaring its base weight) merge in too.
            for (term_key, sum) in sums {
                let entry = row
                    .mean_contributions_per_decision
                    .entry(term_key.clone())
                    .or_insert(0.0);
                *entry = sum / decision_total as f64;
            }
        }
        if let Some(trigs) = trigger_sums_total.get(fid)
            && tick_total > 0
        {
            for (trigger_id, fires) in trigs {
                let entry = row
                    .trigger_fire_rates
                    .entry(trigger_id.clone())
                    .or_insert(0.0);
                *entry = (*fires as f64) / (tick_total as f64);
            }
        }
    }

    out
}

/// Reorder the per-term map of a summary into [`UtilityTerm`]
/// declaration order for renderer-side use. The map's natural
/// `BTreeMap`-by-string ordering is alphabetic, which puts
/// `attribution_risk` before `casualties_inflicted`; the analyst
/// reading the report wants the canonical declaration order
/// (`Control` first, then `CasualtiesSelf`, etc.).
pub fn ordered_term_keys() -> Vec<&'static str> {
    UtilityTerm::all().iter().map(|t| t.as_key()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::faction::{
        AdaptiveCondition, AdaptiveTrigger, Faction, FactionType, FactionUtility, UtilityTerm,
    };
    use faultline_types::stats::UtilityDecisionReport;

    fn make_scenario_with_alpha_profile() -> Scenario {
        let alpha = FactionId::from("alpha");
        let mut s = Scenario::default();
        let mut terms = BTreeMap::new();
        terms.insert(UtilityTerm::Control, 1.0);
        terms.insert(UtilityTerm::CasualtiesSelf, 0.5);
        let mut adj = BTreeMap::new();
        adj.insert(UtilityTerm::CasualtiesSelf, 2.0);
        s.factions.insert(
            alpha.clone(),
            Faction {
                id: alpha.clone(),
                faction_type: FactionType::Civilian,
                utility: Some(FactionUtility {
                    terms,
                    triggers: vec![AdaptiveTrigger {
                        id: "panic".into(),
                        description: "".into(),
                        condition: AdaptiveCondition::MoraleBelow { threshold: 0.3 },
                        adjustments: adj,
                    }],
                    time_horizon_ticks: None,
                }),
                ..Default::default()
            },
        );
        s
    }

    fn make_run_with_utility_data(
        run_index: u32,
        alpha_tick_count: u32,
        alpha_decision_count: u64,
        alpha_term_sum_control: f64,
    ) -> RunResult {
        let alpha = FactionId::from("alpha");
        let mut term_sums = BTreeMap::new();
        term_sums.insert("control".into(), alpha_term_sum_control);
        let mut trigger_fires = BTreeMap::new();
        trigger_fires.insert("panic".into(), 5_u32);
        let report = UtilityDecisionReport {
            faction: alpha.clone(),
            tick_count: alpha_tick_count,
            decision_count: alpha_decision_count,
            term_sums,
            trigger_fires,
        };
        let mut udecisions = BTreeMap::new();
        udecisions.insert(alpha, report);
        empty_run(run_index, udecisions)
    }

    fn empty_run(
        run_index: u32,
        udecisions: BTreeMap<FactionId, UtilityDecisionReport>,
    ) -> RunResult {
        RunResult {
            run_index,
            seed: 1,
            outcome: faultline_types::stats::Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 1,
            final_state: faultline_types::stats::StateSnapshot {
                tick: 1,
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
            utility_decisions: udecisions,
        }
    }

    #[test]
    fn empty_input_returns_empty_when_no_profiles_declared() {
        let s = Scenario::default();
        let summaries = compute_utility_decompositions(&[], &s);
        assert!(summaries.is_empty());
    }

    #[test]
    fn faction_with_profile_appears_with_zeros_when_no_runs() {
        let s = make_scenario_with_alpha_profile();
        let summaries = compute_utility_decompositions(&[], &s);
        let alpha_sum = summaries.get(&FactionId::from("alpha")).expect("alpha row");
        assert_eq!(alpha_sum.runs_with_contribution, 0);
        assert_eq!(alpha_sum.mean_tick_count, 0.0);
        // Pre-seeded entries — author declared `panic` trigger and
        // Control/CasualtiesSelf terms.
        assert_eq!(
            alpha_sum.trigger_fire_rates.get("panic").copied(),
            Some(0.0)
        );
        assert_eq!(
            alpha_sum
                .mean_contributions_per_decision
                .get("control")
                .copied(),
            Some(0.0)
        );
    }

    #[test]
    fn aggregator_means_are_run_averaged() {
        // Two runs, alpha contributed in both. Run 0: tick_count=10,
        // decision_count=20, control_sum=5.0. Run 1: tick_count=20,
        // decision_count=40, control_sum=10.0. Across runs:
        //  - mean_tick_count = (10+20)/2 = 15
        //  - mean_decision_count = (20+40)/2 = 30
        //  - mean_control = (5+10) / (20+40) = 0.25
        let s = make_scenario_with_alpha_profile();
        let runs = vec![
            make_run_with_utility_data(0, 10, 20, 5.0),
            make_run_with_utility_data(1, 20, 40, 10.0),
        ];
        let summaries = compute_utility_decompositions(&runs, &s);
        let alpha = summaries.get(&FactionId::from("alpha")).expect("alpha row");
        assert!((alpha.mean_tick_count - 15.0).abs() < 1e-12);
        assert!((alpha.mean_decision_count - 30.0).abs() < 1e-12);
        assert!(
            (alpha
                .mean_contributions_per_decision
                .get("control")
                .copied()
                .unwrap_or_default()
                - 0.25)
                .abs()
                < 1e-12
        );
        assert_eq!(alpha.runs_with_contribution, 2);
    }

    #[test]
    fn trigger_fire_rate_is_fires_over_total_ticks() {
        // Two runs, panic fired 5 times each, tick_count=10/20 across
        // runs. Rate = (5+5) / (10+20) = 10/30 = 0.333...
        let s = make_scenario_with_alpha_profile();
        let runs = vec![
            make_run_with_utility_data(0, 10, 20, 5.0),
            make_run_with_utility_data(1, 20, 40, 10.0),
        ];
        let summaries = compute_utility_decompositions(&runs, &s);
        let alpha = summaries.get(&FactionId::from("alpha")).expect("alpha row");
        let rate = alpha
            .trigger_fire_rates
            .get("panic")
            .copied()
            .expect("panic trigger seeded");
        assert!((rate - (10.0 / 30.0)).abs() < 1e-12);
    }

    #[test]
    fn compute_is_pure_function() {
        // Same inputs should produce bit-identical output.
        let s = make_scenario_with_alpha_profile();
        let runs = vec![
            make_run_with_utility_data(0, 10, 20, 5.0),
            make_run_with_utility_data(1, 20, 40, 10.0),
        ];
        let a = compute_utility_decompositions(&runs, &s);
        let b = compute_utility_decompositions(&runs, &s);
        // BTreeMap iteration is deterministic — convert to JSON to
        // catch any silent reordering.
        let a_json = serde_json::to_string(&a).expect("ser a");
        let b_json = serde_json::to_string(&b).expect("ser b");
        assert_eq!(a_json, b_json);
    }
}
