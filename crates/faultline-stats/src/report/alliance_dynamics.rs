//! Alliance Dynamics section (Epic D round two — coalition fracture).
//!
//! Surfaces per-rule fracture rates, mean fire-tick distribution, and
//! the terminal-stance distribution across runs so analysts can see
//! which alliances actually broke under the run conditions and how
//! quickly. Pairs with the engine-side fracture phase that mutates
//! `SimulationState.diplomacy_overrides`.
//!
//! Elided when `summary.alliance_dynamics` is `None` — i.e. no
//! scenario faction declared an `alliance_fracture` block.

use std::fmt::Write;

use faultline_types::faction::Diplomacy;
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct AllianceDynamics;

impl ReportSection for AllianceDynamics {
    fn render(&self, summary: &MonteCarloSummary, _scenario: &Scenario, out: &mut String) {
        let Some(dyn_) = summary.alliance_dynamics.as_ref() else {
            return;
        };
        if dyn_.rules.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Alliance Dynamics");
        let _ = writeln!(
            out,
            "Per-rule fracture analytics. Each row reports the probability that the rule fired at all over the Monte Carlo batch, the mean tick of firing among runs that fired, and the distribution of terminal stances across runs (stances absent from the table had count zero). A rule that fired in zero runs surfaces as `0.0%` with `—` in the timing column."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "**Scope.** This section is **analytical accounting** — it reports *that* an alliance fractured under the conditions the author declared. The current engine does not consume diplomatic stance for downstream effects (combat targeting, AI decisions, victory checks); a fracture is observable in the post-run log and in this section, not in tick-level run dynamics. Treat fire rates as scenario-design diagnostics rather than live behavioral predictions. Terminal stances reflect alliance-fracture rule firings only; `EventEffect::DiplomacyChange` overrides set by scripted events are not retraced into the per-run log and so are not represented in the terminal-stance distribution."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Source | Counterparty | Rule | Description | Fire rate | Mean fire tick | Terminal stance distribution |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");

        for row in &dyn_.rules {
            let mean_fire = row
                .mean_fire_tick
                .map(|t| format!("{t:.1}"))
                .unwrap_or_else(|| "—".to_string());
            let dist = format_stance_distribution(&row.final_stance_distribution);
            let _ = writeln!(
                out,
                "| `{}` | `{}` | `{}` | {} | {:.1}% ({}/{}) | {} | {} |",
                escape_md_cell(&row.faction.0),
                escape_md_cell(&row.counterparty.0),
                escape_md_cell(&row.rule_id),
                if row.description.is_empty() {
                    "—".to_string()
                } else {
                    escape_md_cell(&row.description)
                },
                row.fire_rate * 100.0,
                row.fire_count,
                row.n_runs,
                mean_fire,
                dist,
            );
        }
        let _ = writeln!(out);
    }
}

/// Render the terminal-stance histogram inline. Stances are listed in
/// `Diplomacy`'s `Ord` order so the cell is byte-stable across runs.
/// Zero-count stances are omitted to keep the cell readable.
fn format_stance_distribution(dist: &std::collections::BTreeMap<Diplomacy, u32>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for stance in [
        Diplomacy::War,
        Diplomacy::Hostile,
        Diplomacy::Neutral,
        Diplomacy::Cooperative,
        Diplomacy::Allied,
    ] {
        if let Some(count) = dist.get(&stance)
            && *count > 0
        {
            parts.push(format!("{}={}", stance_label(stance), count));
        }
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

fn stance_label(s: Diplomacy) -> &'static str {
    match s {
        Diplomacy::War => "War",
        Diplomacy::Hostile => "Hostile",
        Diplomacy::Neutral => "Neutral",
        Diplomacy::Cooperative => "Cooperative",
        Diplomacy::Allied => "Allied",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::ids::FactionId;
    use faultline_types::stats::{AllianceDynamics as AllianceDynamicsT, FractureRuleSummary};

    use crate::report::test_support::{empty_summary, minimal_scenario};

    #[test]
    fn elides_when_no_alliance_dynamics() {
        let mut out = String::new();
        let summary = empty_summary();
        let scenario = minimal_scenario();
        AllianceDynamics.render(&summary, &scenario, &mut out);
        assert!(out.is_empty(), "should elide when None; got: {out}");
    }

    #[test]
    fn elides_when_alliance_dynamics_has_no_rules() {
        let mut out = String::new();
        let mut summary = empty_summary();
        summary.alliance_dynamics = Some(AllianceDynamicsT { rules: vec![] });
        AllianceDynamics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.is_empty(), "should elide on empty rules; got: {out}");
    }

    #[test]
    fn renders_fired_rule_with_distribution() {
        let mut summary = empty_summary();
        let mut dist = BTreeMap::new();
        dist.insert(Diplomacy::Hostile, 7);
        dist.insert(Diplomacy::Cooperative, 3);
        summary.alliance_dynamics = Some(AllianceDynamicsT {
            rules: vec![FractureRuleSummary {
                faction: FactionId::from("ally"),
                counterparty: FactionId::from("attacker"),
                rule_id: "betrayed".into(),
                description: "Public attribution".into(),
                n_runs: 10,
                fire_count: 7,
                fire_rate: 0.7,
                mean_fire_tick: Some(12.5),
                fire_ticks: vec![10, 11, 12, 12, 13, 14, 15],
                final_stance_distribution: dist,
            }],
        });
        let mut out = String::new();
        AllianceDynamics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("## Alliance Dynamics"));
        assert!(out.contains("`betrayed`"));
        assert!(out.contains("70.0%"));
        assert!(out.contains("12.5"));
        assert!(out.contains("Hostile=7"));
        assert!(out.contains("Cooperative=3"));
    }

    #[test]
    fn renders_unfired_rule_with_dash_for_mean_tick() {
        let mut summary = empty_summary();
        summary.alliance_dynamics = Some(AllianceDynamicsT {
            rules: vec![FractureRuleSummary {
                faction: FactionId::from("ally"),
                counterparty: FactionId::from("attacker"),
                rule_id: "betrayed".into(),
                description: String::new(),
                n_runs: 10,
                fire_count: 0,
                fire_rate: 0.0,
                mean_fire_tick: None,
                fire_ticks: vec![],
                final_stance_distribution: BTreeMap::new(),
            }],
        });
        let mut out = String::new();
        AllianceDynamics.render(&summary, &minimal_scenario(), &mut out);
        assert!(out.contains("0.0%"));
        // Description gets `—` placeholder when empty; the timing
        // column also uses `—` when no fires recorded.
        assert!(out.matches('—').count() >= 2);
    }
}
