//! Structured Markdown report generation from Monte Carlo summaries.
//!
//! Produces a structured document suitable for pasting into research
//! write-ups. Consumes only types from
//! `faultline_types` so it works against any summary source (native CLI,
//! WASM, or stored JSON).

use std::fmt::Write;

use faultline_types::campaign::{CampaignPhase, KillChain, ObservableDiscipline, WarningIndicator};
use faultline_types::events::{DefenderOption, EventDefinition};
use faultline_types::faction::{EscalationRules, Faction};
use faultline_types::ids::PhaseId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{
    CampaignSummary, ConfidenceInterval, ConfidenceLevel, CorrelationMatrix, DistributionStats,
    FeasibilityConfidence, FeasibilityRow, MetricType, MonteCarloSummary, ParetoFrontier,
};

use crate::counterfactual::{ComparisonReport, ParamOverride};
use crate::search::{SearchMethod, SearchResult, SearchTrial};
use faultline_types::strategy_space::SearchObjective;

/// Render a Markdown feasibility / cost asymmetry / seam analysis
/// report for a single Monte Carlo run.
pub fn render_markdown(summary: &MonteCarloSummary, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Analysis Report");
    let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
    let _ = writeln!(out, "_{}_", scenario.meta.description.trim());
    let _ = writeln!(out);
    if let Some(conf) = &scenario.meta.confidence {
        // Banner is distinct from the Wilson CIs — it flags *parameter*
        // defensibility, not sampling precision. Symbol is intentionally
        // plain ASCII so reports render identically in stripped terminals.
        let (glyph, label) = match conf {
            ConfidenceLevel::High => ("[H]", "publication-ready rigor"),
            ConfidenceLevel::Medium => ("[M]", "working draft"),
            ConfidenceLevel::Low => ("[L]", "conceptual sketch"),
        };
        let _ = writeln!(
            out,
            "> **Scenario confidence: {} {} — _{}_.** See Methodology for how this interacts with the Wilson CIs below.",
            glyph,
            confidence_word(conf),
            label
        );
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "- **Runs:** {}", summary.total_runs);
    let _ = writeln!(
        out,
        "- **Average duration (ticks):** {:.1}",
        summary.average_duration
    );
    let _ = writeln!(out);

    if !summary.win_rates.is_empty() {
        let _ = writeln!(out, "## Win Rates");
        let has_cis = !summary.win_rate_cis.is_empty();
        if has_cis {
            let _ = writeln!(out, "| Faction | Probability | 95% CI |");
            let _ = writeln!(out, "|---|---|---|");
        } else {
            let _ = writeln!(out, "| Faction | Probability |");
            let _ = writeln!(out, "|---|---|");
        }
        for (fid, rate) in &summary.win_rates {
            let ci_cell = summary.win_rate_cis.get(fid);
            // `has_cis` fixes the table column count for the whole
            // section. If any individual faction is missing a CI entry,
            // emit a placeholder rather than a short row — otherwise
            // the Markdown table becomes malformed. The two maps are
            // built from the same iterator in the runner today, so this
            // branch is defensive against divergence if `MonteCarloSummary`
            // is constructed by other callers.
            if has_cis {
                let cell = ci_cell.map(fmt_ci_pct).unwrap_or_else(|| "—".to_string());
                let _ = writeln!(out, "| `{}` | {:.1}% | {} |", fid, rate * 100.0, cell);
            } else {
                debug_assert!(
                    ci_cell.is_none(),
                    "win_rate_cis populated but has_cis is false for `{fid}`",
                );
                let _ = writeln!(out, "| `{}` | {:.1}% |", fid, rate * 100.0);
            }
        }
        if has_cis {
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_Win-rate CIs use the Wilson score interval (95%, z ≈ 1.960)._"
            );
        }
        let _ = writeln!(out);
    }

    render_continuous_metrics(&mut out, summary);

    if !summary.feasibility_matrix.is_empty() {
        let _ = writeln!(out, "## Feasibility Matrix");
        let _ = writeln!(
            out,
            "| Chain | Tech Readiness | Op Complexity | Detection | Success | Severity | Attribution Diff | Cost Ratio |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
        for row in &summary.feasibility_matrix {
            let _ = writeln!(
                out,
                "| **{}** | {} | {} | {} | {} | {} | {:.2} | **{:.0}×** |",
                escape_md_cell(&row.chain_name),
                fmt_cell(
                    row.technology_readiness,
                    row.confidence.technology_readiness.clone(),
                    None,
                ),
                fmt_cell(
                    row.operational_complexity,
                    row.confidence.operational_complexity.clone(),
                    None,
                ),
                fmt_cell(
                    row.detection_probability,
                    row.confidence.detection_probability.clone(),
                    row.ci_95.detection_probability.as_ref(),
                ),
                fmt_cell(
                    row.success_probability,
                    row.confidence.success_probability.clone(),
                    row.ci_95.success_probability.as_ref(),
                ),
                fmt_cell(
                    row.consequence_severity,
                    row.confidence.consequence_severity.clone(),
                    row.ci_95.consequence_severity.as_ref(),
                ),
                row.attribution_difficulty,
                row.cost_asymmetry_ratio,
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Cell format: `value [confidence]` or `value [confidence] (lo–hi)` when a 95% Wilson CI is available. Confidence bucket is derived from the CI half-width; see Methodology._"
        );
        let _ = writeln!(out);
    }

    if !summary.campaign_summaries.is_empty() {
        let _ = writeln!(out, "## Kill Chain Phase Breakdown");
        for (chain_id, cs) in &summary.campaign_summaries {
            let _ = writeln!(out, "### `{}`", chain_id);
            let _ = writeln!(
                out,
                "- Overall success: **{:.1}%** · Detection: **{:.1}%** · Attribution confidence: {:.2}",
                cs.overall_success_rate * 100.0,
                cs.detection_rate * 100.0,
                cs.mean_attribution_confidence
            );
            let _ = writeln!(
                out,
                "- Attacker spend: **${:.0}** · Defender spend: **${:.0}** · Asymmetry: **{:.0}×**",
                cs.mean_attacker_spend, cs.mean_defender_spend, cs.cost_asymmetry_ratio
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "| Phase | Success | Failure | Detection | Not reached | Mean completion tick |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|");
            for (pid, ps) in &cs.phase_stats {
                let mean_tick = ps
                    .mean_completion_tick
                    .map(|t| format!("{:.1}", t))
                    .unwrap_or_else(|| "—".to_string());
                let cis = ps.ci_95.as_ref();
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} | {} | {} |",
                    pid,
                    fmt_rate_cell(ps.success_rate, cis.map(|c| &c.success_rate)),
                    fmt_rate_cell(ps.failure_rate, cis.map(|c| &c.failure_rate)),
                    fmt_rate_cell(ps.detection_rate, cis.map(|c| &c.detection_rate)),
                    fmt_rate_cell(ps.not_reached_rate, cis.map(|c| &c.not_reached_rate)),
                    mean_tick
                );
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "_Rate cells show point estimate with 95% Wilson bounds when `n > 0`. Bounds widen for rare outcomes — a `0.0% (0.0–7.1)` success rate at `n = 50` is not the same as a deterministic zero._"
            );
            let _ = writeln!(out);
        }
    }

    render_time_dynamics(&mut out, summary);
    render_pareto_frontier(&mut out, summary.pareto_frontier.as_ref());
    render_correlation_matrix(&mut out, summary.correlation_matrix.as_ref());
    render_defender_capacity(&mut out, summary);

    if !summary.seam_scores.is_empty() {
        let _ = writeln!(out, "## Doctrinal Seam Analysis");
        let _ = writeln!(
            out,
            "| Chain | Cross-domain phases | Mean domains/phase | Seam exploitation share |"
        );
        let _ = writeln!(out, "|---|---|---|---|");
        for (chain_id, s) in &summary.seam_scores {
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.2} | {:.1}% |",
                chain_id,
                s.cross_domain_phase_count,
                s.mean_domains_per_phase,
                s.seam_exploitation_share * 100.0,
            );
        }
        let _ = writeln!(out);
        for (chain_id, s) in &summary.seam_scores {
            if s.domain_frequency.is_empty() {
                continue;
            }
            let _ = writeln!(out, "**`{}` domain frequency:**", chain_id);
            for (d, n) in &s.domain_frequency {
                let _ = writeln!(out, "- {}: {}", d, n);
            }
            let _ = writeln!(out);
        }
    }

    if !summary.regional_control.is_empty() {
        let _ = writeln!(out, "## Regional Control (terminal)");
        for (rid, fmap) in &summary.regional_control {
            let _ = write!(out, "- `{}`: ", rid);
            let parts: Vec<String> = fmap
                .iter()
                .map(|(fid, p)| format!("{} {:.0}%", fid, p * 100.0))
                .collect();
            let _ = writeln!(out, "{}", parts.join(", "));
        }
        let _ = writeln!(out);
    }

    let author_flagged = collect_author_flagged(scenario);
    if !author_flagged.is_empty() {
        let _ = writeln!(out, "## Author-Flagged Low-Confidence Parameters");
        let _ = writeln!(
            out,
            "The following scenario parameters are tagged `Low` confidence by the scenario author. Results that depend on them should be interpreted with correspondingly wider uncertainty than the Wilson CIs alone suggest."
        );
        let _ = writeln!(out);
        for (chain_name, phase_id, kind) in &author_flagged {
            let _ = writeln!(out, "- **{}** / `{}` — {}", chain_name, phase_id, kind);
        }
        let _ = writeln!(out);
    }

    render_policy_implications(&mut out, scenario);
    render_countermeasure_analysis(&mut out, scenario);
    render_environment_schedule(&mut out, scenario);
    render_leadership_disruption(&mut out, scenario);

    let _ = writeln!(out, "## Methodology & Confidence");
    let _ = writeln!(out, "{}", METHODOLOGY_APPENDIX.trim_start());

    out
}

// ---------------------------------------------------------------------------
// Policy Implications (Epic B)
// ---------------------------------------------------------------------------

/// Render the Policy Implications section: surfaces declarative
/// defender_options on events and escalation_rules on factions, so
/// analysts see which counterfactuals the scenario author has
/// pre-enumerated alongside the doctrine / ROE contract each faction
/// operates under.
///
/// The section is elided when no events carry `defender_options` and
/// no factions carry `escalation_rules` — we don't want an empty
/// section cluttering scenarios that pre-date Epic B.
fn render_policy_implications(out: &mut String, scenario: &Scenario) {
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

// ---------------------------------------------------------------------------
// Countermeasure Analysis (Epic B)
// ---------------------------------------------------------------------------

/// Render the Countermeasure Analysis section: surfaces per-phase
/// warning indicators (IWI / IOC entries). Each indicator pairs an
/// observable discipline with a detectability estimate and — when
/// available — a time-to-detect figure and annual monitoring cost.
///
/// Declarative in this iteration: detection probability in the engine
/// is still driven by `CampaignPhase.detection_probability_per_tick`.
/// The section exists to make the *monitoring posture* required to
/// hit that rate concrete, so analysts can reason about whether the
/// assumed detection rate is credibly achievable.
fn render_countermeasure_analysis(out: &mut String, scenario: &Scenario) {
    let chains_with_indicators: Vec<(&KillChain, Vec<(&PhaseId, &CampaignPhase)>)> = scenario
        .kill_chains
        .values()
        .filter_map(|chain| {
            let phases: Vec<_> = chain
                .phases
                .iter()
                .filter(|(_, p)| !p.warning_indicators.is_empty())
                .collect();
            if phases.is_empty() {
                None
            } else {
                Some((chain, phases))
            }
        })
        .collect();

    if chains_with_indicators.is_empty() {
        return;
    }

    let _ = writeln!(out, "## Countermeasure Analysis");
    let _ = writeln!(
        out,
        "Warning indicators the scenario author has tagged on each phase, showing the monitoring posture the defender would need in order to catch the operation before completion. Detectability is the probability that an adequately-resourced monitor picks up the observable during the phase; time-to-detect is the expected latency from phase activation. Costs are annual, if the author supplied them."
    );
    let _ = writeln!(out);

    for (chain, phases) in chains_with_indicators {
        let _ = writeln!(out, "### `{}` — {}", chain.id, chain.name);
        let _ = writeln!(
            out,
            "| Phase | Indicator | Observable | Detectability | Time to detect | Annual cost |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for (pid, phase) in phases {
            for ind in &phase.warning_indicators {
                render_indicator_row(out, pid, phase, ind);
            }
        }
        let _ = writeln!(out);
    }
}

fn render_indicator_row(
    out: &mut String,
    pid: &PhaseId,
    phase: &CampaignPhase,
    ind: &WarningIndicator,
) {
    let ttd = ind
        .time_to_detect_ticks
        .map(|t| format!("{} ticks", t))
        .unwrap_or_else(|| "—".into());
    let cost = ind
        .monitoring_cost_annual
        .map(|c| format!("${:.0}", c))
        .unwrap_or_else(|| "—".into());
    // Author-supplied strings (`phase.name`, `ind.name`, `Custom` discipline
    // labels) are interpolated into a Markdown table cell. A literal `|`
    // would close the cell early and silently mangle the table; escape it.
    let _ = writeln!(
        out,
        "| `{}` ({}) | `{}` {} | {} | {:.0}% | {} | {} |",
        pid,
        escape_md_cell(&phase.name),
        ind.id,
        escape_md_cell(&ind.name),
        escape_md_cell(observable_label(&ind.observable)),
        ind.detectability * 100.0,
        ttd,
        cost
    );
}

fn observable_label(d: &ObservableDiscipline) -> &str {
    match d {
        ObservableDiscipline::SIGINT => "SIGINT",
        ObservableDiscipline::HUMINT => "HUMINT",
        ObservableDiscipline::OSINT => "OSINT",
        ObservableDiscipline::GEOINT => "GEOINT",
        ObservableDiscipline::MASINT => "MASINT",
        ObservableDiscipline::CYBINT => "CYBINT",
        ObservableDiscipline::FININT => "FININT",
        ObservableDiscipline::Physical => "Physical",
        ObservableDiscipline::Custom(s) => s,
    }
}

/// Escape user-supplied strings for inclusion in a Markdown table cell.
///
/// A literal `|` closes the cell early and breaks table rendering;
/// `\n` / `\r` would split the row across multiple table rows;
/// backticks open inline code spans that can leak formatting into
/// neighboring cells when unbalanced. All can appear in author-
/// supplied scenario fields (phase / indicator names, custom
/// discipline labels, escalation-rung action lists, environment-
/// window IDs), so escape them at the boundary rather than relying
/// on author hygiene.
fn escape_md_cell(s: &str) -> String {
    s.replace('\\', r"\\")
        .replace('|', r"\|")
        .replace('`', r"\`")
        .replace(['\n', '\r'], " ")
}

// ---------------------------------------------------------------------------
// Leadership disruption (Epic D — decapitation + succession)
// ---------------------------------------------------------------------------

/// Render the declarative leadership cadre table per faction.
///
/// Surfaces the *structure* of every faction's leadership cadre
/// (ranks, succession parameters) so analysts can read the
/// decapitation surface a scenario exposes without having to grep the
/// TOML. The dynamic per-run decapitation tally is emitted only in
/// single-run mode (per-run `RunResult.final_state` carries the
/// cumulative counters); cross-run aggregation is left for a follow-up
/// epic that adds decap analytics to `MonteCarloSummary`. Elided when
/// no faction declares a cadre.
fn render_leadership_disruption(out: &mut String, scenario: &Scenario) {
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
        "Declared decapitation surface per faction. A `LeadershipDecapitation` phase output advances the rank index by one and applies a morale shock; the new rank's effectiveness × `succession_floor` caps the target's morale during the recovery ramp."
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

// ---------------------------------------------------------------------------
// Environment schedule (Epic D — weather, time-of-day)
// ---------------------------------------------------------------------------

/// Render the environment-schedule section.
///
/// Elided when the scenario declares no windows — readers of legacy
/// scenarios see the report unchanged. When windows exist, surface
/// each one with its activation summary and any non-unity factors so
/// analysts can audit which environmental effects shaped the run.
fn render_environment_schedule(out: &mut String, scenario: &Scenario) {
    use faultline_types::map::Activation;

    if scenario.environment.windows.is_empty() {
        return;
    }

    let _ = writeln!(out, "## Environment Schedule");
    let _ = writeln!(
        out,
        "Active environmental windows. Per-terrain factors apply when the engaged region's terrain is in `applies_to` (empty = every terrain). The `detection` factor is global — it multiplies every kill-chain phase's per-tick detection probability regardless of region."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Window | Activation | Applies to | Movement | Defense | Visibility | Detection |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for w in &scenario.environment.windows {
        let activation = match &w.activation {
            Activation::Always => "Always".to_string(),
            Activation::TickRange { start, end } => {
                format!("Ticks {start}–{end}")
            },
            Activation::Cycle {
                period,
                phase,
                duration,
            } => {
                format!("Cycle p={period} φ={phase} d={duration}")
            },
        };
        let applies = if w.applies_to.is_empty() {
            "all".to_string()
        } else {
            w.applies_to
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let fmt_factor = |f: f64| {
            if (f - 1.0).abs() < 1e-9 {
                "—".to_string()
            } else {
                format!("{f:.2}×")
            }
        };
        let _ = writeln!(
            out,
            "| `{}` ({}) | {} | {} | {} | {} | {} | {} |",
            escape_md_cell(&w.id),
            escape_md_cell(&w.name),
            activation,
            escape_md_cell(&applies),
            fmt_factor(w.movement_factor),
            fmt_factor(w.defense_factor),
            fmt_factor(w.visibility_factor),
            fmt_factor(w.detection_factor),
        );
    }
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Comparison report (Epic B --counterfactual / --compare)
// ---------------------------------------------------------------------------

/// Render a Markdown report for a counterfactual or `--compare` run.
///
/// Prepends a "Counterfactual Comparison" section to the usual
/// per-scenario report so readers see the deltas first. `scenario` is
/// the baseline; each variant summary is already included in `report`.
pub fn render_comparison_markdown(report: &ComparisonReport, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Counterfactual Report");
    let _ = writeln!(out, "## Baseline: {}", report.baseline_label);
    let _ = writeln!(out);
    let _ = writeln!(out, "- **Baseline runs:** {}", report.baseline.total_runs);
    let _ = writeln!(
        out,
        "- **Baseline mean duration:** {:.1} ticks",
        report.baseline.average_duration
    );
    let _ = writeln!(out);

    for (variant, delta) in report.variants.iter().zip(report.deltas.iter()) {
        render_variant_section(&mut out, variant, delta, &report.baseline);
    }

    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# Baseline Full Report");
    let _ = writeln!(out);

    out.push_str(&render_markdown(&report.baseline, scenario));

    out
}

fn render_variant_section(
    out: &mut String,
    variant: &crate::counterfactual::VariantSummary,
    delta: &crate::counterfactual::ComparisonDelta,
    baseline: &MonteCarloSummary,
) {
    let _ = writeln!(out, "## Variant: {}", variant.label);
    if let Some(src) = &variant.source_scenario {
        let _ = writeln!(out, "- **Source scenario:** {}", src);
    }
    if !variant.overrides.is_empty() {
        let _ = writeln!(out, "- **Applied overrides:**");
        for ov in &variant.overrides {
            render_override_line(out, ov);
        }
    }
    let _ = writeln!(
        out,
        "- **Mean duration delta:** {:+.2} ticks ({:.1} → {:.1})",
        delta.mean_duration_delta, baseline.average_duration, variant.summary.average_duration
    );
    let _ = writeln!(out);

    if !delta.win_rate_deltas.is_empty() {
        let _ = writeln!(out, "### Win-rate deltas");
        let _ = writeln!(out, "| Faction | Baseline | Variant | Δ (pp) |");
        let _ = writeln!(out, "|---|---|---|---|");
        for (fid, d) in &delta.win_rate_deltas {
            let b = baseline.win_rates.get(fid).copied().unwrap_or(0.0);
            let v = variant.summary.win_rates.get(fid).copied().unwrap_or(0.0);
            let _ = writeln!(
                out,
                "| `{}` | {:.1}% | {:.1}% | **{:+.1}** |",
                fid,
                b * 100.0,
                v * 100.0,
                d * 100.0
            );
        }
        let _ = writeln!(out);
    }

    if !delta.chain_deltas.is_empty() {
        let _ = writeln!(out, "### Kill-chain deltas");
        let _ = writeln!(
            out,
            "| Chain | Success Δ (pp) | Detection Δ (pp) | Cost-ratio Δ | Attacker spend Δ | Defender spend Δ |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for (cid, cd) in &delta.chain_deltas {
            let _ = writeln!(
                out,
                "| `{}` | **{:+.1}** | **{:+.1}** | **{:+.1}×** | **${:+.0}** | **${:+.0}** |",
                cid,
                cd.overall_success_rate_delta * 100.0,
                cd.detection_rate_delta * 100.0,
                cd.cost_asymmetry_ratio_delta,
                cd.attacker_spend_delta,
                cd.defender_spend_delta
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Positive success Δ = campaign more likely to succeed under the variant; positive detection Δ = defender more likely to catch it; positive cost-ratio Δ = defender paying more per attacker dollar. Both batches share the same seed and run count._"
        );
        let _ = writeln!(out);
    }
}

fn render_override_line(out: &mut String, ov: &ParamOverride) {
    let _ = writeln!(out, "  - `{}` = **{}**", ov.path, ov.value);
}

// ---------------------------------------------------------------------------
// Strategy search report (Epic H)
// ---------------------------------------------------------------------------

/// Render the Markdown report for a strategy-search batch.
///
/// Layout:
///
/// 1. Setup block (method, trials, objectives) so the reader sees the
///    search scope before any rankings.
/// 2. Best-by-objective table — one row per objective, naming the
///    winning trial and its objective value.
/// 3. Pareto frontier table — every non-dominated trial with its
///    full objective-value vector.
/// 4. Per-trial detail — assignments + objective values, one line per
///    trial. Truncated to the first 64 trials with a hint when the
///    search is larger, since the JSON artifact carries the rest.
pub fn render_search_markdown(result: &SearchResult, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Strategy Search Report");
    let _ = writeln!(out);
    let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
    let _ = writeln!(out);
    let method_label = match result.method {
        SearchMethod::Random => "random",
        SearchMethod::Grid => "grid",
    };
    let _ = writeln!(out, "- **Method:** `{method_label}`");
    let _ = writeln!(out, "- **Trials evaluated:** {}", result.trials.len());
    let _ = writeln!(out, "- **Objectives:** {}", result.objectives.len());
    if !scenario.strategy_space.variables.is_empty() {
        let _ = writeln!(
            out,
            "- **Decision variables:** {}",
            scenario.strategy_space.variables.len()
        );
    }
    let _ = writeln!(out);

    if result.trials.is_empty() {
        let _ = writeln!(
            out,
            "_No trials evaluated; nothing to report. Increase `--search-trials` or check the scenario's strategy space._"
        );
        return out;
    }

    render_best_by_objective(&mut out, result);
    render_search_pareto(&mut out, result);
    render_counter_recommendation(&mut out, result, scenario);
    render_search_trials(&mut out, result);

    out
}

fn render_best_by_objective(out: &mut String, result: &SearchResult) {
    let _ = writeln!(out, "## Best Trial Per Objective");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Objective | Direction | Trial | Value |");
    let _ = writeln!(out, "|---|---|---|---|");
    for obj in &result.objectives {
        let label = obj.label();
        let direction = if obj.maximize() { "max" } else { "min" };
        let cell = match result.best_by_objective.get(&label) {
            Some(idx) => {
                let v = result
                    .trials
                    .get(*idx as usize)
                    .and_then(|t| t.objective_values.get(&label))
                    .copied()
                    .unwrap_or(0.0);
                format!("`#{idx}` | **{:.4}**", v)
            },
            None => "—".to_string(),
        };
        let _ = writeln!(
            out,
            "| `{label}` | {direction} | {cell} |",
            label = label,
            direction = direction,
            cell = cell,
        );
    }
    let _ = writeln!(out);
}

fn render_search_pareto(out: &mut String, result: &SearchResult) {
    let _ = writeln!(out, "## Pareto Frontier");
    let _ = writeln!(out);
    if result.pareto_indices.is_empty() {
        let _ = writeln!(out, "_No non-dominated trials._");
        let _ = writeln!(out);
        return;
    }
    let _ = writeln!(
        out,
        "Non-dominated trials across all declared objectives. A trial is on the frontier when no other trial is at least as good on every objective and strictly better on at least one (direction-aware)."
    );
    let _ = writeln!(out);
    let mut header = String::from("| Trial |");
    let mut rule = String::from("|---|");
    for obj in &result.objectives {
        header.push_str(&format!(" {} |", obj.label()));
        rule.push_str("---|");
    }
    let _ = writeln!(out, "{header}");
    let _ = writeln!(out, "{rule}");
    for idx in &result.pareto_indices {
        if let Some(t) = result.trials.get(*idx as usize) {
            let mut row = format!("| `#{}` |", idx);
            for obj in &result.objectives {
                let v = t.objective_values.get(&obj.label()).copied().unwrap_or(0.0);
                row.push_str(&format!(" {:.4} |", v));
            }
            let _ = writeln!(out, "{row}");
        }
    }
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Counter-Recommendation (Epic I)
// ---------------------------------------------------------------------------

/// Sub-epsilon deltas (floating-point noise from re-running the same
/// MC config with the same seed against the baseline) read as "no
/// change" — otherwise the table renders misleading
/// `+0.0000` / `−0.0000` cells for effectively identical baselines.
const DELTA_EPSILON: f64 = 1e-9;

/// Render the Counter-Recommendation section: ranks Pareto-frontier
/// trials by per-objective improvement against the search's
/// "do-nothing" baseline, with Wilson CIs on rate-valued metrics.
///
/// Surfaces only when:
///
/// - the search produced a baseline (`SearchConfig.compute_baseline`),
/// - at least one decision variable carries an `owner` so the section
///   can name *whose* posture is being evaluated,
/// - the Pareto frontier is non-empty.
///
/// The deltas the section reports are direction-aware: a row tagged
/// "improvement" means the trial moved the objective in the
/// optimization direction declared by `SearchObjective::maximize()`.
/// For win-rate-style rate objectives, the table reports the trial's
/// 95% Wilson CI alongside the baseline's so the analyst can read
/// whether the improvement is statistically distinguishable from
/// sampling noise.
fn render_counter_recommendation(out: &mut String, result: &SearchResult, scenario: &Scenario) {
    let baseline = match result.baseline.as_ref() {
        Some(b) => b,
        None => return,
    };
    if result.pareto_indices.is_empty() {
        return;
    }
    // Only emit the section when at least one decision variable names
    // an owner — without it the analyst can't read "the defender's
    // best posture" off the table, and the section adds noise. This
    // keeps the legacy attacker-only `strategy_search_demo.toml`
    // search reports unchanged.
    let has_owner = scenario
        .strategy_space
        .variables
        .iter()
        .any(|v| v.owner.is_some());
    if !has_owner {
        return;
    }

    let _ = writeln!(out, "## Counter-Recommendation");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Ranks Pareto-frontier trials by per-objective improvement against the **do-nothing baseline** (the scenario evaluated with no decision-variable assignment). Each delta is direction-aware: positive = better in the objective's optimization direction; negative = worse."
    );
    let _ = writeln!(out);

    // Group decision variables by owner so the recommendation reads
    // "alpha's optimal posture" / "bravo's optimal posture". Owners
    // are surfaced only when present; un-owned variables are listed
    // separately under "(no owner)".
    let owners = collect_decision_owners(scenario);
    // `owners.len()` counts the `None` bucket (un-owned variables) toward
    // the length, so this gate also fires when there is exactly one
    // owner-tagged variable plus one or more un-owned variables — the
    // subsection then renders a `_(no owner)_` row alongside the faction
    // entry, which is intentional. Read this as "more than one group",
    // not "two or more distinct factions".
    if owners.len() > 1 {
        let _ = writeln!(out, "### Decision variables by owner");
        let _ = writeln!(out);
        for (owner, paths) in &owners {
            let label = owner
                .as_ref()
                .map(|f| format!("`{f}`"))
                .unwrap_or_else(|| "_(no owner)_".to_string());
            let _ = writeln!(out, "- {label}: {}", paths.join(", "));
        }
        let _ = writeln!(out);
    }

    // Per Pareto-frontier trial: a single block with the trial's
    // assignments, then a small per-objective delta table.
    for idx in &result.pareto_indices {
        let trial = match result.trials.get(*idx as usize) {
            Some(t) => t,
            None => continue,
        };
        let _ = writeln!(out, "### Recommendation: trial `#{}`", idx);
        if !trial.assignments.is_empty() {
            let _ = writeln!(out, "Posture:");
            for ov in &trial.assignments {
                let _ = writeln!(out, "- `{}` = **{:.4}**", ov.path, ov.value);
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "| Objective | Direction | Baseline | Trial | Δ | Improvement? |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|");
        for obj in &result.objectives {
            let label = obj.label();
            let bv = baseline
                .objective_values
                .get(&label)
                .copied()
                .unwrap_or(0.0);
            let tv = trial.objective_values.get(&label).copied().unwrap_or(0.0);
            let raw_delta = tv - bv;
            let is_zero = raw_delta.abs() < DELTA_EPSILON;
            let improved = !is_zero
                && if obj.maximize() {
                    raw_delta > 0.0
                } else {
                    raw_delta < 0.0
                };
            let direction = if obj.maximize() { "max" } else { "min" };
            // Zero-delta cells render as the bare `·` glyph — appending
            // `0.0000` to the symbol reads strangely and misleads the
            // analyst into looking for a magnitude.
            let delta_cell = if is_zero {
                "·".to_string()
            } else {
                let glyph = if improved { "+" } else { "−" };
                format!("{}{:.4}", glyph, raw_delta.abs())
            };
            let improvement_cell = if is_zero {
                "·"
            } else if improved {
                "yes"
            } else {
                "no"
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.4} | {:.4} | {} | {} |",
                label, direction, bv, tv, delta_cell, improvement_cell,
            );
        }
        let _ = writeln!(out);

        // Optional Wilson-CI panel for `MaximizeWinRate` objectives —
        // the only currently-defined rate-valued objective with the
        // sample size carried on `MonteCarloSummary.total_runs`. Other
        // objectives (sums, maxes, durations) are continuous metrics
        // that need bootstrap CIs; deferred to a follow-up so the
        // first round-one slice ships clean.
        let mut wilson_lines: Vec<String> = Vec::new();
        for obj in &result.objectives {
            if let SearchObjective::MaximizeWinRate { faction } = obj {
                let bn = baseline.summary.total_runs;
                let tn = trial.summary.total_runs;
                let bp = baseline
                    .summary
                    .win_rates
                    .get(faction)
                    .copied()
                    .unwrap_or(0.0);
                let tp = trial.summary.win_rates.get(faction).copied().unwrap_or(0.0);
                if let (Some(bw), Some(tw)) = (
                    crate::uncertainty::wilson_from_rate(bp, bn),
                    crate::uncertainty::wilson_from_rate(tp, tn),
                ) {
                    wilson_lines.push(format!(
                        "- `{}`: baseline {:.1}% (95% CI {:.1}–{:.1}%), trial {:.1}% (95% CI {:.1}–{:.1}%)",
                        obj.label(),
                        bp * 100.0,
                        bw.lower * 100.0,
                        bw.upper * 100.0,
                        tp * 100.0,
                        tw.lower * 100.0,
                        tw.upper * 100.0,
                    ));
                }
            }
        }
        if !wilson_lines.is_empty() {
            let _ = writeln!(out, "Win-rate Wilson 95% CIs:");
            for line in &wilson_lines {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out);
        }
    }
}

fn collect_decision_owners(
    scenario: &Scenario,
) -> std::collections::BTreeMap<Option<faultline_types::ids::FactionId>, Vec<String>> {
    let mut by_owner: std::collections::BTreeMap<
        Option<faultline_types::ids::FactionId>,
        Vec<String>,
    > = std::collections::BTreeMap::new();
    for var in &scenario.strategy_space.variables {
        by_owner
            .entry(var.owner.clone())
            .or_default()
            .push(format!("`{}`", var.path));
    }
    by_owner
}

const SEARCH_TRIAL_RENDER_LIMIT: usize = 64;

fn render_search_trials(out: &mut String, result: &SearchResult) {
    let _ = writeln!(out, "## Trial Detail");
    let _ = writeln!(out);
    let total = result.trials.len();
    let shown = total.min(SEARCH_TRIAL_RENDER_LIMIT);
    if shown < total {
        let _ = writeln!(
            out,
            "_Showing first {shown} of {total} trials. Full set lives in `search.json`._"
        );
        let _ = writeln!(out);
    }
    for t in result.trials.iter().take(shown) {
        render_search_trial(out, t, &result.objectives);
    }
}

fn render_search_trial(out: &mut String, trial: &SearchTrial, objectives: &[SearchObjective]) {
    // Trials in `SearchResult.trials` always carry an index; the
    // baseline lives in its own field and is never rendered here.
    let label = match trial.trial_index {
        Some(i) => format!("#{i}"),
        None => "baseline".to_string(),
    };
    let _ = writeln!(out, "### Trial `{label}`");
    if !trial.assignments.is_empty() {
        let _ = writeln!(out, "Assignments:");
        for ov in &trial.assignments {
            let _ = writeln!(out, "- `{}` = **{:.4}**", ov.path, ov.value);
        }
    }
    let _ = writeln!(out, "Objective values:");
    for obj in objectives {
        let label = obj.label();
        let v = trial.objective_values.get(&label).copied().unwrap_or(0.0);
        let _ = writeln!(out, "- `{label}` = {:.4}", v);
    }
    let _ = writeln!(out);
}

const METHODOLOGY_APPENDIX: &str = r#"
This report combines two distinct sources of uncertainty. Mixing them up is a common way to get analysis wrong, so they are reported separately:

- **Sampling uncertainty** (the Wilson CIs below). Given the scenario's specified parameters, how precisely did the Monte Carlo runs estimate the rates shown? More runs shrink these intervals.
- **Parameter uncertainty** (the author-flagged confidence tags). Are the input parameters themselves defensible? A tight Wilson CI around a success rate derived from expert-guess detection probabilities does not mean the real-world success rate is known to that precision.

### 95% confidence intervals
Win rates, phase success rates, detection rates, and the rate-valued feasibility cells use the [Wilson score interval][wilson] at `z ≈ 1.960` (the standard-normal 97.5% quantile). Wilson is used in preference to the textbook Wald approximation because Wald collapses to `[0, 0]` or `[1, 1]` when zero or all runs succeed, implying false certainty for rare events. Wilson retains well-calibrated coverage across `p ∈ [0, 1]`.

Continuous metrics (duration, casualties, resources expended) are summarised by their mean with a 95% **percentile-bootstrap CI** on the mean, plus the 5th / 95th percentiles and standard deviation of the run distribution itself. The bootstrap draws 500 resamples from a deterministic `ChaCha8Rng` seeded from `scenario.simulation.seed` so the report is bit-identical across repeated runs. Keep the two quantities distinct: the bootstrap CI narrows as `n_runs` grows; the 5–95 percentile spread reflects inherent variability in the modelled outcome and does not.

[wilson]: https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval#Wilson_score_interval

### Confidence bucket derivation
The `[H]` / `[M]` / `[L]` tag on rate-valued feasibility cells is a coarse readability aid derived from the Wilson CI half-width at the scenario's run count:

| Bucket | Wilson half-width | Interpretation |
|---|---|---|
| `H` (High) | `< 0.03` | ±3 percentage points at 95% |
| `M` (Medium) | `< 0.08` | ±8 percentage points at 95% |
| `L` (Low) | otherwise (or `n < 30`) | Wide enough that comparing two `L` values is unsafe |

The `technology_readiness` bucket is a separate diagnostic: it is `L` when fewer than two phases exist in the chain, and otherwise buckets the coefficient of variation of per-phase base-success probabilities (`<0.15` → `H`, `<0.40` → `M`, else `L`). A `L` tag here means the chain's phases vary widely in expected success and a single "readiness" number is lossy, not that the MC estimate is imprecise.

### Author-flagged parameters
Authors can annotate `CampaignPhase.parameter_confidence` and `PhaseCost.confidence` in the TOML scenario to signal how defensible the input numbers are — `High` for commodity-parts costs or published rate cards, `Low` for wide expert estimates. Any phase or cost block flagged `Low` is listed in a dedicated section above when present. This complements, and does not replace, a full sensitivity sweep.

### Scenario-level confidence banner
The optional `[meta].confidence` field tags the scenario as a whole:

| Tag | Intended meaning |
|---|---|
| `High` | Publication-ready rigor — every capability parameter is backed by a cited open source. |
| `Medium` | Working draft — structurally complete but some parameters still rest on expert guess. |
| `Low` | Conceptual sketch — intended to illustrate a mechanic, not to stand as analysis. |

This is a coarse, author-asserted flag. It is *not* derived from the MC output and does not narrow or widen any CI — it tells the reader how much weight to place on the inputs before any sampling question comes into play.
"#;

fn fmt_cell(value: f64, conf: ConfidenceLevel, ci: Option<&ConfidenceInterval>) -> String {
    let tag = match conf {
        ConfidenceLevel::High => "H",
        ConfidenceLevel::Medium => "M",
        ConfidenceLevel::Low => "L",
    };
    match ci {
        Some(ci) => format!("{:.2} [{}] ({:.2}–{:.2})", value, tag, ci.lower, ci.upper),
        None => format!("{:.2} [{}]", value, tag),
    }
}

fn fmt_rate_cell(rate: f64, ci: Option<&ConfidenceInterval>) -> String {
    match ci {
        Some(ci) => format!(
            "{:.1}% ({:.1}–{:.1})",
            rate * 100.0,
            ci.lower * 100.0,
            ci.upper * 100.0
        ),
        None => format!("{:.1}%", rate * 100.0),
    }
}

fn fmt_ci_pct(ci: &ConfidenceInterval) -> String {
    format!("{:.1}% – {:.1}%", ci.lower * 100.0, ci.upper * 100.0)
}

fn confidence_word(c: &ConfidenceLevel) -> &'static str {
    match c {
        ConfidenceLevel::High => "High",
        ConfidenceLevel::Medium => "Medium",
        ConfidenceLevel::Low => "Low",
    }
}

fn metric_label(m: &MetricType) -> String {
    match m {
        MetricType::Duration => "Duration (ticks)".into(),
        MetricType::FinalTension => "Final tension".into(),
        MetricType::TotalCasualties => "Total casualties".into(),
        MetricType::InfrastructureDamage => "Infrastructure damage".into(),
        MetricType::CivilianDisplacement => "Civilian displacement".into(),
        MetricType::ResourcesExpended => "Resources expended".into(),
        MetricType::Custom(s) => s.clone(),
    }
}

fn render_continuous_metrics(out: &mut String, summary: &MonteCarloSummary) {
    if summary.metric_distributions.is_empty() {
        return;
    }
    // Header must match cell content: if any metric lacks a bootstrap CI
    // (e.g. a legacy `MonteCarloSummary` deserialized from a pre-bootstrap
    // build where `bootstrap_ci_mean` defaults to `None`), `fmt_mean_with_bootstrap`
    // falls back to a bare mean for those rows. A blanket "Mean (95% bootstrap CI)"
    // header would then mislabel those cells.
    let all_have_ci = summary
        .metric_distributions
        .values()
        .all(|s| s.bootstrap_ci_mean.is_some());
    let mean_header = if all_have_ci {
        "Mean (95% bootstrap CI)"
    } else {
        "Mean"
    };
    let _ = writeln!(out, "## Continuous Metrics");
    let _ = writeln!(
        out,
        "| Metric | {mean_header} | Median | 5th – 95th pct | Std dev |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|");
    for (metric, stats) in &summary.metric_distributions {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} – {} | {} |",
            metric_label(metric),
            fmt_mean_with_bootstrap(stats, all_have_ci),
            fmt_scalar(stats.median),
            fmt_scalar(stats.percentile_5),
            fmt_scalar(stats.percentile_95),
            fmt_scalar(stats.std_dev),
        );
    }
    if all_have_ci {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "_Bootstrap CIs use 500 percentile-bootstrap resamples seeded from the scenario. Percentiles describe the *distribution* of run outcomes — not uncertainty on the mean._"
        );
    }
    let _ = writeln!(out);
}

// `show_ci` must mirror the column header: if the header does not advertise
// a bootstrap CI (because some other row in the same table lacks one), this
// row must also suppress its bounds even if its own `bootstrap_ci_mean` is
// `Some(..)`. Otherwise the cell carries CI syntax under a plain "Mean" header.
fn fmt_mean_with_bootstrap(stats: &DistributionStats, show_ci: bool) -> String {
    match (show_ci, stats.bootstrap_ci_mean.as_ref()) {
        (true, Some(ci)) => format!(
            "{} ({} – {})",
            fmt_scalar(stats.mean),
            fmt_scalar(ci.lower),
            fmt_scalar(ci.upper)
        ),
        _ => fmt_scalar(stats.mean),
    }
}

// Adaptive number formatting: proportions get three decimals, larger
// magnitudes round to whole units. Keeps the metrics table legible
// whether it's showing `0.234` tension or `2_500` casualties.
fn fmt_scalar(v: f64) -> String {
    let abs = v.abs();
    if abs < 1.0 {
        format!("{v:.3}")
    } else if abs < 100.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.0}")
    }
}

fn collect_author_flagged(scenario: &Scenario) -> Vec<(String, PhaseId, String)> {
    let mut out = Vec::new();
    for chain in scenario.kill_chains.values() {
        collect_flagged_from_chain(chain, &mut out);
    }
    out
}

fn collect_flagged_from_chain(chain: &KillChain, out: &mut Vec<(String, PhaseId, String)>) {
    let chain_name = chain.name.clone();
    for (pid, phase) in &chain.phases {
        if let Some(kind) = describe_flag(phase) {
            out.push((chain_name.clone(), pid.clone(), kind));
        }
    }
}

fn describe_flag(phase: &CampaignPhase) -> Option<String> {
    let mut parts: Vec<&'static str> = Vec::new();
    if matches!(phase.parameter_confidence, Some(ConfidenceLevel::Low)) {
        parts.push("phase parameters");
    }
    if matches!(phase.cost.confidence, Some(ConfidenceLevel::Low)) {
        parts.push("phase cost");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

// The renderer pulls these types into the compiled output via
// `summary.feasibility_matrix`; keep an explicit anchor so that
// removing the table from the document does not silently drop the
// import.
#[allow(dead_code)]
fn _type_anchor(_r: &FeasibilityRow, _c: &FeasibilityConfidence) {}

// ---------------------------------------------------------------------------
// Time dynamics (Epic C)
// ---------------------------------------------------------------------------

/// Render the per-chain time-to-first-detection and defender-reaction-
/// time tables. The section is elided when no chain has either signal —
/// the engine emits both fields as `None` for chains the runner never
/// observed, and we don't want to print empty tables in scenarios with
/// no kill chains at all.
fn render_time_dynamics(out: &mut String, summary: &MonteCarloSummary) {
    let any_ttd = summary
        .campaign_summaries
        .values()
        .any(|cs| cs.time_to_first_detection.is_some());
    let any_react = summary
        .campaign_summaries
        .values()
        .any(|cs| cs.defender_reaction_time.is_some());
    let any_km = summary
        .campaign_summaries
        .values()
        .any(|cs| !cs.phase_survival.is_empty());
    if !any_ttd && !any_react && !any_km {
        return;
    }
    let _ = writeln!(out, "## Time & Attribution Dynamics");
    let _ = writeln!(
        out,
        "Per-chain timing of the first defender alert, the post-detection runway the operation kept, and Kaplan-Meier survival curves for each phase. Detection times are right-censored when the defender was never alerted in a run — those runs sit in the `censored` column and do *not* contribute to the mean. Reaction time = `final_tick - first_detection_tick`; longer means the defender saw the operation but had no time to interrupt it."
    );
    let _ = writeln!(out);

    if any_ttd || any_react {
        let _ = writeln!(
            out,
            "| Chain | Detected runs | Censored | TTD mean | TTD p5 | TTD p95 | Reaction mean | Reaction p5 | Reaction p95 |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
        for (cid, cs) in &summary.campaign_summaries {
            let (det, cen, tm, tp5, tp95) = match &cs.time_to_first_detection {
                Some(ttd) => {
                    let (m, p5, p95) = match &ttd.stats {
                        Some(s) => (
                            format!("{:.1}", s.mean),
                            format!("{:.1}", s.percentile_5),
                            format!("{:.1}", s.percentile_95),
                        ),
                        None => ("—".into(), "—".into(), "—".into()),
                    };
                    (
                        ttd.detected_runs.to_string(),
                        ttd.right_censored.to_string(),
                        m,
                        p5,
                        p95,
                    )
                },
                None => ("—".into(), "—".into(), "—".into(), "—".into(), "—".into()),
            };
            let (rm, rp5, rp95) = match &cs.defender_reaction_time {
                Some(rt) => match &rt.stats {
                    Some(s) => (
                        format!("{:.1}", s.mean),
                        format!("{:.1}", s.percentile_5),
                        format!("{:.1}", s.percentile_95),
                    ),
                    None => ("—".into(), "—".into(), "—".into()),
                },
                None => ("—".into(), "—".into(), "—".into()),
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
                cid, det, cen, tm, tp5, tp95, rm, rp5, rp95
            );
        }
        let _ = writeln!(out);
    }

    if any_km {
        for (cid, cs) in &summary.campaign_summaries {
            if cs.phase_survival.is_empty() {
                continue;
            }
            let _ = writeln!(out, "### `{}` — phase survival (Kaplan-Meier)", cid);
            let _ = writeln!(
                out,
                "| Phase | n events | Censored | S(median tick) | S(p90 tick) | Median time-to-event |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|");
            render_phase_km_rows(out, cs);
            let _ = writeln!(out);
        }
        let _ = writeln!(
            out,
            "_S(t) is the probability the phase is still pending at tick `t`, with right-censoring for runs that ended without reaching the phase. Median time-to-event is the smallest tick where `S` first dropped to ≤ 0.5; `—` means it never did (most runs censored)._"
        );
        let _ = writeln!(out);
    }
}

fn render_phase_km_rows(out: &mut String, cs: &CampaignSummary) {
    for (pid, curve) in &cs.phase_survival {
        let n_events: u32 = curve.events.iter().sum();
        // Median tick of the run distribution serves as a representative
        // probe point for `S`. Hand-pick one rather than sample many to
        // keep rows compact.
        let median_tick = if curve.times.is_empty() {
            None
        } else {
            curve.times.get(curve.times.len() / 2).copied()
        };
        let p90_tick = if curve.times.is_empty() {
            None
        } else {
            // 90th-percentile event tick — the right tail of the curve.
            let idx = ((curve.times.len() as f64 - 1.0) * 0.9).round() as usize;
            curve.times.get(idx).copied()
        };
        let s_at_median = match median_tick {
            Some(t) => surv_at(curve, t),
            None => "—".into(),
        };
        let s_at_p90 = match p90_tick {
            Some(t) => surv_at(curve, t),
            None => "—".into(),
        };
        let median_event_time = curve
            .survival
            .iter()
            .position(|s| *s <= 0.5)
            .and_then(|i| curve.times.get(i))
            .map(|t| format!("{} ticks", t))
            .unwrap_or_else(|| "—".into());
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            pid, n_events, curve.censored, s_at_median, s_at_p90, median_event_time
        );
    }
}

fn surv_at(curve: &faultline_types::stats::KaplanMeierCurve, t: u32) -> String {
    // Right-continuous step function: S is held constant between event
    // times; the value at `t` is `S(t_i)` for the largest `t_i <= t`.
    let mut s = 1.0_f64;
    for (i, ti) in curve.times.iter().enumerate() {
        if *ti <= t {
            s = curve.survival[i];
        } else {
            break;
        }
    }
    format!("{:.2}", s)
}

// ---------------------------------------------------------------------------
// Pareto frontier (Epic C)
// ---------------------------------------------------------------------------

fn render_pareto_frontier(out: &mut String, frontier: Option<&ParetoFrontier>) {
    let frontier = match frontier {
        Some(f) if !f.points.is_empty() => f,
        _ => return,
    };
    let _ = writeln!(out, "## Pareto Frontier (cost · success · stealth)");
    let _ = writeln!(
        out,
        "Non-dominated runs across all {} runs in the batch. A run is on the frontier when no other run beat it on every axis simultaneously. Use this to identify the *envelope* of achievable trade-offs before reaching for a sensitivity sweep — runs *behind* the frontier had no realised advantage on any axis.",
        frontier.total_runs
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Run | Attacker cost | Success rate | Stealth (1 − max detection) |"
    );
    let _ = writeln!(out, "|---|---|---|---|");
    for p in &frontier.points {
        let _ = writeln!(
            out,
            "| `{}` | ${:.0} | {:.1}% | {:.1}% |",
            p.run_index,
            p.attacker_cost,
            p.success * 100.0,
            p.stealth * 100.0
        );
    }
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Correlation matrix (Epic C)
// ---------------------------------------------------------------------------

fn render_correlation_matrix(out: &mut String, matrix: Option<&CorrelationMatrix>) {
    let matrix = match matrix {
        Some(m) if !m.labels.is_empty() => m,
        _ => return,
    };
    // If every off-diagonal entry is None (degenerate scenario where
    // every output is constant) the matrix is uninformative — elide
    // the section rather than print a wall of `—`.
    let n = matrix.labels.len();
    let any_off_diag = (0..n)
        .flat_map(|i| (0..n).map(move |j| (i, j)))
        .filter(|(i, j)| i != j)
        .any(|(i, j)| matrix.values[i * n + j].is_some());
    if !any_off_diag {
        return;
    }
    let _ = writeln!(out, "## Output Correlation Matrix");
    let _ = writeln!(
        out,
        "Pearson correlations across the {} runs in the batch. A constant series shows up as `—` (correlation undefined). High |r| between two outputs flags shared underlying drivers; near-zero r means they move independently across runs.",
        matrix.n
    );
    let _ = writeln!(out);
    // Header.
    let _ = write!(out, "|     |");
    for label in &matrix.labels {
        let _ = write!(out, " `{}` |", label);
    }
    let _ = writeln!(out);
    let _ = write!(out, "|---|");
    for _ in &matrix.labels {
        let _ = write!(out, "---|");
    }
    let _ = writeln!(out);
    for (i, row_label) in matrix.labels.iter().enumerate() {
        let _ = write!(out, "| `{}` |", row_label);
        for j in 0..n {
            match matrix.values[i * n + j] {
                Some(v) => {
                    let _ = write!(out, " {:+.2} |", v);
                },
                None => {
                    let _ = write!(out, " — |");
                },
            }
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out);
}

/// Render the Defender Capacity section (Epic K).
///
/// Elided entirely when no scenario faction declares
/// `defender_capacities` — the rollup is empty in that case.
/// Otherwise emits a single utilization table plus a
/// time-to-saturation row per role; downstream tooling that wants the
/// raw distributions reads them off `summary.defender_capacity`
/// directly.
fn render_defender_capacity(out: &mut String, summary: &MonteCarloSummary) {
    if summary.defender_capacity.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Defender Capacity");
    let _ = writeln!(
        out,
        "Per-role investigative-queue analytics across the {} runs in the batch. Utilization is mean-depth / capacity; shadow detections are detection rolls suppressed by saturation (the defender would have caught the operation at idle but missed it under load).",
        summary.total_runs
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Faction | Role | Capacity | Mean util. | Max util. | Mean dropped | Mean shadow det. | Saturated runs |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    for q in &summary.defender_capacity {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {} | {:.1}% | {:.1}% | {:.1} | {:.2} | {}/{} |",
            q.faction,
            q.role,
            q.capacity,
            q.mean_utilization * 100.0,
            q.max_utilization * 100.0,
            q.mean_dropped,
            q.mean_shadow_detections,
            q.time_to_saturation.saturated_runs,
            q.n_runs,
        );
    }
    let _ = writeln!(out);
    // Time-to-saturation distribution per role. Right-censored: runs
    // that never saturated do not appear in the descriptive stats.
    for q in &summary.defender_capacity {
        let Some(stats) = q.time_to_saturation.stats.as_ref() else {
            continue;
        };
        let _ = writeln!(
            out,
            "**`{}` / `{}` time-to-saturation:** {} of {} runs saturated; mean {:.1} ticks (5th–95th percentile {:.1}–{:.1}).",
            q.faction,
            q.role,
            q.time_to_saturation.saturated_runs,
            q.n_runs,
            stats.mean,
            stats.percentile_5,
            stats.percentile_95,
        );
    }
    if summary
        .defender_capacity
        .iter()
        .any(|q| q.time_to_saturation.stats.is_some())
    {
        let _ = writeln!(out);
    }
}

// ---------------------------------------------------------------------------
// Co-evolution report (Epic H — round two)
// ---------------------------------------------------------------------------

/// Render a co-evolution Markdown report from a [`CoevolveResult`].
///
/// Top section explains the convergence outcome (Converged / Cycle /
/// NoEquilibrium); the round trajectory section walks each round with
/// the mover, the chosen assignments, and the objective value; the
/// final block shows the equilibrium joint state and the resulting
/// objective values for both sides.
pub fn render_coevolve_markdown(
    result: &crate::coevolve::CoevolveResult,
    scenario: &Scenario,
) -> String {
    use crate::coevolve::{CoevolveSide, CoevolveStatus};
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Co-Evolution Report");
    let _ = writeln!(out);
    let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
    let _ = writeln!(out);
    let _ = writeln!(out, "- **Attacker:** `{}`", result.attacker_faction);
    let _ = writeln!(out, "- **Defender:** `{}`", result.defender_faction);
    let _ = writeln!(out, "- **Rounds executed:** {}", result.rounds.len());
    let _ = writeln!(out);

    // Convergence callout — the most important thing the analyst is
    // looking for. Render it as a one-line bold + a short prose
    // paragraph so it survives copy-paste into a tracker without the
    // surrounding tables.
    match &result.status {
        CoevolveStatus::Converged => {
            let _ = writeln!(out, "**Outcome: Converged.**");
            let _ = writeln!(
                out,
                "Two consecutive rounds produced the same joint `(attacker, defender)` state — neither side wanted to deviate from its current assignment given the opponent's. This is a Nash equilibrium in pure strategies on the discrete strategy space the search visits."
            );
        },
        CoevolveStatus::Cycle { period } => {
            let _ = writeln!(out, "**Outcome: 2-cycle detected (period {period}).**");
            let _ = writeln!(
                out,
                "The joint state is oscillating between two configurations rather than settling. This typically means a finer search granularity (`--coevolve-trials` or per-variable `steps`) would surface a stable midpoint, *or* the underlying preference structure has no pure-strategy equilibrium at this granularity. Examine the round table to see the alternation pattern."
            );
        },
        CoevolveStatus::NoEquilibrium => {
            let _ = writeln!(
                out,
                "**Outcome: no equilibrium found within the round budget.**"
            );
            let _ = writeln!(
                out,
                "The loop hit `max_rounds` without convergence or a detected 2-cycle. Possible reasons: (a) the objective landscape is genuinely non-stationary; (b) a higher-period cycle (>2) is in play and the round-two detector misses it; (c) the round budget is too small. Try `--coevolve-rounds` 2-4× higher; if the result still doesn't converge, the strategy structure itself may be misspecified."
            );
        },
    }
    let _ = writeln!(out);

    // Round trajectory. One markdown table; each row is one mover's
    // best response. Long path / value lists fold into the assignments
    // column joined by `; ` so the table stays scannable. (The full
    // structured per-round detail lives in `coevolve.json` for
    // analysts who want to drill in.)
    let _ = writeln!(out, "## Round trajectory");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Round | Mover | Mover assignments | Objective | Value |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|");
    for round in &result.rounds {
        let mover_label = match round.mover {
            CoevolveSide::Attacker => format!("attacker `{}`", result.attacker_faction),
            CoevolveSide::Defender => format!("defender `{}`", result.defender_faction),
        };
        let assignments = if round.mover_assignments.is_empty() {
            "_(no assignment)_".to_string()
        } else {
            round
                .mover_assignments
                .iter()
                .map(|ov| format!("`{}` = {:.4}", ov.path, ov.value))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let _ = writeln!(
            out,
            "| {round_num} | {mover} | {assignments} | `{label}` | {value:.4} |",
            round_num = round.round,
            mover = mover_label,
            assignments = assignments,
            label = round.mover_objective_label,
            value = round.mover_objective_value,
        );
    }
    let _ = writeln!(out);

    // Final joint state — the equilibrium (or last-attempted) posture.
    let _ = writeln!(out, "## Final joint state");
    let _ = writeln!(out);
    if result.final_attacker_assignments.is_empty() {
        let _ = writeln!(
            out,
            "_Attacker did not move in this run (no rounds reached the attacker's turn)._"
        );
    } else {
        let _ = writeln!(out, "**Attacker `{}`:**", result.attacker_faction);
        for ov in &result.final_attacker_assignments {
            let _ = writeln!(out, "- `{}` = **{:.4}**", ov.path, ov.value);
        }
    }
    let _ = writeln!(out);
    if result.final_defender_assignments.is_empty() {
        let _ = writeln!(
            out,
            "_Defender did not move in this run (no rounds reached the defender's turn)._"
        );
    } else {
        let _ = writeln!(out, "**Defender `{}`:**", result.defender_faction);
        for ov in &result.final_defender_assignments {
            let _ = writeln!(out, "- `{}` = **{:.4}**", ov.path, ov.value);
        }
    }
    let _ = writeln!(out);

    // Per-side objective at the equilibrium.
    let _ = writeln!(out, "## Equilibrium objective values");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Objective | Value |");
    let _ = writeln!(out, "|---|---|");
    for (label, value) in &result.final_objective_values {
        let _ = writeln!(out, "| `{label}` | {value:.4} |");
    }
    let _ = writeln!(out);

    // Reproducibility footer. Co-evolution is double-seeded
    // (coevolve_seed + mc_seed); spell it out so the report is
    // self-documenting for later replays.
    let _ = writeln!(out, "## Reproducibility");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Replay this run via `faultline <scenario.toml> --verify manifest.json`. The manifest records `coevolve_seed` (drives per-round sub-search sampling) and `mc_config.base_seed` (drives the inner Monte Carlo evaluation); both must match for bit-identical replay."
    );
    let _ = writeln!(out);

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use faultline_types::campaign::{CampaignPhase, KillChain, PhaseCost};
    use faultline_types::ids::{FactionId, KillChainId, PhaseId};
    use faultline_types::map::{MapConfig, MapSource};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::{Scenario, ScenarioMeta};
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::stats::MonteCarloSummary;

    fn empty_summary() -> MonteCarloSummary {
        MonteCarloSummary {
            total_runs: 0,
            win_rates: BTreeMap::new(),
            win_rate_cis: BTreeMap::new(),
            average_duration: 0.0,
            metric_distributions: BTreeMap::new(),
            regional_control: BTreeMap::new(),
            event_probabilities: BTreeMap::new(),
            campaign_summaries: BTreeMap::new(),
            feasibility_matrix: Vec::new(),
            seam_scores: BTreeMap::new(),
            correlation_matrix: None,
            pareto_frontier: None,
            defender_capacity: Vec::new(),
        }
    }

    fn minimal_scenario() -> Scenario {
        Scenario {
            meta: ScenarioMeta {
                name: "Report Test".into(),
                description: "description".into(),
                author: "test".into(),
                version: "0.0.1".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions: BTreeMap::new(),
                infrastructure: BTreeMap::new(),
                terrain: vec![],
            },
            factions: BTreeMap::new(),
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.0,
                institutional_trust: 0.5,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.7,
                    internet_availability: 0.8,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 1,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 1,
                seed: Some(0),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 0,
            },
            victory_conditions: BTreeMap::new(),
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: faultline_types::strategy_space::StrategySpace::default(),
        }
    }

    #[test]
    fn report_always_includes_methodology() {
        let summary = empty_summary();
        let scenario = minimal_scenario();
        let md = render_markdown(&summary, &scenario);
        assert!(
            md.contains("## Methodology & Confidence"),
            "methodology section should always be present; got:\n{md}"
        );
        assert!(
            md.contains("Wilson score interval"),
            "methodology should mention Wilson; got:\n{md}"
        );
    }

    #[test]
    fn report_shows_win_rate_cis_when_present() {
        let mut summary = empty_summary();
        let fid = FactionId::from("gov");
        summary.total_runs = 100;
        summary.win_rates.insert(fid.clone(), 0.62);
        summary
            .win_rate_cis
            .insert(fid, ConfidenceInterval::new(0.62, 0.52, 0.71, 100));
        let md = render_markdown(&summary, &minimal_scenario());
        assert!(
            md.contains("95% CI"),
            "win-rate table should have CI column"
        );
        assert!(
            md.contains("52.0% – 71.0%"),
            "formatted CI should be present, got:\n{md}"
        );
    }

    #[test]
    fn report_lists_author_flagged_parameters() {
        let mut scenario = minimal_scenario();
        let chain_id = KillChainId::from("alpha");
        let phase_id = PhaseId::from("recon");
        let mut phases: BTreeMap<PhaseId, CampaignPhase> = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Recon".into(),
                description: "".into(),
                prerequisites: vec![],
                base_success_probability: 0.8,
                min_duration: 1,
                max_duration: 2,
                detection_probability_per_tick: 0.1,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 1_000.0,
                    defender_dollars: 10_000.0,
                    attacker_resources: 0.0,
                    confidence: Some(ConfidenceLevel::Low),
                },
                targets_domains: vec![],
                outputs: vec![],
                branches: vec![],
                parameter_confidence: Some(ConfidenceLevel::Low),
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );
        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Alpha Campaign".into(),
                description: "".into(),
                attacker: FactionId::from("red"),
                target: FactionId::from("blue"),
                entry_phase: phase_id,
                phases,
            },
        );
        let md = render_markdown(&empty_summary(), &scenario);
        assert!(
            md.contains("Author-Flagged Low-Confidence Parameters"),
            "should include low-confidence section"
        );
        assert!(md.contains("Alpha Campaign"), "should reference chain name");
        assert!(
            md.contains("phase parameters") && md.contains("phase cost"),
            "should describe both flag kinds; got:\n{md}"
        );
    }

    #[test]
    fn report_omits_flagged_section_when_no_flags() {
        let md = render_markdown(&empty_summary(), &minimal_scenario());
        assert!(
            !md.contains("Author-Flagged Low-Confidence Parameters"),
            "section should be elided when nothing is flagged"
        );
    }

    #[test]
    fn escape_md_cell_neutralizes_pipes_and_newlines() {
        // Bare strings: pipe must be escaped, newlines collapsed, backslash
        // doubled so the escape itself is not ambiguous, backticks escaped
        // so an unbalanced one can't open an inline code span that bleeds
        // into adjacent cells.
        assert_eq!(escape_md_cell("a|b"), r"a\|b");
        assert_eq!(escape_md_cell("line1\nline2"), "line1 line2");
        assert_eq!(escape_md_cell("line1\r\nline2"), "line1  line2");
        assert_eq!(escape_md_cell(r"back\slash"), r"back\\slash");
        assert_eq!(escape_md_cell("a`b"), r"a\`b");
        assert_eq!(escape_md_cell("clean"), "clean");
    }
}
