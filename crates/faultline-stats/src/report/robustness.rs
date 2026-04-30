//! Robustness Markdown report (Epic I — round two): per-posture
//! rollup tables (worst / best / mean across profiles) and an explicit
//! (posture × profile) cell table per objective.
//!
//! The narrative is "rank postures by worst-case profile" — that's the
//! analyst's question. Mean and best columns are present so a posture
//! that's robust on average but fragile to one specific profile shows
//! up clearly via a large worst-vs-mean gap.

use std::fmt::Write;

use faultline_types::scenario::Scenario;

use crate::robustness::RobustnessResult;

use super::util::escape_md_cell;

/// Render a Markdown report of a robustness analysis.
pub fn render_robustness_markdown(result: &RobustnessResult, scenario: &Scenario) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Faultline Robustness Report");
    let _ = writeln!(out);
    let _ = writeln!(out, "## Scenario: {}", scenario.meta.name);
    let _ = writeln!(out);

    let _ = writeln!(out, "- **Postures evaluated:** {}", result.postures.len());
    let _ = writeln!(out, "- **Attacker profiles:** {}", result.profiles.len());
    let _ = writeln!(out, "- **Objectives:** {}", result.objectives.len());
    if let Some(ref baseline) = result.baseline_label {
        let _ = writeln!(
            out,
            "- **Baseline anchor:** `{}` (natural-state defender)",
            baseline
        );
    }
    let _ = writeln!(out);

    if result.cells.is_empty() {
        let _ = writeln!(
            out,
            "_No cells evaluated; nothing to report. Check `[strategy_space.attacker_profiles]` is non-empty and at least one posture or `--robustness-skip-baseline=false`._"
        );
        return out;
    }

    // Profile metadata table — useful so an analyst reading the rollup
    // tables can match a profile name back to what it actually changes.
    let _ = writeln!(out, "## Attacker profiles");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Profile | Description | Assignments |");
    let _ = writeln!(out, "|---|---|---|");
    for p in &result.profiles {
        let desc = if p.description.is_empty() {
            "—".to_string()
        } else {
            escape_md_cell(&p.description)
        };
        let _ = writeln!(
            out,
            "| `{name}` | {desc} | {n} |",
            name = p.name,
            desc = desc,
            n = p.assignment_count,
        );
    }
    let _ = writeln!(out);

    // Per-objective rollup tables. One section per objective so a long
    // objectives list still reads cleanly. Each row is a posture; each
    // column is worst / best / mean / stdev with the profile name in
    // parens for worst/best.
    for objective in &result.objectives {
        let label = objective.label();
        let direction = if objective.maximize() { "max" } else { "min" };
        let _ = writeln!(out, "## Per-posture rollup: `{label}` ({direction})");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Per-posture aggregate of objective `{label}` across attacker profiles. \"Worst\" is direction-aware: for a maximize objective it's the smallest cell value (defender does poorly); for a minimize objective it's the largest. A large worst-vs-mean gap signals a posture that is sensitive to which attacker profile it faces."
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Posture | Worst | Worst profile | Best | Best profile | Mean | Stdev |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|");
        for rollup in &result.rollups {
            let worst = rollup.worst_per_objective.get(&label);
            let best = rollup.best_per_objective.get(&label);
            // `compute_rollups` always inserts mean/stdev for every
            // objective alongside worst/best, so a missing key here
            // would be a logic error in the runner — surface it
            // instead of silently rendering 0.
            let mean = rollup
                .mean_per_objective
                .get(&label)
                .copied()
                .expect("mean_per_objective missing key populated by compute_rollups");
            let stdev = rollup
                .stdev_per_objective
                .get(&label)
                .copied()
                .expect("stdev_per_objective missing key populated by compute_rollups");
            let (worst_v, worst_p) = match worst {
                Some(nv) => (format!("{:.4}", nv.value), nv.profile_name.clone()),
                None => ("—".to_string(), "—".to_string()),
            };
            let (best_v, best_p) = match best {
                Some(nv) => (format!("{:.4}", nv.value), nv.profile_name.clone()),
                None => ("—".to_string(), "—".to_string()),
            };
            let _ = writeln!(
                out,
                "| `{posture}` | {worst_v} | `{worst_p}` | {best_v} | `{best_p}` | {mean:.4} | {stdev:.4} |",
                posture = rollup.posture_label,
            );
        }
        let _ = writeln!(out);
    }

    // Full (posture × profile) cell tables per objective. The rollup
    // already covers the analyst question; this section is for drilling
    // in when a specific (posture, profile) combination needs context.
    // Skipped when the matrix is large to keep the report short — over
    // 64 cells per objective (8×8) we elide and point at the JSON.
    let cells_per_objective = result.postures.len() * result.profiles.len();
    if cells_per_objective <= 64 {
        for objective in &result.objectives {
            let label = objective.label();
            let _ = writeln!(out, "## Cell matrix: `{label}`");
            let _ = writeln!(out);
            let mut header = String::from("| Posture |");
            let mut rule = String::from("|---|");
            for p in &result.profiles {
                header.push_str(&format!(" `{}` |", p.name));
                rule.push_str("---|");
            }
            let _ = writeln!(out, "{header}");
            let _ = writeln!(out, "{rule}");
            for (pi, posture) in result.postures.iter().enumerate() {
                let mut row = format!("| `{}` |", posture.label);
                for (qi, _) in result.profiles.iter().enumerate() {
                    let cell_idx = pi * result.profiles.len() + qi;
                    let v = result
                        .cells
                        .get(cell_idx)
                        .and_then(|c| c.objective_values.get(&label).copied())
                        .unwrap_or(f64::NAN);
                    if v.is_nan() {
                        row.push_str(" — |");
                    } else {
                        row.push_str(&format!(" {:.4} |", v));
                    }
                }
                let _ = writeln!(out, "{row}");
            }
            let _ = writeln!(out);
        }
    } else {
        let _ = writeln!(
            out,
            "_Cell matrix elided — `{cells_per_objective}` cells per objective is too dense for Markdown. Drill into `robustness.json` for per-cell objective values and full Monte Carlo summaries._"
        );
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Reproducibility");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Robustness has no RNG of its own — the cross-product is iterated deterministically. Replay this run via `faultline <scenario.toml> --verify manifest.json`. The manifest records the inner Monte Carlo seed plus the full posture list (and, when present, the SHA-256 of the source `search.json`); changing any of them flips the output hash."
    );
    let _ = writeln!(out);

    out
}
