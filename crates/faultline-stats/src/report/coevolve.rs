//! Co-evolution Markdown report (Epic H — round two): convergence
//! callout, round trajectory, final joint state, equilibrium objective
//! values, reproducibility footer.

use std::fmt::Write;

use faultline_types::scenario::Scenario;

use crate::coevolve::{CoevolveResult, CoevolveSide, CoevolveStatus};

/// Render a co-evolution Markdown report from a [`CoevolveResult`].
///
/// Top section explains the convergence outcome (Converged / Cycle /
/// NoEquilibrium); the round trajectory section walks each round with
/// the mover, the chosen assignments, and the objective value; the
/// final block shows the equilibrium joint state and the resulting
/// objective values for both sides.
pub fn render_coevolve_markdown(result: &CoevolveResult, scenario: &Scenario) -> String {
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
            let _ = writeln!(
                out,
                "**Outcome: cycle detected (joint-state period {period}).**"
            );
            let _ = writeln!(
                out,
                "The joint state is oscillating rather than settling — round N's `(attacker, defender)` state matched round N-{period}'s. Note that in alternating-mover play the smallest possible period is 4 (a 2-cycle in each side's own history corresponds to a 4-cycle in the joint state). Typical fixes: (a) finer search granularity (`--coevolve-trials` or per-variable `steps`) may surface a stable midpoint between the cycle's vertices, (b) the underlying preference structure may genuinely have no pure-strategy equilibrium at this granularity. Examine the round table to see the alternation pattern."
            );
        },
        CoevolveStatus::NoEquilibrium => {
            let _ = writeln!(
                out,
                "**Outcome: no equilibrium found within the round budget.**"
            );
            let _ = writeln!(
                out,
                "The loop hit `max_rounds` without convergence or a detected cycle. Possible reasons: (a) the objective landscape is genuinely non-stationary; (b) the cycle period is longer than the rounds executed (the detector needs at least one full repeat to flag a cycle); (c) the round budget is too small. Try `--coevolve-rounds` 2-4× higher; if the result still doesn't converge, the strategy structure itself may be misspecified."
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
