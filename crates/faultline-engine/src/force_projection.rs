//! Force-projection phase — units that affect regions beyond the one
//! they physically occupy.
//!
//! Baseline combat (`tick::combat_phase`) only resolves where opposing
//! forces *co-locate*. A unit with a declared
//! [`ForceProjection`](faultline_types::faction::ForceProjection) carries
//! a separate "reach" primitive: it can influence a region within range
//! of its own without moving into it.
//!
//! Round one wires the **`StandoffStrike { range, damage }`** variant:
//! a unit applies `damage` strength-attrition to a hostile force in a
//! region within `range` graph-hops of the unit's region, along
//! [`Region.borders`](faultline_types::map::Region::borders) adjacency.
//! `Airlift` and `Naval` are declared, validated, and reserved (see the
//! note on each below); they apply no per-tick effect yet because the
//! mechanics they imply (embark / disembark for airlift, sea-lane
//! reach for naval transport) need destination state the model does not
//! yet carry. They are *not* silent no-ops at the scenario level —
//! validation rejects malformed shapes at load — they are explicitly
//! reserved variants with no engine consumer in this round.
//!
//! ## Determinism
//!
//! The phase iterates `BTreeMap`/sorted collections only and consumes
//! **zero** RNG: standoff-strike attrition is a pure function of
//! `(state, scenario, map)`. The whole phase is gated on at least one
//! force declaring `force_projection.is_some()` — a scenario with no
//! projection-bearing unit returns in O(number of forces) without
//! touching any counter, so its output is bit-identical to the
//! pre-feature engine.
//!
//! ## `range` → graph distance
//!
//! `range` is an aggregate physical reach in kilometres (OSINT-style:
//! "300 km standoff reach"). It is converted to an integer hop budget
//! against the region adjacency graph using a fixed convention of
//! [`REGION_HOP_KM`] kilometres per border crossing:
//! `hops = floor(range / REGION_HOP_KM)`, floored at `1` so any
//! positive reach can always strike at least an adjacent region. A
//! breadth-first search from the firing unit's region (excluding the
//! unit's own region) enumerates every region within that hop budget.
//!
//! ## `damage` → attrition
//!
//! `damage` is the strength removed from the target per strike, applied
//! through the same proportional distribution combat uses
//! (`tick::apply_attrition_to_region`): the loss is spread across the
//! target faction's forces in the struck region in proportion to each
//! force's strength, and destroyed forces are pruned. This mirrors the
//! Lanchester-linear "fixed shot against the defending force" shape —
//! a standoff strike is a one-sided application of damage, so unlike
//! co-located combat there is no return attrition on the firing unit.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use faultline_geo::GameMap;
use faultline_types::faction::ForceProjection;
use faultline_types::ids::{FactionId, RegionId};
use faultline_types::scenario::Scenario;

use crate::state::SimulationState;

/// Kilometres of physical reach mapped to one region border crossing.
///
/// A deliberately coarse aggregate: real theatre regions in bundled
/// scenarios span on the order of low-hundreds of kilometres, so one
/// adjacency hop is treated as ~150 km. Authors expressing a reach in
/// kilometres get an intuitive hop budget (300 km → 2 hops) without the
/// schema exposing a hop count directly.
pub const REGION_HOP_KM: f64 = 150.0;

/// Convert a kilometre reach into an integer adjacency-hop budget.
///
/// Floored at `1` so any strictly-positive reach can always engage at
/// least an adjacent region; a unit declaring `StandoffStrike` has
/// already passed validation guaranteeing `range > 0` and finite.
fn range_to_hops(range: f64) -> u32 {
    let hops = (range / REGION_HOP_KM).floor();
    if !hops.is_finite() || hops < 1.0 {
        1
    } else {
        // Saturating cast: an absurd authored range can't overflow the
        // BFS depth counter. Realistic ranges are well under u32::MAX.
        hops.min(f64::from(u32::MAX)) as u32
    }
}

/// Regions within `hops` border crossings of `origin`, excluding
/// `origin` itself. Deterministic breadth-first search over the region
/// adjacency graph; the returned set is iterated in `BTreeSet` order by
/// callers, so traversal order does not affect any downstream effect.
fn regions_within_hops(origin: &RegionId, hops: u32, map: &GameMap) -> BTreeSet<RegionId> {
    let mut reached: BTreeSet<RegionId> = BTreeSet::new();
    let mut visited: BTreeSet<RegionId> = BTreeSet::new();
    visited.insert(origin.clone());
    let mut frontier: VecDeque<(RegionId, u32)> = VecDeque::new();
    frontier.push_back((origin.clone(), 0));
    while let Some((region, depth)) = frontier.pop_front() {
        if depth >= hops {
            continue;
        }
        // `adjacency` is a `BTreeMap<_, Vec<_>>`; the neighbour list is
        // authored order. We sort into a `BTreeSet` for a stable reached
        // set regardless of border declaration order.
        if let Some(neighbours) = map.adjacency.get(&region) {
            for nid in neighbours {
                if visited.insert(nid.clone()) {
                    reached.insert(nid.clone());
                    frontier.push_back((nid.clone(), depth + 1));
                }
            }
        }
    }
    reached
}

/// One standoff strike that landed during the run. Engine-internal;
/// surfaced post-run via `RunResult.force_projection_reports` (rolled
/// up per attacker faction).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrikeEvent {
    pub tick: u32,
    pub attacker: FactionId,
    pub target: FactionId,
    pub region: RegionId,
    /// Strength actually removed from the target in the struck region
    /// (may be less than the declared `damage` if the target had less
    /// strength present).
    pub strength_removed: f64,
}

/// Resolve standoff-strike force projection for every unit that
/// declares it. Pure function of `(state, scenario, map)` — no RNG.
///
/// Gated: returns immediately if no force anywhere declares
/// `force_projection`, so legacy scenarios consume zero counters and
/// stay bit-identical.
///
/// Ordering within the phase is fully `BTreeMap`/`BTreeSet`-deterministic:
/// attackers iterate in faction order, each attacker's forces in
/// `ForceId` order, candidate target regions in `RegionId` order, and
/// target factions in faction order. Each strike is applied immediately
/// so a later strike in the same tick reads the post-strike strength —
/// matching the single-pass, deterministic convention of the other
/// per-tick phases.
pub fn force_projection_phase(state: &mut SimulationState, scenario: &Scenario, map: &GameMap) {
    if !any_projection_declared(scenario) {
        return;
    }
    let tick = state.tick;

    // Snapshot the (attacker, force-region, projection) tuples up front
    // so we can mutate target strengths while iterating without holding
    // a borrow on `state.faction_states`. Built in deterministic order.
    struct Projector {
        attacker: FactionId,
        origin: RegionId,
        range: f64,
        damage: f64,
    }
    let mut projectors: Vec<Projector> = Vec::new();
    for (attacker, fs) in &state.faction_states {
        if fs.eliminated {
            continue;
        }
        for force in fs.forces.values() {
            if let Some(ForceProjection::StandoffStrike { range, damage }) = &force.force_projection
            {
                projectors.push(Projector {
                    attacker: attacker.clone(),
                    origin: force.region.clone(),
                    range: *range,
                    damage: *damage,
                });
            }
            // Airlift / Naval: reserved, no per-tick effect this round.
            // (Validation has already rejected malformed shapes.)
        }
    }

    for proj in projectors {
        let hops = range_to_hops(proj.range);
        let in_range = regions_within_hops(&proj.origin, hops, map);

        for region in &in_range {
            // Collect hostile targets in this region in deterministic
            // faction order. A faction is a valid target iff it is not
            // the attacker, is not eliminated, has positive strength in
            // the region, and combat is not diplomatically blocked
            // (mutual alliance). We reuse `combat_blocked` so a standoff
            // strike honours the same alliance coupling co-located
            // combat does — a unit will not strike a sworn ally.
            let targets: Vec<(FactionId, f64)> = state
                .faction_states
                .iter()
                .filter(|(fid, tfs)| {
                    **fid != proj.attacker
                        && !tfs.eliminated
                        && !crate::diplomacy::combat_blocked(state, scenario, &proj.attacker, fid)
                })
                .filter_map(|(fid, tfs)| {
                    let present: f64 = tfs
                        .forces
                        .values()
                        .filter(|f| f.region == *region)
                        .map(|f| f.strength)
                        .sum();
                    if present > 0.0 {
                        Some((fid.clone(), present))
                    } else {
                        None
                    }
                })
                .collect();

            for (target, present) in targets {
                let removed = proj.damage.min(present).max(0.0);
                if removed <= 0.0 {
                    continue;
                }
                crate::tick::apply_attrition_to_region(state, region, &target, removed);
                state.force_projection_strikes.push(StrikeEvent {
                    tick,
                    attacker: proj.attacker.clone(),
                    target: target.clone(),
                    region: region.clone(),
                    strength_removed: removed,
                });
            }
        }
    }
}

/// True iff any force in the scenario declares a `force_projection`.
/// O(forces); the gate that keeps legacy scenarios bit-identical.
fn any_projection_declared(scenario: &Scenario) -> bool {
    scenario
        .factions
        .values()
        .any(|f| f.forces.values().any(|u| u.force_projection.is_some()))
}

/// Per-attacker standoff-strike roll-up for one run, derived from the
/// engine's [`StrikeEvent`] log. Returns an empty map when no strike
/// landed (so the `RunResult` field elides and legacy output is
/// unchanged). Keyed by attacker faction for deterministic rendering.
pub fn collect_strike_reports(
    state: &SimulationState,
) -> BTreeMap<FactionId, faultline_types::stats::ForceProjectionReport> {
    let mut out: BTreeMap<FactionId, faultline_types::stats::ForceProjectionReport> =
        BTreeMap::new();
    for ev in &state.force_projection_strikes {
        let entry = out.entry(ev.attacker.clone()).or_insert_with(|| {
            faultline_types::stats::ForceProjectionReport {
                attacker: ev.attacker.clone(),
                strikes: 0,
                total_strength_removed: 0.0,
                regions_struck: Vec::new(),
            }
        });
        entry.strikes += 1;
        entry.total_strength_removed += ev.strength_removed;
        if !entry.regions_struck.contains(&ev.region) {
            entry.regions_struck.push(ev.region.clone());
        }
    }
    // Normalize region lists to sorted order for stable rendering.
    for report in out.values_mut() {
        report.regions_struck.sort();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::ids::RegionId;

    #[test]
    fn range_maps_to_hops_floored_at_one() {
        assert_eq!(range_to_hops(300.0), 2);
        assert_eq!(range_to_hops(150.0), 1);
        // Sub-one-hop reach still strikes adjacent regions.
        assert_eq!(range_to_hops(50.0), 1);
        assert_eq!(range_to_hops(450.0), 3);
    }

    #[test]
    fn bfs_excludes_origin_and_respects_budget() {
        use faultline_geo::{GameMap, RegionInfo};
        // Chain a - b - c - d.
        let mut adjacency = BTreeMap::new();
        let a = RegionId::from("a");
        let b = RegionId::from("b");
        let c = RegionId::from("c");
        let d = RegionId::from("d");
        adjacency.insert(a.clone(), vec![b.clone()]);
        adjacency.insert(b.clone(), vec![a.clone(), c.clone()]);
        adjacency.insert(c.clone(), vec![b.clone(), d.clone()]);
        adjacency.insert(d.clone(), vec![c.clone()]);
        let mut regions = BTreeMap::new();
        for rid in [&a, &b, &c, &d] {
            regions.insert(
                rid.clone(),
                RegionInfo {
                    id: rid.clone(),
                    name: rid.0.clone(),
                    population: 0,
                    urbanization: 0.0,
                    strategic_value: 0.0,
                },
            );
        }
        let map = GameMap {
            regions,
            adjacency,
            movement_costs: BTreeMap::new(),
        };
        let one_hop = regions_within_hops(&a, 1, &map);
        assert_eq!(one_hop, BTreeSet::from([b.clone()]));
        let two_hop = regions_within_hops(&a, 2, &map);
        assert_eq!(two_hop, BTreeSet::from([b.clone(), c.clone()]));
        // origin is never in the reached set.
        assert!(!two_hop.contains(&a));
    }
}
