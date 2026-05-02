//! Network Resilience section: per-network mean / max disrupted-node
//! and component counts, fragmentation rate, and the Brandes
//! critical-node ranking on the static topology.
//!
//! Elided when the scenario declares no networks.

use std::fmt::Write;

use faultline_types::scenario::Scenario;
use faultline_types::stats::MonteCarloSummary;

use super::ReportSection;

pub(super) struct NetworkResilience;

impl ReportSection for NetworkResilience {
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String) {
        if summary.network_summaries.is_empty() {
            return;
        }
        let _ = writeln!(out, "## Network Resilience");
        let _ = writeln!(
            out,
            "Per-network resilience across the {} runs in the batch. The fragmentation rate is the fraction of runs that ended with at least one disrupted node; the critical-node ranking is Brandes betweenness centrality on the static topology and answers \"which node is most painful to remove regardless of who removes it\".",
            summary.total_runs
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Network | Kind | Owner | Nodes | Edges | Mean disrupted | Max disrupted | Mean components | Fragmentation |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
        for (nid, ns) in &summary.network_summaries {
            let net = scenario.networks.get(nid);
            let kind = net.map(|n| n.kind.as_str()).unwrap_or("");
            let owner = net
                .and_then(|n| n.owner.as_ref())
                .map(|f| f.0.as_str())
                .unwrap_or("—");
            let nodes = net.map(|n| n.nodes.len()).unwrap_or(0);
            let edges = net.map(|n| n.edges.len()).unwrap_or(0);
            let _ = writeln!(
                out,
                "| `{}` | {} | `{}` | {} | {} | {:.2} | {} | {:.2} | {:.1}% |",
                nid,
                kind,
                owner,
                nodes,
                edges,
                ns.mean_disrupted_nodes,
                ns.max_disrupted_nodes,
                ns.mean_terminal_components,
                ns.fragmentation_rate * 100.0,
            );
        }
        let _ = writeln!(out);

        // Critical-node ranking per network.
        for (nid, ns) in &summary.network_summaries {
            if ns.critical_nodes.is_empty() {
                continue;
            }
            let _ = writeln!(
                out,
                "**`{nid}` critical nodes (top {}):**",
                ns.critical_nodes.len()
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "| Rank | Node | Betweenness | Author criticality |");
            let _ = writeln!(out, "|---|---|---|---|");
            for (i, c) in ns.critical_nodes.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "| {} | `{}` ({}) | {:.4} | {:.2} |",
                    i + 1,
                    c.node,
                    c.name,
                    c.betweenness,
                    c.criticality,
                );
            }
            let _ = writeln!(out);
        }

        // Per-faction infiltration rollup is computed at the runs level
        // (see `network_metrics::mean_infiltration_per_faction`) and
        // ships in the JSON manifest. A future revision can either widen
        // `MonteCarloSummary` with the per-faction map or thread `&[RunResult]`
        // into `render_markdown`.
    }
}
