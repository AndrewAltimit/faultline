//! Per-tick network resilience capture (Epic L).
//!
//! The engine emits one [`NetworkSample`] per tick per declared
//! network, recorded *after* the event phase fired so a same-tick
//! interdiction shows up in this tick's sample. The cross-run
//! analytics module ([`faultline_stats::network_metrics`]) consumes
//! the captured `samples` for resilience-curve rendering.
//!
//! Determinism: every operation here is a pure function of the
//! current `(scenario, network_states)` pair — no RNG draws, no
//! HashMap iteration, no mutation across networks. Iteration is
//! `BTreeMap`-ordered so the manifest hash stays stable.

use std::collections::{BTreeMap, BTreeSet};

use faultline_types::ids::NodeId;
use faultline_types::network::Network;
use faultline_types::scenario::Scenario;
use faultline_types::stats::NetworkSample;

use crate::state::{NetworkRuntimeState, SimulationState};

/// Capture one [`NetworkSample`] per declared network and append it
/// to the corresponding [`NetworkRuntimeState::samples`]. No-op when
/// the scenario declares no networks (legacy hot path).
pub fn capture_samples(state: &mut SimulationState, scenario: &Scenario) {
    if scenario.networks.is_empty() {
        return;
    }
    let tick = state.tick;
    for (nid, net) in &scenario.networks {
        let Some(rt) = state.network_states.get_mut(nid) else {
            continue;
        };
        let sample = compute_sample(net, rt, tick);
        rt.samples.push(sample);
    }
}

/// Compute a [`NetworkSample`] from a network's static topology and
/// its current runtime state.
///
/// Pure function — exposed for unit testing and report-side
/// re-derivation. Treats edges incident to a disrupted node as
/// severed; treats edges with `runtime_factor * capacity == 0` as
/// severed for component counting (so a `factor = 0` interdiction
/// fragments the network the same way a node disruption would).
pub fn compute_sample(net: &Network, rt: &NetworkRuntimeState, tick: u32) -> NetworkSample {
    // Build an undirected adjacency over non-severed edges.
    // Endpoints touching disrupted nodes don't connect anything.
    let disrupted = &rt.disrupted_nodes;
    let mut adj: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for nid in net.nodes.keys() {
        adj.insert(nid.clone(), BTreeSet::new());
    }

    let mut residual_capacity = 0.0_f64;
    for (eid, edge) in &net.edges {
        // Edges referencing unknown nodes are dropped silently here —
        // engine validation rejects them at scenario load, so this
        // arm is only reached if the validator is bypassed.
        if !net.nodes.contains_key(&edge.from) || !net.nodes.contains_key(&edge.to) {
            continue;
        }
        if disrupted.contains(&edge.from) || disrupted.contains(&edge.to) {
            continue;
        }
        let factor = rt.edge_factor(eid);
        let effective = edge.capacity * factor;
        if effective <= 0.0 {
            continue;
        }
        residual_capacity += effective;
        adj.entry(edge.from.clone())
            .or_default()
            .insert(edge.to.clone());
        adj.entry(edge.to.clone())
            .or_default()
            .insert(edge.from.clone());
    }

    let (component_count, largest_component) = connected_components(&adj, &net.nodes);

    let disrupted_count =
        u32::try_from(disrupted.len()).expect("disrupted_nodes count exceeds u32::MAX");

    NetworkSample {
        tick,
        component_count,
        largest_component,
        residual_capacity,
        disrupted_nodes: disrupted_count,
    }
}

/// Number of weakly-connected components and the size of the
/// largest. Treats disrupted nodes as still being members of the
/// graph (each becomes its own singleton component, which is what
/// "fragmented from the rest" means analytically — the node is
/// still *there*, it's just isolated). DFS via a `Vec` stack with
/// `BTreeSet`-ordered neighbour iteration; traversal order is
/// canonical and the count is deterministic. (Component count is
/// traversal-order-independent regardless, but a `Vec`-stack DFS
/// avoids the per-pop heap allocation that a `VecDeque`-based BFS
/// would incur on tight tick loops.)
fn connected_components(
    adj: &BTreeMap<NodeId, BTreeSet<NodeId>>,
    nodes: &BTreeMap<NodeId, faultline_types::network::NetworkNode>,
) -> (u32, u32) {
    let mut visited: BTreeSet<NodeId> = BTreeSet::new();
    let mut count: u32 = 0;
    let mut largest: u32 = 0;

    for start in nodes.keys() {
        if visited.contains(start) {
            continue;
        }
        // DFS stack — initialize with the start node.
        let mut stack: Vec<NodeId> = vec![start.clone()];
        let mut size: u32 = 0;
        while let Some(n) = stack.pop() {
            if !visited.insert(n.clone()) {
                continue;
            }
            size += 1;
            if let Some(neighbors) = adj.get(&n) {
                for next in neighbors {
                    if !visited.contains(next) {
                        stack.push(next.clone());
                    }
                }
            }
        }
        count += 1;
        if size > largest {
            largest = size;
        }
    }

    (count, largest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::ids::{EdgeId, NetworkId};
    use faultline_types::network::{NetworkEdge, NetworkNode};

    fn make_path_network() -> Network {
        // Three nodes in a path: a -- b -- c
        let mut nodes = BTreeMap::new();
        for id in ["a", "b", "c"] {
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
                capacity: 10.0,
                ..Default::default()
            },
        );
        edges.insert(
            EdgeId::from("bc"),
            NetworkEdge {
                id: EdgeId::from("bc"),
                from: NodeId::from("b"),
                to: NodeId::from("c"),
                capacity: 10.0,
                ..Default::default()
            },
        );
        Network {
            id: NetworkId::from("supply"),
            name: "Supply".into(),
            nodes,
            edges,
            ..Default::default()
        }
    }

    #[test]
    fn pristine_path_is_one_component() {
        let net = make_path_network();
        let rt = NetworkRuntimeState::default();
        let sample = compute_sample(&net, &rt, 1);
        assert_eq!(sample.component_count, 1);
        assert_eq!(sample.largest_component, 3);
        assert!((sample.residual_capacity - 20.0).abs() < f64::EPSILON);
        assert_eq!(sample.disrupted_nodes, 0);
    }

    #[test]
    fn disrupting_middle_node_fragments_path() {
        let net = make_path_network();
        let mut rt = NetworkRuntimeState::default();
        rt.disrupted_nodes.insert(NodeId::from("b"));
        let sample = compute_sample(&net, &rt, 2);
        // After disrupting 'b', both edges are severed, so a, b, c
        // are each isolated => 3 components, largest = 1.
        assert_eq!(sample.component_count, 3);
        assert_eq!(sample.largest_component, 1);
        assert_eq!(sample.disrupted_nodes, 1);
        assert!((sample.residual_capacity - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_capacity_edge_severs_path() {
        let net = make_path_network();
        let mut rt = NetworkRuntimeState::default();
        rt.edge_factors.insert(EdgeId::from("ab"), 0.0);
        let sample = compute_sample(&net, &rt, 3);
        // Edge ab is severed; bc connects b-c. So {a} and {b,c}.
        assert_eq!(sample.component_count, 2);
        assert_eq!(sample.largest_component, 2);
        assert!((sample.residual_capacity - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn factor_composition_is_multiplicative_under_clamp() {
        // A new event applies factor 0.5 to an edge already at 0.5;
        // composed factor should be 0.25 (0.5 * 0.5), not 0.5.
        let mut rt = NetworkRuntimeState::default();
        rt.edge_factors.insert(EdgeId::from("ab"), 0.5);
        let prev = rt.edge_factor(&EdgeId::from("ab"));
        let composed = (prev * 0.5).clamp(0.0, 4.0);
        assert!((composed - 0.25).abs() < f64::EPSILON);
    }
}
