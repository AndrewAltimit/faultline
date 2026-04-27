//! Typed network primitive (Epic L).
//!
//! A [`Network`] is a directed weighted multigraph attached to a
//! [`Scenario`](crate::scenario::Scenario). It models any flow-bearing
//! topology a scenario author needs — supply lines, communications,
//! financial settlement, social influence — with capacity and
//! per-edge metadata (latency, bandwidth, trust). Multiple networks
//! coexist on the same scenario without sharing nodes, so a single
//! faction's supply, comms, and social graphs can be modeled
//! simultaneously.
//!
//! The schema is *declarative*: it captures topology and per-edge
//! metadata. Runtime mutation (capacity reductions from interdiction,
//! node disruption, attacker visibility from infiltration) lives in
//! [`crate::stats`] / engine state and is driven by event effects
//! (`EventEffect::NetworkEdgeCapacity` / `NodeDisrupt` / `Infiltrate`).
//!
//! All collections are `BTreeMap` for deterministic iteration order —
//! the determinism contract requires bit-identical output across native
//! and WASM for the same seed.
//!
//! # Validation
//!
//! Engine-side validation rejects:
//! - edges with a `from` or `to` that is not a declared node;
//! - duplicate edge ids;
//! - non-finite or negative capacity / latency / bandwidth;
//! - trust outside `[0, 1]`;
//! - self-loops (no analytical use; almost always an authoring typo).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{EdgeId, FactionId, NetworkId, NodeId, RegionId};

/// A typed graph attached to a [`Scenario`](crate::scenario::Scenario).
///
/// Directed: an edge `from -> to` does not imply the reverse edge.
/// Authors who want bidirectional flow declare both directions.
/// `kind` is metadata only — the engine treats every network the same.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Network {
    pub id: NetworkId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Free-text classification (e.g. `"supply"`, `"comms"`,
    /// `"social"`, `"finance"`). Surfaced in the report; not
    /// interpreted by the engine.
    #[serde(default)]
    pub kind: String,
    /// Faction that owns / depends on this network. Optional —
    /// scenarios can model neutral / shared infrastructure (a public
    /// road grid contested by two sides) by leaving it `None`.
    #[serde(default)]
    pub owner: Option<FactionId>,
    pub nodes: BTreeMap<NodeId, NetworkNode>,
    pub edges: BTreeMap<EdgeId, NetworkEdge>,
}

/// One node in a [`Network`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkNode {
    pub id: NodeId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Optional region the node is geographically situated in. Lets
    /// reports correlate network damage with kinetic activity in the
    /// same region. Validation does *not* require the region to exist
    /// — networks can be abstract (financial / social) and have no
    /// geographic tie.
    #[serde(default)]
    pub region: Option<RegionId>,
    /// Multiplicative importance weight `[0, 1]`. Used when ranking
    /// critical nodes — an author flag for "this hub is more painful
    /// to lose than its degree alone implies." Defaults to `1.0`.
    #[serde(default = "one_f64")]
    pub criticality: f64,
}

/// One directed edge in a [`Network`].
///
/// Capacity is the maximum flow the edge can carry per tick.
/// `runtime_capacity_factor` (held in
/// [`crate::stats::NetworkRuntimeSnapshot`]) multiplies into this for
/// dynamic interdiction effects; the static `capacity` here is the
/// pristine baseline.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    /// Static capacity (units / tick). Must be `>= 0`. `0` is
    /// permitted — useful for declaring an edge that exists
    /// topologically but is currently saturated by other traffic.
    pub capacity: f64,
    /// Latency (ticks). `>= 0`. Surfaced in the report; not currently
    /// consumed by metrics.
    #[serde(default)]
    pub latency: f64,
    /// Bandwidth (units). `>= 0`. Distinct from `capacity` for
    /// scenarios where capacity = peak burst and bandwidth = sustained
    /// throughput.
    #[serde(default)]
    pub bandwidth: f64,
    /// Trust score `[0, 1]` — scenario-author-asserted confidence
    /// the edge is not adversarially observed. Used in the Infiltrate
    /// event effect: a high-trust edge that has been infiltrated
    /// surfaces as a high-impact intelligence loss in the report.
    #[serde(default = "one_f64")]
    pub trust: f64,
    #[serde(default)]
    pub description: String,
}

fn one_f64() -> f64 {
    1.0
}
