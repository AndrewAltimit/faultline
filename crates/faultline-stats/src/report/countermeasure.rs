//! Countermeasure Analysis section (Epic B): per-phase warning
//! indicators (IWI / IOC entries).
//!
//! Declarative in this iteration: detection probability in the engine
//! is still driven by `CampaignPhase.detection_probability_per_tick`.
//! The section exists to make the *monitoring posture* required to
//! hit that rate concrete, so analysts can reason about whether the
//! assumed detection rate is credibly achievable.
//!
//! Elided when no chain has any phase tagged with warning indicators.

use std::fmt::Write;

use faultline_types::campaign::{CampaignPhase, KillChain, ObservableDiscipline, WarningIndicator};
use faultline_types::ids::PhaseId;
use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;
use super::util::escape_md_cell;

pub(super) struct Countermeasure;

impl ReportSection for Countermeasure {
    fn render(&self, _summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
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
