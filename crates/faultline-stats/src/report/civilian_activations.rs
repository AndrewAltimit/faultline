//! Civilian Activations section (population-segment activation).
//!
//! Surfaces per-segment activation analytics: rate of activation
//! across the run set, mean tick of activation, modal favored faction,
//! and a tally of which `CivilianAction` kinds fired in those runs.
//! Pairs with the politics phase that rolls civilian sympathies under
//! the influence of media landscape (fragmentation, social-media
//! penetration, internet availability — newly read this round) and
//! latches activation when sympathy crosses the author-set threshold.
//!
//! Elided when `summary.civilian_activation_summaries` is empty — i.e.
//! the scenario declared no `population_segments`. A scenario that
//! declared segments but produced zero activations across all runs
//! still emits the section so the analyst sees "segment X declared,
//! never tripped" rather than an unexplained absence.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct CivilianActivations;

impl ReportSection for CivilianActivations {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        if summary.civilian_activation_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Civilian Activations");
        let _ = writeln!(
            out,
            "Per-population-segment activation analytics across the Monte Carlo batch. A segment *activates* when its top faction sympathy crosses the author-set threshold; activation is one-shot per run (the engine latches the segment on first crossing). The drift toward that threshold is shaped by the segment's `volatility`, the political climate's `tension`, and three media-landscape fields — `fragmentation`, `social_media_penetration`, and `internet_availability` — which compose multiplicatively into a noise amplifier and a tension-pull dampener. *Activation rate* is the fraction of runs in which the segment ever activated; *Mean tick* is right-censored (averaged only over runs that activated)."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Segment | Name | Modal beneficiary | Runs | Activation rate | Mean tick | Top actions |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");

        for row in summary.civilian_activation_summaries.values() {
            let mean_tick = match row.mean_activation_tick {
                Some(t) => format!("{t:.1}"),
                None => "—".to_string(),
            };
            // Top three action kinds by firing count, with ties broken
            // by name (deterministic). Empty when the segment never
            // activated — render as `—` so the table shape stays.
            let mut kinds: Vec<(&String, &u32)> = row.action_kind_counts.iter().collect();
            kinds.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            let top_actions = if kinds.is_empty() {
                "—".to_string()
            } else {
                kinds
                    .iter()
                    .take(3)
                    .map(|(name, count)| format!("{name}×{count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | `{}` | {} | {:.0}% ({}/{}) | {} | {} |",
                escape_md_cell(&row.segment.0),
                escape_md_cell(&row.name),
                escape_md_cell(&row.favored_faction.0),
                row.n_runs,
                row.activation_rate * 100.0,
                row.activation_count,
                row.n_runs,
                mean_tick,
                escape_md_cell(&top_actions),
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Reading the table: a low *Activation rate* with a near-threshold modal beneficiary points to a segment authored on a knife-edge — small parameter shifts will swing the rate. *Mean tick* near `max_ticks` indicates segments that mostly activate at the end of the run; nearer zero indicates triggers that fire early and shape the rest of the trajectory. *Top actions* lists the segment's `activation_actions` weighted by how often each variant has fired across the batch — an `ArmedResistance` heavy row is a different gameplay signature than a `Protest`-only one even when the rate matches."
        );
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::{FactionId, SegmentId};
    use faultline_types::stats::SegmentActivationSummary;

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_civilian_summaries() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        CivilianActivations.render(&summary, &scenario, &mut out);
        assert!(
            out.is_empty(),
            "should elide when no civilian summaries; got: {out}"
        );
    }

    #[test]
    fn renders_per_segment_row_with_top_actions() {
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let sid = SegmentId::from("urban_pop");
        let mut action_counts = BTreeMap::new();
        action_counts.insert("Protest".into(), 5);
        action_counts.insert("NonCooperation".into(), 3);
        action_counts.insert("Sabotage".into(), 1);
        sums.insert(
            sid.clone(),
            SegmentActivationSummary {
                segment: sid,
                name: "Urban Population".into(),
                favored_faction: FactionId::from("gov"),
                n_runs: 10,
                activation_count: 6,
                activation_rate: 0.6,
                mean_activation_tick: Some(8.5),
                action_kind_counts: action_counts,
            },
        );
        summary.civilian_activation_summaries = sums;
        let mut out = String::new();
        CivilianActivations.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Civilian Activations"));
        assert!(out.contains("`urban_pop`"));
        assert!(out.contains("Urban Population"));
        assert!(out.contains("60% (6/10)"));
        assert!(out.contains("8.5"));
        // Top actions: Protest first (count 5), NonCooperation second (3),
        // Sabotage third (1).
        assert!(
            out.contains("Protest×5"),
            "top-action ranking should put Protest first: {out}"
        );
    }

    #[test]
    fn renders_unfired_segment_with_em_dash() {
        // A scenario declared a segment but no run activated it —
        // the table still emits a row with mean_tick = "—" and top
        // actions = "—" so the analyst sees the segment exists.
        let mut summary = empty_summary();
        let mut sums = BTreeMap::new();
        let sid = SegmentId::from("rural_pop");
        sums.insert(
            sid.clone(),
            SegmentActivationSummary {
                segment: sid,
                name: "Rural Communities".into(),
                favored_faction: FactionId::from("rebel"),
                n_runs: 10,
                activation_count: 0,
                activation_rate: 0.0,
                mean_activation_tick: None,
                action_kind_counts: BTreeMap::new(),
            },
        );
        summary.civilian_activation_summaries = sums;
        let mut out = String::new();
        CivilianActivations.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("`rural_pop`"));
        assert!(out.contains("0% (0/10)"));
        // The em-dash should appear in both the mean-tick and top-actions
        // columns — render the row with an explicit "never fired" signal.
        assert!(
            out.matches('—').count() >= 2,
            "em-dashes should fill both empty cells: {out}"
        );
    }
}
