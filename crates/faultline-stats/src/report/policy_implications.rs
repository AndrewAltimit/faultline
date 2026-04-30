//! Policy Implications section (Epic B): surfaces declarative
//! `defender_options` on events and `escalation_rules` on factions, so
//! analysts see which counterfactuals the scenario author has
//! pre-enumerated alongside the doctrine / ROE contract each faction
//! operates under.
//!
//! Elided when no events carry `defender_options` and no factions
//! carry `escalation_rules` — empty section noise on legacy scenarios.

use std::fmt::Write;

use faultline_types::events::{DefenderOption, EventDefinition};
use faultline_types::faction::{EscalationRules, Faction};
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct PolicyImplications;

impl ReportSection for PolicyImplications {
    fn render(&self, _summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        let option_events: Vec<&EventDefinition> = scenario
            .events
            .values()
            .filter(|e| !e.defender_options.is_empty())
            .collect();
        let ruled_factions: Vec<&Faction> = scenario
            .factions
            .values()
            .filter(|f| f.escalation_rules.is_some())
            .collect();

        if option_events.is_empty() && ruled_factions.is_empty() {
            return;
        }

        let _ = writeln!(out, "## Policy Implications");
        let _ = writeln!(
            out,
            "Declarative counterfactual hooks from the scenario — alternative defender responses and standing escalation doctrine. These are surfaced so analysts can see which branches the author has pre-enumerated and which faction decisions implicitly require crossing a doctrinal red line. Nothing here is consumed by the Monte Carlo roll; use `--counterfactual` to actually evaluate a branch."
        );
        let _ = writeln!(out);

        if !option_events.is_empty() {
            let _ = writeln!(out, "### Defender Options on Events");
            for event in option_events {
                let _ = writeln!(out, "- **`{}` — {}**", event.id, event.name);
                if !event.description.trim().is_empty() {
                    let _ = writeln!(out, "  - _{}_", event.description.trim());
                }
                for opt in &event.defender_options {
                    render_defender_option(out, opt);
                }
            }
            let _ = writeln!(out);
        }

        if !ruled_factions.is_empty() {
            let _ = writeln!(out, "### Escalation Rules");
            for faction in ruled_factions {
                if let Some(rules) = &faction.escalation_rules {
                    render_escalation_rules(out, faction, rules);
                }
            }
            let _ = writeln!(out);
        }
    }
}

fn render_defender_option(out: &mut String, opt: &DefenderOption) {
    let cost = if opt.preparedness_cost > 0.0 {
        format!(" · preparedness cost **${:.0}**", opt.preparedness_cost)
    } else {
        String::new()
    };
    let _ = writeln!(out, "  - `option:{}` **{}**{}", opt.key, opt.name, cost);
    if !opt.description.trim().is_empty() {
        let _ = writeln!(out, "    - _{}_", opt.description.trim());
    }
    if opt.modifier_effects.is_empty() {
        let _ = writeln!(out, "    - Effect: cancels the event's default effects.");
    } else {
        let _ = writeln!(
            out,
            "    - Effect: replaces default with {} modifier(s).",
            opt.modifier_effects.len()
        );
    }
}

fn render_escalation_rules(out: &mut String, faction: &Faction, rules: &EscalationRules) {
    let _ = writeln!(out, "- **`{}` — {}**", faction.id, faction.name);
    if !rules.posture.trim().is_empty() {
        let _ = writeln!(out, "  - _{}_", rules.posture.trim());
    }
    if let Some(floor) = rules.de_escalation_floor {
        let _ = writeln!(
            out,
            "  - De-escalation floor: faction will not voluntarily fall below tension **{:.2}** without an external trigger.",
            floor
        );
    }
    if !rules.ladder.is_empty() {
        let _ = writeln!(out, "  - Ladder (low → high escalation):");
        for rung in &rules.ladder {
            let trigger = rung
                .trigger_tension
                .map(|t| format!(" @ tension ≥ **{:.2}**", t))
                .unwrap_or_default();
            let _ = writeln!(out, "    - `{}` **{}**{}", rung.id, rung.name, trigger);
            if !rung.description.trim().is_empty() {
                let _ = writeln!(out, "      - _{}_", rung.description.trim());
            }
            if !rung.permitted_actions.is_empty() {
                let _ = writeln!(
                    out,
                    "      - Permitted: {}",
                    rung.permitted_actions.join("; ")
                );
            }
            if !rung.prohibited_actions.is_empty() {
                let _ = writeln!(
                    out,
                    "      - Prohibited: {}",
                    rung.prohibited_actions.join("; ")
                );
            }
        }
    }
}
