//! Property tests for `faultline_stats::network_metrics` (R3-5).
//!
//! Two invariants worth pinning per the May 2026 refresh:
//!
//! 1. **Post-disruption residual capacity ≤ pre-disruption.** Disrupting
//!    nodes (the `disrupted` set) or zeroing edge capacities (`edge_factors`)
//!    can never *increase* max-flow on a static topology. The
//!    Edmonds-Karp implementation passes augmenting paths through the
//!    residual graph; a regression that flips a sign or adds back a
//!    fully-saturated edge would silently inflate the metric.
//! 2. **Brandes betweenness scores stay in `[0, 1]`.** The `(n-1)*(n-2)`
//!    normalization is supposed to keep the score bounded for any
//!    undirected graph; running the algorithm against random graphs
//!    guards against an off-by-one on the normalization or a sign flip
//!    in the back-propagation step.
//!
//! Both invariants are checked against random small networks (≤ 20
//! nodes, ≤ 40 edges) so each proptest case runs in milliseconds even
//! on the standard 256-case proptest budget.

use std::collections::{BTreeMap, BTreeSet};

use faultline_stats::network_metrics::{brandes_top_critical, max_flow};
use faultline_types::ids::{EdgeId, NetworkId, NodeId};
use faultline_types::network::{Network, NetworkEdge, NetworkNode};
use proptest::prelude::*;

/// Build a random connected-ish network with `n_nodes` nodes and up to
/// `n_edges` edges. Every edge has capacity ∈ `[1, 10]`. Nodes are
/// labeled `n0..n{n_nodes-1}`.
///
/// Edges are sampled from `(from_idx, to_idx, capacity)` triples; we
/// drop self-loops (validation rejects them) and dedupe parallel edges
/// by giving each a unique `EdgeId`. Parallel edges are *not* deduped
/// for the purpose of capacity — `max_flow` sums parallel capacities,
/// which is the correct semantics.
///
/// To keep the disruption invariant non-trivially exercised, the
/// generator first lays down a directed spanning chain `n0 → n1 → … →
/// n{n-1}` so an `n0 → n1` flow is *always* feasible at baseline. The
/// random extra edges then layer additional structure on top. Without
/// the chain, sparse graphs (≤ 3 random edges over 20 nodes) very
/// rarely contain any `n0 → n1` path and the disruption invariant
/// passes vacuously at `0.0 ≤ 0.0`.
fn build_network(n_nodes: usize, edges: &[(usize, usize, f64)]) -> Network {
    let mut nodes = BTreeMap::new();
    for i in 0..n_nodes {
        let id = NodeId::from(format!("n{i}").as_str());
        nodes.insert(
            id.clone(),
            NetworkNode {
                id,
                name: format!("Node {i}"),
                ..Default::default()
            },
        );
    }
    let mut net_edges = BTreeMap::new();
    // Spanning chain n0 → n1 → ... → n{n-1} (capacity 1.0 each) so the
    // baseline n0→n1 flow is at least 1.0.
    for i in 0..n_nodes.saturating_sub(1) {
        let eid = EdgeId::from(format!("chain{i}").as_str());
        net_edges.insert(
            eid.clone(),
            NetworkEdge {
                id: eid,
                from: NodeId::from(format!("n{i}").as_str()),
                to: NodeId::from(format!("n{}", i + 1).as_str()),
                capacity: 1.0,
                ..Default::default()
            },
        );
    }
    for (i, &(u, v, cap)) in edges.iter().enumerate() {
        if u == v {
            continue;
        }
        let eid = EdgeId::from(format!("e{i}").as_str());
        net_edges.insert(
            eid.clone(),
            NetworkEdge {
                id: eid,
                from: NodeId::from(format!("n{u}").as_str()),
                to: NodeId::from(format!("n{v}").as_str()),
                capacity: cap,
                ..Default::default()
            },
        );
    }
    Network {
        id: NetworkId::from("prop"),
        name: "Prop".into(),
        nodes,
        edges: net_edges,
        ..Default::default()
    }
}

/// Strategy: a small random graph layered on top of an n0→…→n{n-1}
/// spanning chain (added in `build_network`).
fn arb_network() -> impl Strategy<Value = Network> {
    (3usize..=20)
        .prop_flat_map(|n| {
            let edge_strat = (0usize..n, 0usize..n, 1.0_f64..=10.0);
            (Just(n), proptest::collection::vec(edge_strat, 1..=40))
        })
        .prop_map(|(n, edges)| build_network(n, &edges))
}

proptest! {
    /// **Invariant: disrupting nodes or zeroing edges cannot increase
    /// max-flow.** The pinned example invariant from `improvement-plan.md`:
    /// "post-disruption network samples never have a larger residual
    /// capacity than pre-disruption ones."
    #[test]
    fn disruption_never_increases_max_flow(
        net in arb_network(),
        disrupt_count in 0usize..=5,
        zeroed_edge_count in 0usize..=5,
    ) {
        // Source / sink are the lexicographically-first two nodes; that
        // gives a stable pair without rejecting samples and avoids
        // re-importing rng into the proptest body.
        let mut keys = net.nodes.keys();
        let Some(source) = keys.next().cloned() else { return Ok(()); };
        let Some(sink) = keys.next().cloned() else { return Ok(()); };

        let baseline = max_flow(
            &net,
            &source,
            &sink,
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .expect("source / sink defined");

        // Skip vacuous cases. `arb_network` lays down a spanning chain
        // so `baseline.flow` should be ≥ 1.0 in practice, but if a
        // future generator change breaks that assumption we want the
        // failure to surface as a *missing* signal (proptest reports
        // "filtered all generated values") rather than as the
        // invariant `0.0 <= 0.0` quietly always passing.
        prop_assume!(baseline.flow > 1e-9);

        // Disrupt the next-N nodes after source/sink. Skipping the
        // endpoints themselves keeps the comparison meaningful: if we
        // disrupted the source, max_flow would still return Some but
        // with flow = 0, which trivially satisfies the invariant. The
        // useful case is interior-node disruption.
        let interior: Vec<NodeId> = net
            .nodes
            .keys()
            .filter(|k| **k != source && **k != sink)
            .take(disrupt_count)
            .cloned()
            .collect();
        let disrupted: BTreeSet<NodeId> = interior.into_iter().collect();

        // Zero the first-N edges by EdgeId.
        let mut factors = BTreeMap::new();
        for eid in net.edges.keys().take(zeroed_edge_count) {
            factors.insert(eid.clone(), 0.0);
        }

        let after = max_flow(&net, &source, &sink, &factors, &disrupted)
            .expect("source / sink defined");
        prop_assert!(
            after.flow <= baseline.flow + 1e-9,
            "disruption increased max-flow: baseline={} after={}",
            baseline.flow,
            after.flow
        );
        prop_assert!(after.flow >= 0.0, "max_flow must be non-negative");
        prop_assert!(after.flow.is_finite(), "max_flow must be finite");
    }

    /// **Invariant: max-flow is non-negative and finite for any input.**
    /// A signing or NaN regression would break every downstream
    /// network-resilience report.
    #[test]
    fn max_flow_is_non_negative_and_finite(net in arb_network()) {
        let mut keys = net.nodes.keys();
        let Some(source) = keys.next().cloned() else { return Ok(()); };
        let Some(sink) = keys.next().cloned() else { return Ok(()); };
        let res = max_flow(
            &net,
            &source,
            &sink,
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .expect("source / sink defined");
        prop_assert!(res.flow >= 0.0);
        prop_assert!(res.flow.is_finite());
    }

    /// **Invariant: Brandes betweenness scores are in `[0, 1]`.** The
    /// `(n-1)*(n-2)` normalization on the doubled raw count plus the
    /// path-counting algebra collectively pin every score into the
    /// unit interval. A regression on the normalization (e.g. wrong
    /// factor of 2) would push scores above 1 on bridging nodes; a
    /// sign error in the back-propagation would push them negative.
    #[test]
    fn brandes_scores_in_unit_interval(net in arb_network()) {
        let ranked = brandes_top_critical(&net, 20);
        for r in &ranked {
            prop_assert!(
                r.betweenness.is_finite(),
                "betweenness must be finite, got {}",
                r.betweenness
            );
            prop_assert!(
                r.betweenness >= 0.0,
                "betweenness must be non-negative, got {}",
                r.betweenness
            );
            // Strict floor on 1.0 would fail on graphs with a perfect
            // bridge node (the star centre normalizes to exactly 1.0);
            // 1e-9 absorbs floating-point drift around that boundary.
            prop_assert!(
                r.betweenness <= 1.0 + 1e-9,
                "betweenness must be ≤ 1.0, got {}",
                r.betweenness
            );
        }
        // Ranking must be in descending score order — a regression on
        // the sort key would silently mis-rank critical nodes.
        for w in ranked.windows(2) {
            prop_assert!(
                w[0].betweenness >= w[1].betweenness - 1e-12,
                "ranking not descending: {} then {}",
                w[0].betweenness,
                w[1].betweenness
            );
        }
    }
}
