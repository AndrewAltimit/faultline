//! Leadership Cadres section (decapitation + succession):
//! the declarative leadership cadre table per faction.
//!
//! Surfaces the *structure* of every faction's leadership cadre
//! (ranks, succession parameters) so analysts can read the
//! decapitation surface a scenario exposes without grepping the TOML.
//! The dynamic per-run decapitation tally is emitted only in single-
//! run mode (per-run `RunResult.final_state` carries the cumulative
//! counters); cross-run aggregation is left for a follow-up epic that
//! adds decap analytics to `MonteCarloSummary`.
//!
//! Elided when no faction declares a cadre.

use std::fmt::Write;

use faultline_types::faction::Faction;
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct LeadershipDisruption;

impl ReportSection for LeadershipDisruption {
    fn render(&self, _summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let cadre_factions: Vec<&Faction> = scenario
            .factions
            .values()
            .filter(|f| f.leadership.is_some())
            .collect();
        if cadre_factions.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Leadership Cadres");
        let _ = writeln!(
            out,
            "Declared decapitation surface per faction. A `LeadershipDecapitation` phase output advances the rank index by one and applies a morale shock; the new rank's effectiveness × `succession_floor` is written to the target's `command_effectiveness` (a multiplicative scalar combat reads alongside morale) for the recovery ramp."
        );
        let _ = writeln!(out);

        for faction in cadre_factions {
            let cadre = faction
                .leadership
                .as_ref()
                .expect("cadre_factions filtered to leadership.is_some()");
            let _ = writeln!(
                out,
                "### `{}` — {}",
                escape_md_cell(&faction.id.0),
                escape_md_cell(&faction.name)
            );
            let _ = writeln!(
                out,
                "Recovery: {} ticks, succession floor {:.2}.",
                cadre.succession_recovery_ticks, cadre.succession_floor
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "| Rank | Name | Effectiveness |");
            let _ = writeln!(out, "|---|---|---|");
            for (idx, rank) in cadre.ranks.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "| {} | `{}` ({}) | {:.2} |",
                    idx,
                    escape_md_cell(&rank.id),
                    escape_md_cell(&rank.name),
                    rank.effectiveness,
                );
            }
            let _ = writeln!(out);
        }
    }
}
