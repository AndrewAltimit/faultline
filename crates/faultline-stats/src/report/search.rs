//! Strategy-search Markdown report: setup block, best-by-objective
//! table, Pareto-frontier table, the Counter-Recommendation block,
//! then per-trial detail.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::strategy_space::SearchObjective;

use crate::search::{SearchMethod, SearchResult, SearchTrial};

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
// Counter-Recommendation
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
        // first slice ships clean.
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
