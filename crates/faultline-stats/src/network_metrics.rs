//! Deterministic graph metrics on declared networks.
//!
//! Three metric families:
//!
//! - **Connectivity** — number of weakly-connected components and
//!   the largest-component size, computed by the engine on every
//!   tick and folded into [`NetworkSample`]. The cross-run rollup
//!   here surfaces mean / max disrupted-node and component counts
//!   plus the fraction of runs that fragmented at all.
//!
//! - **Max-flow / min-cut** — Edmonds-Karp (BFS-based Ford-Fulkerson
//!   with capacity scaling skipped) over the static topology. The
//!   minimum cut is reported as the saturated edges in the residual
//!   graph at termination — each saturated edge is a single-edge
//!   removal that fragments the source from the sink.
//!
//! - **Betweenness centrality** — Brandes O(VE) algorithm. Returns
//!   normalized scores per node so the report can rank "structural
//!   single points of failure" deterministically.
//!
//! All three are pure functions of the static topology plus
//! per-tick runtime mutations; no RNG, no HashMap iteration.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use faultline_types::ids::{EdgeId, FactionId, NetworkId, NodeId};
use faultline_types::network::Network;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{CriticalNode, NetworkSummary, RunResult};

/// Cap on how many critical nodes the report surfaces per network.
/// Bounded so the rendered table stays readable even on large
/// networks. The full ranking is recoverable from the manifest's
/// `summary.json` — this is purely a rendering choice.
const CRITICAL_NODES_CAP: usize = 10;

/// Compute the per-network cross-run summary.
///
/// Returns an empty map when `scenario.networks.is_empty()` so legacy
/// scenarios pay zero overhead. For each declared network, aggregates
/// per-run mutations (mean / max disrupted-node and component counts,
/// fragmentation rate) and computes the static-topology Brandes
/// betweenness ranking once (it doesn't depend on runtime state).
pub fn compute_network_summaries(
    runs: &[RunResult],
    scenario: &Scenario,
) -> BTreeMap<NetworkId, NetworkSummary> {
    let mut out = BTreeMap::new();
    if scenario.networks.is_empty() {
        return out;
    }

    let n_runs = u32::try_from(runs.len()).expect("MC run count exceeds u32::MAX");
    let n_runs_f = f64::from(n_runs).max(1.0);

    for (nid, net) in &scenario.networks {
        let mut sum_disrupted = 0.0_f64;
        let mut max_disrupted: u32 = 0;
        let mut sum_components = 0.0_f64;
        let mut max_components: u32 = 0;
        let mut runs_with_disruption = 0_u32;

        for run in runs {
            let report = match run.network_reports.get(nid) {
                Some(r) => r,
                None => continue,
            };
            let terminal_disrupted = u32::try_from(report.terminal_disrupted_nodes.len())
                .expect("terminal_disrupted_nodes count exceeds u32::MAX");
            sum_disrupted += f64::from(terminal_disrupted);
            if terminal_disrupted > max_disrupted {
                max_disrupted = terminal_disrupted;
            }
            if terminal_disrupted > 0 {
                runs_with_disruption += 1;
            }
            // Terminal component count: read the last sample if any.
            if let Some(last) = report.samples.last() {
                sum_components += f64::from(last.component_count);
                if last.component_count > max_components {
                    max_components = last.component_count;
                }
            }
        }

        let mean_disrupted = sum_disrupted / n_runs_f;
        let mean_components = sum_components / n_runs_f;
        let fragmentation_rate = f64::from(runs_with_disruption) / n_runs_f;

        let critical_nodes = brandes_top_critical(net, CRITICAL_NODES_CAP);

        out.insert(
            nid.clone(),
            NetworkSummary {
                network: nid.clone(),
                n_runs,
                mean_disrupted_nodes: mean_disrupted,
                max_disrupted_nodes: max_disrupted,
                mean_terminal_components: mean_components,
                max_terminal_components: max_components,
                fragmentation_rate,
                critical_nodes,
            },
        );
    }

    out
}

/// Brandes betweenness centrality on the *static* topology, treating
/// the network as undirected for centrality purposes (a directed
/// betweenness would need to choose a direction convention, and for
/// resilience the symmetric measure is more meaningful — we want to
/// know which node is most painful to remove regardless of flow
/// direction). Returns the top `cap` nodes ranked by descending
/// score; ties resolve by `BTreeMap` key order so the output is
/// deterministic across platforms.
pub fn brandes_top_critical(net: &Network, cap: usize) -> Vec<CriticalNode> {
    let n = net.nodes.len();
    if n < 2 {
        return Vec::new();
    }

    // Stable index mapping over the BTreeMap ordering.
    let nodes: Vec<NodeId> = net.nodes.keys().cloned().collect();
    let index: BTreeMap<NodeId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();

    // Undirected adjacency. Self-loops are filtered (validation
    // rejects them at scenario load, but defensive here).
    let mut adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for edge in net.edges.values() {
        let (Some(&u), Some(&v)) = (index.get(&edge.from), index.get(&edge.to)) else {
            continue;
        };
        if u == v {
            continue;
        }
        adj[u].insert(v);
        adj[v].insert(u);
    }

    let mut cb = vec![0.0_f64; n];

    // Standard Brandes (2001): single-source shortest-path BFS;
    // back-propagate dependencies. O(V * (V + E)) on unweighted graphs.
    for s in 0..n {
        let mut stack: Vec<usize> = Vec::new();
        let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
        // f64 (not u64) for sigma is the standard Brandes practice:
        // shortest-path counts can grow factorially on graphs like
        // hypercubes (Q_d has d! shortest paths between antipodes), so
        // an integer counter would silently overflow on Q_21+. f64
        // tolerates the dynamic range and the only Brandes operation
        // on sigma is division (sigma[v] / sigma[w]), where the
        // mantissa is more than enough.
        let mut sigma = vec![0.0_f64; n];
        sigma[s] = 1.0;
        let mut dist = vec![-1_i64; n];
        dist[s] = 0;

        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(s);
        while let Some(v) = queue.pop_front() {
            stack.push(v);
            for &w in &adj[v] {
                if dist[w] < 0 {
                    dist[w] = dist[v] + 1;
                    queue.push_back(w);
                }
                if dist[w] == dist[v] + 1 {
                    sigma[w] += sigma[v];
                    pred[w].push(v);
                }
            }
        }

        let mut delta = vec![0.0_f64; n];
        while let Some(w) = stack.pop() {
            for &v in &pred[w] {
                let contribution = (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                delta[v] += contribution;
            }
            if w != s {
                cb[w] += delta[w];
            }
        }
    }

    // Standard normalization for undirected betweenness, matching
    // NetworkX `betweenness_centrality(normalized=True)`. The raw cb
    // accumulated above already counts each unordered pair {s, t}
    // twice (once with s as source, once with t as source), and the
    // conventional `2 / ((n - 1) * (n - 2))` factor halves that into
    // the per-pair share. Equivalently — and what the code does —
    // divide the doubled raw cb by `(n - 1) * (n - 2)`. The result
    // lives in `[0, 1]`; an undirected star centre attains exactly
    // 1.0. Widening to u64 before multiplying avoids silent wraparound
    // on WASM32 (32-bit usize) for `n >= 65538`.
    let denom = ((n as u64 - 1) * (n as u64 - 2)) as f64;
    if denom > 0.0 {
        for v in cb.iter_mut() {
            *v /= denom;
        }
    }

    // Build (score, idx) pairs and sort by descending score, then by
    // ascending idx for ties — `BTreeMap` index order is canonical.
    let mut ranked: Vec<(usize, f64)> = (0..n).map(|i| (i, cb[i])).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    ranked
        .into_iter()
        .take(cap.min(n))
        .map(|(idx, score)| {
            let nid = &nodes[idx];
            // Safe: nid was constructed from net.nodes.keys().
            let node = &net.nodes[nid];
            CriticalNode {
                node: nid.clone(),
                name: node.name.clone(),
                betweenness: score,
                criticality: node.criticality,
            }
        })
        .collect()
}

/// Edmonds-Karp max-flow from `source` to `sink` on the static
/// topology with edge capacities multiplied by their runtime factors.
///
/// Treats the network as a directed graph (edges flow from `from` to
/// `to` only — authors who want bidirectional capacity declare both
/// directions). Returns:
///
/// - `flow` — the total max flow (units / tick),
/// - `min_cut` — the saturated edges in the residual graph at
///   termination (one canonical min-cut, sorted by `EdgeId` for
///   reproducibility).
///
/// `None` when source or sink is missing from the network or when
/// they are the same node. `Some` with `flow == 0.0` when there is
/// no path from source to sink (the min-cut is the empty set in that
/// case — there's nothing to cut).
pub fn max_flow(
    net: &Network,
    source: &NodeId,
    sink: &NodeId,
    edge_factors: &BTreeMap<EdgeId, f64>,
    disrupted: &BTreeSet<NodeId>,
) -> Option<MaxFlowResult> {
    if source == sink {
        return None;
    }
    if !net.nodes.contains_key(source) || !net.nodes.contains_key(sink) {
        return None;
    }

    // Stable index mapping. Edmonds-Karp benefits from index-based
    // adjacency arrays for the BFS.
    let nodes: Vec<NodeId> = net.nodes.keys().cloned().collect();
    let index: BTreeMap<NodeId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let n = nodes.len();
    let s = index[source];
    let t = index[sink];

    // For each (u, v) pair we need a residual capacity. We store this
    // as a Vec<BTreeMap<usize, f64>> for deterministic iteration and
    // to handle duplicate edges (two parallel edges of capacity 5
    // each combine to capacity 10 as expected). Reverse residual
    // entries start at 0; standard FF augmentation.
    let mut residual: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    // Track which forward edges contributed each residual entry so we
    // can return min-cut edges by EdgeId at the end.
    let mut forward_edges: BTreeMap<(usize, usize), Vec<EdgeId>> = BTreeMap::new();

    for (eid, edge) in &net.edges {
        let (Some(&u), Some(&v)) = (index.get(&edge.from), index.get(&edge.to)) else {
            continue;
        };
        if u == v {
            continue;
        }
        if disrupted.contains(&edge.from) || disrupted.contains(&edge.to) {
            continue;
        }
        let factor = edge_factors.get(eid).copied().unwrap_or(1.0);
        let effective = edge.capacity * factor;
        if effective <= 0.0 {
            continue;
        }
        *residual[u].entry(v).or_insert(0.0) += effective;
        residual[v].entry(u).or_insert(0.0); // ensure reverse exists
        forward_edges.entry((u, v)).or_default().push(eid.clone());
    }

    let mut flow = 0.0_f64;
    loop {
        // BFS for shortest augmenting path in residual graph.
        let mut parent: Vec<Option<usize>> = vec![None; n];
        parent[s] = Some(s); // sentinel
        let mut queue: VecDeque<usize> = VecDeque::new();
        queue.push_back(s);
        while let Some(u) = queue.pop_front() {
            if u == t {
                break;
            }
            // BTreeMap iteration is ordered, so the BFS picks the
            // lexicographically smallest next-node, making path
            // selection deterministic.
            for (&v, &cap) in &residual[u] {
                if parent[v].is_none() && cap > 0.0 {
                    parent[v] = Some(u);
                    queue.push_back(v);
                }
            }
        }
        if parent[t].is_none() {
            break;
        }
        // Trace path and find bottleneck.
        let mut path_min = f64::INFINITY;
        let mut cur = t;
        while cur != s {
            let p = parent[cur].expect("parent[s] = s sentinel; intermediate nodes set by BFS");
            let cap = residual[p][&cur];
            if cap < path_min {
                path_min = cap;
            }
            cur = p;
        }
        // Augment.
        let mut cur = t;
        while cur != s {
            let p = parent[cur].expect("parent[s] = s sentinel; intermediate nodes set by BFS");
            *residual[p].get_mut(&cur).expect("verified above") -= path_min;
            *residual[cur].entry(p).or_insert(0.0) += path_min;
            cur = p;
        }
        flow += path_min;
    }

    // Min-cut: BFS from source in residual graph; reachable side is
    // the source-side of the cut. Edges from reachable to
    // un-reachable that were originally forward edges are the cut.
    let mut reachable: BTreeSet<usize> = BTreeSet::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(s);
    reachable.insert(s);
    while let Some(u) = queue.pop_front() {
        for (&v, &cap) in &residual[u] {
            if cap > 0.0 && reachable.insert(v) {
                queue.push_back(v);
            }
        }
    }
    let mut cut_edges: BTreeSet<EdgeId> = BTreeSet::new();
    for (&(u, v), eids) in &forward_edges {
        if reachable.contains(&u) && !reachable.contains(&v) {
            for eid in eids {
                cut_edges.insert(eid.clone());
            }
        }
    }

    Some(MaxFlowResult {
        flow,
        min_cut: cut_edges.into_iter().collect(),
    })
}

/// Result of [`max_flow`] — total flow and one canonical min-cut.
#[derive(Clone, Debug, PartialEq)]
pub struct MaxFlowResult {
    pub flow: f64,
    /// Min-cut edges, sorted by `EdgeId` for determinism. May be
    /// empty when source and sink are already disconnected (flow is
    /// zero and there is no edge to cut).
    pub min_cut: Vec<EdgeId>,
}

/// Aggregate the per-faction infiltration footprint across runs.
///
/// Returns a map from faction to mean count of infiltrated nodes
/// per run on a given network. Used by the report to surface which
/// factions ended up with the most network visibility.
pub fn mean_infiltration_per_faction(
    runs: &[RunResult],
    network: &NetworkId,
) -> BTreeMap<FactionId, f64> {
    let n = runs.len();
    if n == 0 {
        return BTreeMap::new();
    }
    let n_f = n as f64;
    let mut totals: BTreeMap<FactionId, u64> = BTreeMap::new();
    for run in runs {
        let Some(report) = run.network_reports.get(network) else {
            continue;
        };
        for (faction, nodes) in &report.terminal_infiltrated {
            let count =
                u64::try_from(nodes.len()).expect("infiltrated node count exceeds u64::MAX");
            let entry = totals.entry(faction.clone()).or_insert(0);
            *entry = entry.saturating_add(count);
        }
    }
    totals
        .into_iter()
        .map(|(f, total)| (f, total as f64 / n_f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::network::{NetworkEdge, NetworkNode};

    fn make_diamond() -> Network {
        // s -> a -> t
        // s -> b -> t
        // capacity 5 on each edge => max flow 10, min cut = either
        // both s->* edges or both *->t edges.
        let mut nodes = BTreeMap::new();
        for id in ["s", "a", "b", "t"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        for (eid, from, to) in [
            ("sa", "s", "a"),
            ("sb", "s", "b"),
            ("at", "a", "t"),
            ("bt", "b", "t"),
        ] {
            edges.insert(
                EdgeId::from(eid),
                NetworkEdge {
                    id: EdgeId::from(eid),
                    from: NodeId::from(from),
                    to: NodeId::from(to),
                    capacity: 5.0,
                    ..Default::default()
                },
            );
        }
        Network {
            id: NetworkId::from("test"),
            name: "Test".into(),
            nodes,
            edges,
            ..Default::default()
        }
    }

    #[test]
    fn max_flow_diamond_is_ten() {
        let net = make_diamond();
        let res = max_flow(
            &net,
            &NodeId::from("s"),
            &NodeId::from("t"),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .expect("source / sink defined");
        assert!((res.flow - 10.0).abs() < 1e-9);
        // Min-cut on the source side: {sa, sb}. Output is sorted by
        // EdgeId so the contents are deterministic.
        assert_eq!(res.min_cut.len(), 2);
        let names: Vec<String> = res.min_cut.iter().map(|e| e.0.clone()).collect();
        assert_eq!(names, vec!["sa".to_string(), "sb".to_string()]);
    }

    #[test]
    fn max_flow_zeroed_edge_reduces_flow() {
        let net = make_diamond();
        let mut factors = BTreeMap::new();
        factors.insert(EdgeId::from("sa"), 0.0);
        let res = max_flow(
            &net,
            &NodeId::from("s"),
            &NodeId::from("t"),
            &factors,
            &BTreeSet::new(),
        )
        .expect("source / sink defined");
        assert!((res.flow - 5.0).abs() < 1e-9);
    }

    #[test]
    fn max_flow_disrupted_intermediate_severs_path() {
        let net = make_diamond();
        let mut disrupted = BTreeSet::new();
        disrupted.insert(NodeId::from("a"));
        disrupted.insert(NodeId::from("b"));
        let res = max_flow(
            &net,
            &NodeId::from("s"),
            &NodeId::from("t"),
            &BTreeMap::new(),
            &disrupted,
        )
        .expect("source / sink defined");
        assert!((res.flow - 0.0).abs() < 1e-9);
        // No reachable forward edges => empty min-cut.
        assert!(res.min_cut.is_empty());
    }

    #[test]
    fn max_flow_unknown_endpoints_returns_none() {
        let net = make_diamond();
        assert!(
            max_flow(
                &net,
                &NodeId::from("nope"),
                &NodeId::from("t"),
                &BTreeMap::new(),
                &BTreeSet::new(),
            )
            .is_none()
        );
        // Same node => None.
        assert!(
            max_flow(
                &net,
                &NodeId::from("s"),
                &NodeId::from("s"),
                &BTreeMap::new(),
                &BTreeSet::new(),
            )
            .is_none()
        );
    }

    #[test]
    fn brandes_star_center_dominates() {
        // Star: center connected to 4 leaves. Center should have
        // betweenness 1.0 (every non-trivial path goes through it).
        let mut nodes = BTreeMap::new();
        for id in ["c", "a", "b", "d", "e"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        for (i, leaf) in ["a", "b", "d", "e"].iter().enumerate() {
            edges.insert(
                EdgeId::from(format!("c{leaf}").as_str()),
                NetworkEdge {
                    id: EdgeId::from(format!("c{leaf}").as_str()),
                    from: NodeId::from("c"),
                    to: NodeId::from(*leaf),
                    capacity: 1.0,
                    ..Default::default()
                },
            );
            // Force capacity field actually used (silence unused warn).
            let _ = i;
        }
        let net = Network {
            id: NetworkId::from("star"),
            name: "Star".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let ranked = brandes_top_critical(&net, 5);
        assert_eq!(ranked[0].node, NodeId::from("c"));
        // The star center sits on every leaf-to-leaf shortest path. For
        // n = 5 leaves count = 4, ordered (s, t) pairs of leaves = 12,
        // and the raw Brandes cb (run from every source — undirected
        // graphs double-count pairs) accumulates 12. Normalized by the
        // standard `(n - 1) * (n - 2) = 12` the score is exactly 1.0,
        // i.e. the maximum centrality reachable on any undirected
        // graph.
        assert!(
            (ranked[0].betweenness - 1.0).abs() < 1e-9,
            "star center should normalize to 1.0, got {}",
            ranked[0].betweenness
        );
        // Leaves all have betweenness 0.
        for r in &ranked[1..] {
            assert!(r.betweenness.abs() < 1e-9);
        }
    }

    #[test]
    fn brandes_disconnected_returns_zero_for_singletons() {
        // Two disconnected components: {a, b} edge ab; {c, d} edge cd.
        // Nobody bridges => everyone gets 0.
        let mut nodes = BTreeMap::new();
        for id in ["a", "b", "c", "d"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        for (eid, from, to) in [("ab", "a", "b"), ("cd", "c", "d")] {
            edges.insert(
                EdgeId::from(eid),
                NetworkEdge {
                    id: EdgeId::from(eid),
                    from: NodeId::from(from),
                    to: NodeId::from(to),
                    capacity: 1.0,
                    ..Default::default()
                },
            );
        }
        let net = Network {
            id: NetworkId::from("split"),
            name: "Split".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let ranked = brandes_top_critical(&net, 4);
        for r in ranked {
            assert!(r.betweenness.abs() < 1e-9);
        }
    }

    #[test]
    fn brandes_single_node_returns_empty() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            NodeId::from("solo"),
            NetworkNode {
                id: NodeId::from("solo"),
                name: "Solo".into(),
                ..Default::default()
            },
        );
        let net = Network {
            id: NetworkId::from("solo"),
            name: "Solo".into(),
            nodes,
            ..Default::default()
        };
        // n < 2 has no source-target pairs to support betweenness.
        assert!(brandes_top_critical(&net, 5).is_empty());
    }

    #[test]
    fn brandes_two_node_pair_has_zero_betweenness() {
        // a—b path: no node sits between any pair of distinct
        // sources and targets (there are only two nodes, and
        // betweenness counts intermediate nodes).
        let mut nodes = BTreeMap::new();
        for id in ["a", "b"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        edges.insert(
            EdgeId::from("ab"),
            NetworkEdge {
                id: EdgeId::from("ab"),
                from: NodeId::from("a"),
                to: NodeId::from("b"),
                capacity: 1.0,
                ..Default::default()
            },
        );
        let net = Network {
            id: NetworkId::from("pair"),
            name: "Pair".into(),
            nodes,
            edges,
            ..Default::default()
        };
        // n=2 means (n-1)(n-2) = 0; the normalizer falls back to
        // unnormalized cb (still all zeros) because no intermediate
        // node exists. Ensure that division-by-zero guard kicks in
        // and we don't get NaN scores.
        let ranked = brandes_top_critical(&net, 2);
        for r in ranked {
            assert!(
                r.betweenness.is_finite(),
                "betweenness must stay finite even when n=2"
            );
        }
    }

    #[test]
    fn max_flow_parallel_edges_sum() {
        // Two parallel s -> t edges (capacities 3 and 7) should sum
        // to a max flow of 10.
        let mut nodes = BTreeMap::new();
        for id in ["s", "t"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        edges.insert(
            EdgeId::from("e1"),
            NetworkEdge {
                id: EdgeId::from("e1"),
                from: NodeId::from("s"),
                to: NodeId::from("t"),
                capacity: 3.0,
                ..Default::default()
            },
        );
        edges.insert(
            EdgeId::from("e2"),
            NetworkEdge {
                id: EdgeId::from("e2"),
                from: NodeId::from("s"),
                to: NodeId::from("t"),
                capacity: 7.0,
                ..Default::default()
            },
        );
        let net = Network {
            id: NetworkId::from("p"),
            name: "Parallel".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let res = max_flow(
            &net,
            &NodeId::from("s"),
            &NodeId::from("t"),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .expect("source / sink defined");
        assert!((res.flow - 10.0).abs() < 1e-9);
        // Both edges carry flow at saturation, so both appear in the
        // canonical min-cut.
        assert_eq!(res.min_cut.len(), 2);
    }

    #[test]
    fn max_flow_already_disconnected_yields_zero() {
        // A network with no s->t path returns flow 0 and an empty
        // min-cut (there's nothing to cut).
        let mut nodes = BTreeMap::new();
        for id in ["s", "x", "t"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        edges.insert(
            EdgeId::from("sx"),
            NetworkEdge {
                id: EdgeId::from("sx"),
                from: NodeId::from("s"),
                to: NodeId::from("x"),
                capacity: 5.0,
                ..Default::default()
            },
        );
        // No edge x->t. So s and t are disconnected.
        let net = Network {
            id: NetworkId::from("split"),
            name: "Split".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let res = max_flow(
            &net,
            &NodeId::from("s"),
            &NodeId::from("t"),
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .expect("source / sink defined");
        assert!((res.flow - 0.0).abs() < f64::EPSILON);
        assert!(res.min_cut.is_empty());
    }

    #[test]
    fn brandes_path_middle_dominates() {
        // Path a - b - c - d - e: middle node 'c' has the highest
        // betweenness (it's on most pairs' shortest paths).
        let mut nodes = BTreeMap::new();
        for id in ["a", "b", "c", "d", "e"] {
            nodes.insert(
                NodeId::from(id),
                NetworkNode {
                    id: NodeId::from(id),
                    name: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut edges = BTreeMap::new();
        for (eid, from, to) in [
            ("ab", "a", "b"),
            ("bc", "b", "c"),
            ("cd", "c", "d"),
            ("de", "d", "e"),
        ] {
            edges.insert(
                EdgeId::from(eid),
                NetworkEdge {
                    id: EdgeId::from(eid),
                    from: NodeId::from(from),
                    to: NodeId::from(to),
                    capacity: 1.0,
                    ..Default::default()
                },
            );
        }
        let net = Network {
            id: NetworkId::from("path"),
            name: "Path".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let ranked = brandes_top_critical(&net, 5);
        assert_eq!(ranked[0].node, NodeId::from("c"));
        // Middle of a 5-path: cb_raw[c] counts paths from {a,b} to
        // {d,e}: 4 ordered pairs * 2 (both directions) = 8, plus
        // intermediate counts on its own neighborhood. This is
        // strictly above the b/d nodes, which only sit between
        // smaller subsets.
        assert!(
            ranked[0].betweenness > ranked[1].betweenness,
            "middle node must outrank its immediate neighbors"
        );
    }

    #[test]
    fn mean_infiltration_aggregates_across_runs() {
        use faultline_types::stats::{Outcome, RunResult, StateSnapshot};
        // Hand-craft two runs with different infiltration footprints
        // and check that the mean count per faction is correct
        // without invoking the engine.
        let nid = NetworkId::from("supply");

        // Run 1: red has 2 infiltrated nodes; blue has 0.
        let mut report1 = faultline_types::stats::NetworkReport {
            network: nid.clone(),
            static_node_count: 5,
            static_edge_count: 4,
            samples: Vec::new(),
            terminal_disrupted_nodes: Default::default(),
            terminal_edge_factors: Default::default(),
            terminal_infiltrated: Default::default(),
        };
        let mut red_set1 = BTreeSet::new();
        red_set1.insert(NodeId::from("a"));
        red_set1.insert(NodeId::from("b"));
        report1
            .terminal_infiltrated
            .insert(FactionId::from("red"), red_set1);

        // Run 2: red has 4 infiltrated nodes; blue has 1.
        let mut report2 = faultline_types::stats::NetworkReport {
            network: nid.clone(),
            static_node_count: 5,
            static_edge_count: 4,
            samples: Vec::new(),
            terminal_disrupted_nodes: Default::default(),
            terminal_edge_factors: Default::default(),
            terminal_infiltrated: Default::default(),
        };
        let mut red_set2 = BTreeSet::new();
        for n in ["a", "b", "c", "d"] {
            red_set2.insert(NodeId::from(n));
        }
        let mut blue_set2 = BTreeSet::new();
        blue_set2.insert(NodeId::from("e"));
        report2
            .terminal_infiltrated
            .insert(FactionId::from("red"), red_set2);
        report2
            .terminal_infiltrated
            .insert(FactionId::from("blue"), blue_set2);

        let make_run = |idx: u32, report: faultline_types::stats::NetworkReport| RunResult {
            run_index: idx,
            seed: 0,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.0,
            },
            final_tick: 1,
            final_state: StateSnapshot {
                tick: 1,
                faction_states: BTreeMap::new(),
                region_control: BTreeMap::new(),
                infra_status: BTreeMap::new(),
                tension: 0.0,
                events_fired_this_tick: Vec::new(),
            },
            snapshots: Vec::new(),
            event_log: Vec::new(),
            campaign_reports: BTreeMap::new(),
            defender_queue_reports: Vec::new(),
            network_reports: {
                let mut m = BTreeMap::new();
                m.insert(nid.clone(), report);
                m
            },
            fracture_events: Vec::new(),
            supply_pressure_reports: std::collections::BTreeMap::new(),
            civilian_activations: Vec::new(),
            tech_costs: std::collections::BTreeMap::new(),
            narrative_events: Vec::new(),
            narrative_dominance_ticks: BTreeMap::new(),
            narrative_peak_dominance: BTreeMap::new(),
            displacement_reports: std::collections::BTreeMap::new(),
            utility_decisions: BTreeMap::new(),
        };
        let runs = vec![make_run(0, report1), make_run(1, report2)];

        let mean = mean_infiltration_per_faction(&runs, &nid);
        // Red: (2 + 4) / 2 = 3.0
        // Blue: (0 + 1) / 2 = 0.5
        assert!((mean[&FactionId::from("red")] - 3.0).abs() < 1e-9);
        assert!((mean[&FactionId::from("blue")] - 0.5).abs() < 1e-9);
    }
}
