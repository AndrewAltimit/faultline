//! Narrative Dynamics section (Epic D round-three item 4 — info-op
//! narrative competition).
//!
//! Surfaces per-faction information-dominance and per-narrative firing
//! / peak-strength rollups across the run set so analysts can see
//! which side won the narrative war and which messages stuck. Pairs
//! with the engine-side narrative phase that decays the persistent
//! narrative store, scores per-faction dominance, and applies sympathy
//! / tension nudges.
//!
//! Elided when `summary.narrative_dynamics` is `None` — i.e. no run
//! produced any `EventEffect::MediaEvent` firing. Authors who declare
//! `MediaEvent` effects but never trip them in the run set still see
//! no section because the rollup itself is `None`.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct NarrativeDynamics;

impl ReportSection for NarrativeDynamics {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let Some(dyn_) = summary.narrative_dynamics.as_ref() else {
            return;
        };
        if dyn_.faction_summaries.is_empty() && dyn_.narrative_summaries.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Narrative Dynamics");
        let _ = writeln!(
            out,
            "Per-narrative analytics from the info-op narrative-competition phase. Each `MediaEvent` firing reinforces a persistent narrative entry; the narrative phase scores per-faction dominance (sum of `strength × credibility` over narratives that favor each faction), nudges segment sympathy toward the leading faction, and contributes to global tension. Narratives decay each tick at a reach-discounted rate, so a narrative that's reinforced rarely fades — and one that's pushed hard sticks."
        );
        let _ = writeln!(out);

        if !dyn_.faction_summaries.is_empty() {
            let _ = writeln!(out, "### Per-faction dominance");
            let _ = writeln!(
                out,
                "`Mean dominance ticks` is the average per run of how many ticks this faction owned the strongest narrative pressure (a stream-level approximation of the engine's live attribution). `Mean peak dominance` averages the per-run peak `strength × credibility` this faction reached. `Total firings` sums the firings of narratives that favor this faction across the batch."
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "| Faction | Mean dominance ticks | Max dominance ticks | Mean peak dominance | Total firings |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|");
            for row in dyn_.faction_summaries.values() {
                let _ = writeln!(
                    out,
                    "| `{}` | {:.1} | {} | {:.3} | {} |",
                    escape_md_cell(&row.faction.0),
                    row.mean_dominance_ticks,
                    row.max_dominance_ticks,
                    row.mean_peak_information_dominance,
                    row.total_firings,
                );
            }
            let _ = writeln!(out);
        }

        if !dyn_.narrative_summaries.is_empty() {
            let _ = writeln!(out, "### Per-narrative trajectory");
            let _ = writeln!(
                out,
                "`Firing runs` counts runs in which the narrative fired at any tick. `Mean firings / firing run` averages how many reinforcement events the narrative received in those runs. `Mean peak strength` is the average peak strength reached across firing runs. `Mean first tick` reports when the narrative first appeared. `Modal favors` is the most frequent attributed faction across the batch."
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "| Narrative | Firing runs | Mean firings / firing run | Mean peak strength | Mean first tick | Modal favors |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|");
            for row in dyn_.narrative_summaries.values() {
                let favors = row
                    .modal_favors
                    .as_ref()
                    .map(|f| format!("`{}`", escape_md_cell(&f.0)))
                    .unwrap_or_else(|| "—".to_string());
                let _ = writeln!(
                    out,
                    "| `{}` | {}/{} | {:.2} | {:.3} | {:.1} | {} |",
                    escape_md_cell(&row.narrative),
                    row.firing_runs,
                    dyn_.n_runs,
                    row.mean_firings_per_run,
                    row.mean_peak_strength,
                    row.mean_first_tick,
                    favors,
                );
            }
            let _ = writeln!(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::{
        FactionNarrativeSummary, NarrativeDynamics as NarrativeDynT, NarrativeKeySummary,
    };

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_dynamics() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        NarrativeDynamics.render(&summary, &scenario, &mut out);
        assert!(out.is_empty(), "should elide when None; got: {out}");
    }

    #[test]
    fn elides_when_dynamics_is_empty() {
        let mut out = String::new();
        let mut summary = empty_summary();
        summary.narrative_dynamics = Some(NarrativeDynT {
            n_runs: 10,
            faction_summaries: BTreeMap::new(),
            narrative_summaries: BTreeMap::new(),
        });
        NarrativeDynamics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.is_empty(), "empty dynamics should elide");
    }

    #[test]
    fn renders_with_data() {
        let alpha = FactionId::from("alpha");
        let mut faction_summaries = BTreeMap::new();
        faction_summaries.insert(
            alpha.clone(),
            FactionNarrativeSummary {
                faction: alpha.clone(),
                mean_dominance_ticks: 7.5,
                max_dominance_ticks: 12,
                mean_peak_information_dominance: 0.42,
                total_firings: 20,
            },
        );
        let mut narrative_summaries = BTreeMap::new();
        narrative_summaries.insert(
            "headline".to_string(),
            NarrativeKeySummary {
                narrative: "headline".into(),
                firing_runs: 8,
                mean_firings_per_run: 2.5,
                mean_peak_strength: 0.6,
                mean_first_tick: 5.5,
                modal_favors: Some(alpha),
            },
        );
        let mut summary = empty_summary();
        summary.narrative_dynamics = Some(NarrativeDynT {
            n_runs: 10,
            faction_summaries,
            narrative_summaries,
        });
        let mut out = String::new();
        NarrativeDynamics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Narrative Dynamics"));
        assert!(out.contains("`alpha`"));
        assert!(out.contains("`headline`"));
        assert!(out.contains("8/10"));
    }
}
