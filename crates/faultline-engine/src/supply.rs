//! Supply-network interdiction phase (Epic D — round three, item 2).
//!
//! Translates per-network capacity loss into per-faction *supply
//! pressure*: a multiplier in `[0, 1]` that scales the owner's
//! per-tick resource income. Builds on the Epic L network primitives —
//! the static topology, runtime edge factors, and disrupted-node set
//! all live on `state.network_states`; this module only consumes them.
//!
//! ## The contract
//!
//! A network counts as a *supply network* iff its `kind` field equals
//! `"supply"` (case-insensitive) and it carries a non-`None` `owner`.
//! Validation rejects `kind = "supply"` without an owner — that shape
//! is a silent no-op (the engine has no faction to apply pressure to)
//! and we want it to fail loud at scenario-load time.
//!
//! Per tick, for each (faction, owned-supply-network) pair the engine
//! computes `residual_capacity / baseline_capacity`, clamped to
//! `[0, 1]`. (A runtime factor `> 1.0` doesn't help — it can't grow
//! more supply than the static topology was authored to carry.) The
//! per-faction supply pressure is the **product** of those ratios
//! across every owned supply network: cutting any one supply network
//! cuts overall supply, and cutting two is multiplicative. Factions
//! with no owned supply network see `pressure = 1.0` (legacy
//! behavior).
//!
//! ## Where it applies
//!
//! [`crate::tick::attrition_phase`] reads the supply pressure at the
//! top of its per-faction loop and multiplies the faction's effective
//! `resource_rate` by it before computing income. Upkeep is *not*
//! attenuated — the units still need to eat regardless of whether
//! supply is reaching them, which is the whole reason cut supply
//! lines hurt.
//!
//! ## Determinism
//!
//! Every helper here is a pure function of `(scenario, state)` — no
//! RNG, no `HashMap`, no allocation in the hot path. Iteration is
//! `BTreeMap`-ordered. The pressure value is captured into
//! `RuntimeFactionState.current_supply_pressure` once per tick and
//! into running min / sum / pressured-tick counters for post-run
//! reporting.

use faultline_types::ids::FactionId;
use faultline_types::network::Network;
use faultline_types::scenario::Scenario;

use crate::state::{NetworkRuntimeState, SimulationState};

/// `kind`-string discriminator for supply networks.
///
/// Compared case-insensitively so authors can write `"Supply"`,
/// `"SUPPLY"`, etc. The canonical form in bundled scenarios is
/// lowercase.
const SUPPLY_KIND: &str = "supply";

/// Threshold below which a faction is considered to be operating
/// under *meaningful* supply pressure for the post-run "ticks under
/// pressure" counter. Cosmetic — chosen so a 5% capacity dip from
/// background event chatter doesn't dominate the report; sustained
/// loss greater than 10% does. Not load-bearing for any decision the
/// engine makes (income scaling reads the raw pressure value, not a
/// thresholded version).
pub const PRESSURE_REPORTING_THRESHOLD: f64 = 0.9;

/// Whether `net` is a supply network in the round-three contract sense.
///
/// Returns `true` iff `kind` matches `"supply"` (case-insensitive)
/// **and** `owner` is `Some(_)`. The owner check is what makes the
/// supply network actually do something at runtime; validation
/// enforces the same shape so an `kind = "supply"` declaration without
/// an owner fails at scenario load instead of silently no-oping at
/// every tick.
pub fn is_active_supply_network(net: &Network) -> bool {
    net.kind.eq_ignore_ascii_case(SUPPLY_KIND) && net.owner.is_some()
}

/// Compute `(residual_capacity, baseline_capacity)` for one network.
///
/// `baseline_capacity` is the sum of the static `edge.capacity` over
/// every edge whose endpoints both exist in the topology — the
/// pristine throughput of the graph as authored. `residual_capacity`
/// is the same sum restricted to edges where neither endpoint is
/// disrupted and whose effective `capacity * runtime_factor` is
/// strictly positive.
///
/// The residual computation here matches
/// [`crate::network::compute_sample`]'s definition exactly so that
/// post-run resilience curves and live supply pressure agree at every
/// tick.
pub fn compute_residual_and_baseline(net: &Network, rt: &NetworkRuntimeState) -> (f64, f64) {
    let mut baseline = 0.0_f64;
    let mut residual = 0.0_f64;
    for (eid, edge) in &net.edges {
        // Bypass-the-validator defense: skip edges with unknown
        // endpoints. Validation rejects these at scenario load, but
        // a hand-built `Scenario` could still introduce one and we
        // don't want to crash the supply phase over it.
        if !net.nodes.contains_key(&edge.from) || !net.nodes.contains_key(&edge.to) {
            continue;
        }
        baseline += edge.capacity;
        if rt.disrupted_nodes.contains(&edge.from) || rt.disrupted_nodes.contains(&edge.to) {
            continue;
        }
        let factor = rt.edge_factor(eid);
        let effective = edge.capacity * factor;
        if effective <= 0.0 {
            continue;
        }
        residual += effective;
    }
    (residual, baseline)
}

/// Per-faction supply pressure in `[0, 1]` for one tick.
///
/// Returns `1.0` (no effect) for any faction without an owned supply
/// network. Otherwise: the multiplicative product of
/// `(residual / baseline).clamp(0, 1)` across every owned supply
/// network with a non-zero baseline. Networks where `baseline == 0`
/// (degenerate authoring — every edge has zero capacity) are skipped
/// rather than treated as "fully broken" since the topology never
/// had any supply to begin with.
///
/// Determinism: pure function, `BTreeMap`-ordered iteration over
/// `scenario.networks` — the multiplication order is canonical.
pub fn supply_pressure_for_faction(
    scenario: &Scenario,
    state: &SimulationState,
    faction: &FactionId,
) -> f64 {
    if scenario.networks.is_empty() {
        return 1.0;
    }
    let mut pressure = 1.0_f64;
    for (nid, net) in &scenario.networks {
        if !is_active_supply_network(net) {
            continue;
        }
        // owner is Some by construction — checked above.
        let owner = match net.owner.as_ref() {
            Some(o) => o,
            None => continue,
        };
        if owner != faction {
            continue;
        }
        let Some(rt) = state.network_states.get(nid) else {
            continue;
        };
        let (residual, baseline) = compute_residual_and_baseline(net, rt);
        if baseline <= 0.0 {
            continue;
        }
        let ratio = (residual / baseline).clamp(0.0, 1.0);
        pressure *= ratio;
    }
    pressure
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use faultline_types::ids::{EdgeId, FactionId, NetworkId, NodeId};
    use faultline_types::network::{Network, NetworkEdge, NetworkNode};

    fn make_path_network(owner: Option<&str>, kind: &str) -> Network {
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
                capacity: 10.0,
                ..Default::default()
            },
        );
        Network {
            id: NetworkId::from("supply"),
            name: "Supply".into(),
            kind: kind.into(),
            owner: owner.map(FactionId::from),
            nodes,
            edges,
            ..Default::default()
        }
    }

    #[test]
    fn supply_kind_matches_case_insensitively() {
        assert!(is_active_supply_network(&make_path_network(
            Some("blue"),
            "supply"
        )));
        assert!(is_active_supply_network(&make_path_network(
            Some("blue"),
            "Supply"
        )));
        assert!(is_active_supply_network(&make_path_network(
            Some("blue"),
            "SUPPLY"
        )));
        assert!(!is_active_supply_network(&make_path_network(
            Some("blue"),
            "comms"
        )));
    }

    #[test]
    fn supply_kind_requires_owner_to_be_active() {
        // No owner = not an active supply network even if kind matches.
        // (Validation rejects this shape at scenario load.)
        assert!(!is_active_supply_network(&make_path_network(
            None, "supply"
        )));
    }

    #[test]
    fn pristine_network_yields_full_residual() {
        let net = make_path_network(Some("blue"), "supply");
        let rt = NetworkRuntimeState::default();
        let (residual, baseline) = compute_residual_and_baseline(&net, &rt);
        assert!((baseline - 10.0).abs() < f64::EPSILON);
        assert!((residual - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn disrupted_endpoint_drops_residual() {
        let net = make_path_network(Some("blue"), "supply");
        let mut rt = NetworkRuntimeState::default();
        rt.disrupted_nodes.insert(NodeId::from("a"));
        let (residual, baseline) = compute_residual_and_baseline(&net, &rt);
        assert!((baseline - 10.0).abs() < f64::EPSILON);
        assert!(residual <= f64::EPSILON);
    }

    #[test]
    fn zero_factor_drops_residual() {
        let net = make_path_network(Some("blue"), "supply");
        let mut rt = NetworkRuntimeState::default();
        rt.edge_factors.insert(EdgeId::from("ab"), 0.0);
        let (_residual, baseline) = compute_residual_and_baseline(&net, &rt);
        assert!((baseline - 10.0).abs() < f64::EPSILON);
        let (residual, _) = compute_residual_and_baseline(&net, &rt);
        assert!(residual <= f64::EPSILON);
    }

    #[test]
    fn half_factor_halves_residual() {
        let net = make_path_network(Some("blue"), "supply");
        let mut rt = NetworkRuntimeState::default();
        rt.edge_factors.insert(EdgeId::from("ab"), 0.5);
        let (residual, baseline) = compute_residual_and_baseline(&net, &rt);
        assert!((baseline - 10.0).abs() < f64::EPSILON);
        assert!((residual - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn factor_above_one_does_not_inflate_pressure() {
        // Residual can exceed baseline if factor > 1, but the
        // pressure multiplier is clamped to [0, 1] so income is never
        // *boosted* by a runaway author chain.
        let net = make_path_network(Some("blue"), "supply");
        let mut rt = NetworkRuntimeState::default();
        rt.edge_factors.insert(EdgeId::from("ab"), 2.0);
        let (residual, baseline) = compute_residual_and_baseline(&net, &rt);
        assert!(residual > baseline);
        let ratio = (residual / baseline).clamp(0.0, 1.0);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }
}
