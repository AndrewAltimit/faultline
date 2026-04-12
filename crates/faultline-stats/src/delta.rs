//! Delta encoding for state snapshots.
//!
//! Reduces memory and serialization size when storing many snapshots by
//! only recording fields that changed between consecutive snapshots.

use std::collections::BTreeMap;

use faultline_types::ids::{FactionId, InfraId, RegionId};
use faultline_types::stats::{DeltaEncodedRun, DeltaSnapshot, RunResult, StateSnapshot};
use faultline_types::strategy::FactionState;

const EPSILON: f64 = 1e-9;

/// Encode a `RunResult` into a `DeltaEncodedRun`.
pub fn encode_run(run: &RunResult) -> DeltaEncodedRun {
    let mut deltas = Vec::with_capacity(run.snapshots.len());
    let mut prev: Option<&StateSnapshot> = None;

    for snap in &run.snapshots {
        let delta = match prev {
            None => full_to_delta(snap),
            Some(p) => diff_snapshots(p, snap),
        };
        deltas.push(delta);
        prev = Some(snap);
    }

    DeltaEncodedRun {
        run_index: run.run_index,
        seed: run.seed,
        outcome: run.outcome.clone(),
        final_tick: run.final_tick,
        final_state: run.final_state.clone(),
        snapshots: deltas,
        event_log: run.event_log.clone(),
        campaign_reports: run.campaign_reports.clone(),
    }
}

/// Decode a `DeltaEncodedRun` back into a `RunResult` with full snapshots.
pub fn decode_run(encoded: &DeltaEncodedRun) -> RunResult {
    let mut snapshots = Vec::with_capacity(encoded.snapshots.len());
    let mut current: Option<StateSnapshot> = None;

    for delta in &encoded.snapshots {
        let full = match current {
            None => delta_to_full(delta),
            Some(ref prev) => apply_delta(prev, delta),
        };
        current = Some(full.clone());
        snapshots.push(full);
    }

    RunResult {
        run_index: encoded.run_index,
        seed: encoded.seed,
        outcome: encoded.outcome.clone(),
        final_tick: encoded.final_tick,
        final_state: encoded.final_state.clone(),
        snapshots,
        event_log: encoded.event_log.clone(),
        campaign_reports: encoded.campaign_reports.clone(),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert a full snapshot to a delta (the "first" delta — everything present).
fn full_to_delta(snap: &StateSnapshot) -> DeltaSnapshot {
    DeltaSnapshot {
        tick: snap.tick,
        faction_states: snap.faction_states.clone(),
        region_control: snap.region_control.clone(),
        infra_status: snap.infra_status.clone(),
        tension: snap.tension,
        events_fired_this_tick: snap.events_fired_this_tick.clone(),
    }
}

/// Convert a delta (first delta, all fields) back to full snapshot.
fn delta_to_full(delta: &DeltaSnapshot) -> StateSnapshot {
    StateSnapshot {
        tick: delta.tick,
        faction_states: delta.faction_states.clone(),
        region_control: delta.region_control.clone(),
        infra_status: delta.infra_status.clone(),
        tension: delta.tension,
        events_fired_this_tick: delta.events_fired_this_tick.clone(),
    }
}

/// Compute a delta between two consecutive snapshots.
fn diff_snapshots(prev: &StateSnapshot, curr: &StateSnapshot) -> DeltaSnapshot {
    let faction_states = diff_faction_states(&prev.faction_states, &curr.faction_states);
    let region_control = diff_region_control(&prev.region_control, &curr.region_control);
    let infra_status = diff_infra(&prev.infra_status, &curr.infra_status);

    DeltaSnapshot {
        tick: curr.tick,
        faction_states,
        region_control,
        infra_status,
        tension: curr.tension,
        events_fired_this_tick: curr.events_fired_this_tick.clone(),
    }
}

/// Apply a delta to a previous full snapshot to reconstruct the current snapshot.
fn apply_delta(prev: &StateSnapshot, delta: &DeltaSnapshot) -> StateSnapshot {
    let mut faction_states = prev.faction_states.clone();
    for (fid, state) in &delta.faction_states {
        faction_states.insert(fid.clone(), state.clone());
    }

    let mut region_control = prev.region_control.clone();
    for (rid, ctrl) in &delta.region_control {
        region_control.insert(rid.clone(), ctrl.clone());
    }

    let mut infra_status = prev.infra_status.clone();
    for (iid, status) in &delta.infra_status {
        infra_status.insert(iid.clone(), *status);
    }

    StateSnapshot {
        tick: delta.tick,
        faction_states,
        region_control,
        infra_status,
        tension: delta.tension,
        events_fired_this_tick: delta.events_fired_this_tick.clone(),
    }
}

fn diff_faction_states(
    prev: &BTreeMap<FactionId, FactionState>,
    curr: &BTreeMap<FactionId, FactionState>,
) -> BTreeMap<FactionId, FactionState> {
    let mut changed = BTreeMap::new();
    for (fid, curr_state) in curr {
        match prev.get(fid) {
            Some(prev_state) if !faction_state_changed(prev_state, curr_state) => {},
            _ => {
                changed.insert(fid.clone(), curr_state.clone());
            },
        }
    }
    changed
}

fn faction_state_changed(a: &FactionState, b: &FactionState) -> bool {
    (a.morale - b.morale).abs() > EPSILON
        || (a.resources - b.resources).abs() > EPSILON
        || (a.total_strength - b.total_strength).abs() > EPSILON
        || (a.logistics_capacity - b.logistics_capacity).abs() > EPSILON
        || a.controlled_regions != b.controlled_regions
        || a.tech_deployed != b.tech_deployed
        || a.institution_loyalty != b.institution_loyalty
}

fn diff_region_control(
    prev: &BTreeMap<RegionId, Option<FactionId>>,
    curr: &BTreeMap<RegionId, Option<FactionId>>,
) -> BTreeMap<RegionId, Option<FactionId>> {
    let mut changed = BTreeMap::new();
    for (rid, ctrl) in curr {
        if prev.get(rid) != Some(ctrl) {
            changed.insert(rid.clone(), ctrl.clone());
        }
    }
    changed
}

fn diff_infra(
    prev: &BTreeMap<InfraId, f64>,
    curr: &BTreeMap<InfraId, f64>,
) -> BTreeMap<InfraId, f64> {
    let mut changed = BTreeMap::new();
    for (iid, &val) in curr {
        match prev.get(iid) {
            Some(&prev_val) if (prev_val - val).abs() <= EPSILON => {},
            _ => {
                changed.insert(iid.clone(), val);
            },
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::ids::EventId;
    use faultline_types::stats::Outcome;

    fn make_snapshot(tick: u32, morale: f64, tension: f64) -> StateSnapshot {
        let fid = FactionId::from("gov");
        let rid = RegionId::from("region-a");

        let mut faction_states = BTreeMap::new();
        faction_states.insert(
            fid.clone(),
            FactionState {
                faction_id: fid.clone(),
                morale,
                resources: 100.0,
                logistics_capacity: 50.0,
                tech_deployed: vec![],
                controlled_regions: vec![rid.clone()],
                total_strength: 80.0,
                institution_loyalty: BTreeMap::new(),
            },
        );

        let mut region_control = BTreeMap::new();
        region_control.insert(rid, Some(fid));

        StateSnapshot {
            tick,
            faction_states,
            region_control,
            infra_status: BTreeMap::new(),
            tension,
            events_fired_this_tick: vec![],
        }
    }

    #[test]
    fn roundtrip_single_snapshot() {
        let snap = make_snapshot(10, 0.8, 0.5);
        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: 10,
            final_state: snap.clone(),
            snapshots: vec![snap],
            event_log: vec![],
            campaign_reports: Default::default(),
        };

        let encoded = encode_run(&run);
        let decoded = decode_run(&encoded);

        assert_eq!(decoded.snapshots.len(), 1);
        assert_eq!(decoded.snapshots[0].tick, 10);
        assert!((decoded.snapshots[0].tension - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn delta_omits_unchanged_factions() {
        let snap1 = make_snapshot(10, 0.8, 0.5);
        let snap2 = make_snapshot(20, 0.8, 0.6); // morale unchanged, tension changed

        let delta = diff_snapshots(&snap1, &snap2);

        // Faction state did not change (morale, strength, resources all same).
        assert!(
            delta.faction_states.is_empty(),
            "unchanged faction should not appear in delta"
        );
        assert!((delta.tension - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn delta_includes_changed_factions() {
        let snap1 = make_snapshot(10, 0.8, 0.5);
        let snap2 = make_snapshot(20, 0.6, 0.5); // morale changed

        let delta = diff_snapshots(&snap1, &snap2);
        assert_eq!(
            delta.faction_states.len(),
            1,
            "changed faction should be in delta"
        );
    }

    #[test]
    fn roundtrip_multiple_snapshots() {
        let snap1 = make_snapshot(10, 0.8, 0.5);
        let snap2 = make_snapshot(20, 0.7, 0.6);
        let snap3 = make_snapshot(30, 0.7, 0.7); // morale unchanged from snap2

        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.7,
            },
            final_tick: 30,
            final_state: snap3.clone(),
            snapshots: vec![snap1.clone(), snap2.clone(), snap3.clone()],
            event_log: vec![],
            campaign_reports: Default::default(),
        };

        let encoded = encode_run(&run);
        let decoded = decode_run(&encoded);

        assert_eq!(decoded.snapshots.len(), 3);
        for (orig, dec) in run.snapshots.iter().zip(decoded.snapshots.iter()) {
            assert_eq!(orig.tick, dec.tick);
            assert!((orig.tension - dec.tension).abs() < f64::EPSILON);
            for (fid, orig_fs) in &orig.faction_states {
                let dec_fs = dec
                    .faction_states
                    .get(fid)
                    .expect("faction should exist in decoded snapshot");
                assert!(
                    (orig_fs.morale - dec_fs.morale).abs() < f64::EPSILON,
                    "morale mismatch at tick {}",
                    orig.tick
                );
            }
        }
    }

    #[test]
    fn region_control_change_tracked() {
        let snap1 = make_snapshot(10, 0.8, 0.5);
        let mut snap2 = make_snapshot(20, 0.8, 0.5);

        let rid = RegionId::from("region-a");
        let rebel = FactionId::from("rebel");
        snap2.region_control.insert(rid.clone(), Some(rebel));

        let delta = diff_snapshots(&snap1, &snap2);
        assert_eq!(
            delta.region_control.len(),
            1,
            "changed region control should appear in delta"
        );

        // Roundtrip.
        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: 20,
            final_state: snap2.clone(),
            snapshots: vec![snap1, snap2.clone()],
            event_log: vec![],
            campaign_reports: Default::default(),
        };
        let decoded = decode_run(&encode_run(&run));
        assert_eq!(decoded.snapshots[1].region_control, snap2.region_control);
    }

    #[test]
    fn events_always_preserved() {
        let snap1 = make_snapshot(10, 0.8, 0.5);
        let mut snap2 = make_snapshot(20, 0.8, 0.5);
        snap2.events_fired_this_tick = vec![EventId::from("uprising")];

        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: 20,
            final_state: snap2.clone(),
            snapshots: vec![snap1, snap2],
            event_log: vec![],
            campaign_reports: Default::default(),
        };

        let decoded = decode_run(&encode_run(&run));
        assert_eq!(
            decoded.snapshots[1].events_fired_this_tick,
            vec![EventId::from("uprising")]
        );
    }

    #[test]
    fn event_log_preserved_through_roundtrip() {
        use faultline_types::stats::EventRecord;

        let snap = make_snapshot(10, 0.8, 0.5);
        let event_log = vec![
            EventRecord {
                tick: 2,
                event_id: EventId::from("crisis"),
            },
            EventRecord {
                tick: 5,
                event_id: EventId::from("uprising"),
            },
            EventRecord {
                tick: 5,
                event_id: EventId::from("crisis"),
            },
        ];

        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: 10,
            final_state: snap.clone(),
            snapshots: vec![snap],
            event_log: event_log.clone(),
            campaign_reports: Default::default(),
        };

        let encoded = encode_run(&run);
        assert_eq!(
            encoded.event_log.len(),
            3,
            "encoded should preserve event_log"
        );

        let decoded = decode_run(&encoded);
        assert_eq!(
            decoded.event_log.len(),
            3,
            "decoded should preserve event_log"
        );
        assert_eq!(decoded.event_log[0].tick, 2);
        assert_eq!(decoded.event_log[0].event_id, EventId::from("crisis"));
        assert_eq!(decoded.event_log[1].tick, 5);
        assert_eq!(decoded.event_log[2].event_id, EventId::from("crisis"));
    }

    #[test]
    fn infra_status_delta_tracked() {
        let mut snap1 = make_snapshot(10, 0.8, 0.5);
        let mut snap2 = make_snapshot(20, 0.8, 0.5);

        let iid = InfraId::from("power_grid");
        snap1.infra_status.insert(iid.clone(), 1.0);
        snap2.infra_status.insert(iid.clone(), 0.7); // Damaged.

        let delta = diff_snapshots(&snap1, &snap2);
        assert_eq!(
            delta.infra_status.len(),
            1,
            "changed infra should appear in delta"
        );
        assert!(
            (delta.infra_status[&iid] - 0.7).abs() < f64::EPSILON,
            "infra delta should contain new value"
        );
    }

    #[test]
    fn infra_status_unchanged_omitted() {
        let mut snap1 = make_snapshot(10, 0.8, 0.5);
        let mut snap2 = make_snapshot(20, 0.8, 0.5);

        let iid = InfraId::from("power_grid");
        snap1.infra_status.insert(iid.clone(), 1.0);
        snap2.infra_status.insert(iid, 1.0); // Same value.

        let delta = diff_snapshots(&snap1, &snap2);
        assert!(
            delta.infra_status.is_empty(),
            "unchanged infra should not appear in delta"
        );
    }

    #[test]
    fn infra_status_roundtrip() {
        let mut snap1 = make_snapshot(10, 0.8, 0.5);
        let mut snap2 = make_snapshot(20, 0.8, 0.5);

        let iid_a = InfraId::from("grid");
        let iid_b = InfraId::from("telecom");
        snap1.infra_status.insert(iid_a.clone(), 1.0);
        snap1.infra_status.insert(iid_b.clone(), 0.9);
        snap2.infra_status.insert(iid_a.clone(), 0.6); // Changed.
        snap2.infra_status.insert(iid_b.clone(), 0.9); // Unchanged.

        let run = RunResult {
            run_index: 0,
            seed: 42,
            outcome: Outcome {
                victor: None,
                victory_condition: None,
                final_tension: 0.5,
            },
            final_tick: 20,
            final_state: snap2.clone(),
            snapshots: vec![snap1, snap2],
            event_log: vec![],
            campaign_reports: Default::default(),
        };

        let decoded = decode_run(&encode_run(&run));
        assert!(
            (decoded.snapshots[1].infra_status[&iid_a] - 0.6).abs() < f64::EPSILON,
            "grid should be 0.6 after decode"
        );
        assert!(
            (decoded.snapshots[1].infra_status[&iid_b] - 0.9).abs() < f64::EPSILON,
            "telecom should be 0.9 (carried from snap1)"
        );
    }

    #[test]
    fn first_delta_includes_all_fields() {
        let mut snap = make_snapshot(10, 0.8, 0.5);
        let iid = InfraId::from("grid");
        snap.infra_status.insert(iid.clone(), 0.95);

        let delta = full_to_delta(&snap);

        // All fields should be present in first delta.
        assert_eq!(delta.tick, 10);
        assert_eq!(
            delta.faction_states.len(),
            1,
            "faction_states should be in first delta"
        );
        assert_eq!(
            delta.region_control.len(),
            1,
            "region_control should be in first delta"
        );
        assert_eq!(
            delta.infra_status.len(),
            1,
            "infra_status should be in first delta"
        );
        assert!((delta.tension - 0.5).abs() < f64::EPSILON);
    }
}
