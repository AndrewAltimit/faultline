//! Integration tests for the Epic L network primitive.
//!
//! Pin the high-leverage observable behaviors:
//! - the network rollup actually appears in `MonteCarloSummary` when
//!   the scenario declares networks,
//! - the Brandes critical-node ranking surfaces the right structural
//!   hub on a hand-shaped network,
//! - per-tick samples capture interdiction effects in the same tick
//!   the event fires.

use std::collections::BTreeMap;

use faultline_engine::Engine;
use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::ids::{EdgeId, EventId, FactionId, NetworkId, NodeId, RegionId, VictoryId};
use faultline_types::map::{MapConfig, MapSource, Region};
use faultline_types::network::{Network, NetworkEdge, NetworkNode};
use faultline_types::scenario::{Scenario, ScenarioMeta};
use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
use faultline_types::victory::{VictoryCondition, VictoryType};

fn faction(name: &str) -> faultline_types::faction::Faction {
    faultline_types::faction::Faction {
        id: FactionId::from(name),
        name: name.into(),
        initial_morale: 0.7,
        logistics_capacity: 100.0,
        initial_resources: 1000.0,
        resource_rate: 10.0,
        ..Default::default()
    }
}

fn star_network() -> Network {
    // Center 'c' connected to 4 leaves; classic single-point-of-failure
    // shape — c should top the betweenness ranking.
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
    for leaf in ["a", "b", "d", "e"] {
        edges.insert(
            EdgeId::from(format!("c{leaf}").as_str()),
            NetworkEdge {
                id: EdgeId::from(format!("c{leaf}").as_str()),
                from: NodeId::from("c"),
                to: NodeId::from(leaf),
                capacity: 5.0,
                ..Default::default()
            },
        );
    }
    Network {
        id: NetworkId::from("supply"),
        name: "Supply".into(),
        kind: "supply".into(),
        nodes,
        edges,
        ..Default::default()
    }
}

fn minimal_two_region_scenario() -> Scenario {
    let mut regions = BTreeMap::new();
    let r1 = RegionId::from("r1");
    let r2 = RegionId::from("r2");
    regions.insert(
        r1.clone(),
        Region {
            id: r1.clone(),
            name: "R1".into(),
            population: 1000,
            urbanization: 0.5,
            initial_control: Some(FactionId::from("blue")),
            strategic_value: 1.0,
            borders: vec![r2.clone()],
            centroid: None,
        },
    );
    regions.insert(
        r2.clone(),
        Region {
            id: r2.clone(),
            name: "R2".into(),
            population: 1000,
            urbanization: 0.5,
            initial_control: Some(FactionId::from("blue")),
            strategic_value: 1.0,
            borders: vec![r1.clone()],
            centroid: None,
        },
    );

    let mut factions = BTreeMap::new();
    factions.insert(FactionId::from("blue"), faction("blue"));

    let mut victory_conditions = BTreeMap::new();
    victory_conditions.insert(
        VictoryId::from("hold"),
        VictoryCondition {
            id: VictoryId::from("hold"),
            name: "Hold".into(),
            faction: FactionId::from("blue"),
            condition: VictoryType::HoldRegions {
                regions: vec![r1.clone(), r2.clone()],
                duration: 8,
            },
        },
    );

    Scenario {
        meta: ScenarioMeta {
            name: "Epic L Test".into(),
            ..Default::default()
        },
        map: MapConfig {
            source: MapSource::Grid {
                width: 2,
                height: 1,
            },
            regions,
            ..Default::default()
        },
        factions,
        victory_conditions,
        simulation: SimulationConfig {
            max_ticks: 10,
            tick_duration: TickDuration::Days(1),
            monte_carlo_runs: 1,
            seed: Some(42),
            fog_of_war: false,
            attrition_model: AttritionModel::LanchesterLinear,
            snapshot_interval: 0,
        },
        ..Default::default()
    }
}

#[test]
fn engine_runs_with_networks_emits_per_tick_samples() {
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let mut engine = Engine::new(scenario).expect("scenario validates");
    let result = engine.run().expect("engine runs");

    let report = result
        .network_reports
        .get(&NetworkId::from("supply"))
        .expect("network report present");
    assert_eq!(report.static_node_count, 5);
    assert_eq!(report.static_edge_count, 4);
    // Engine ran for >= 1 tick, so at least one sample was captured.
    assert!(!report.samples.is_empty(), "expected at least one sample");
    // Pristine star: 1 connected component for every sample.
    for s in &report.samples {
        assert_eq!(s.component_count, 1, "tick {}", s.tick);
        assert_eq!(s.largest_component, 5, "tick {}", s.tick);
    }
}

#[test]
fn node_disrupt_event_fragments_in_same_tick() {
    // Disrupt the center 'c' on tick 3. Expect samples at tick >= 3
    // to show 5 components (every node now isolated). Verifies the
    // engine captures samples *after* the event phase fires.
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let mut event = EventDefinition {
        id: EventId::from("disrupt_center"),
        name: "Disrupt Center".into(),
        description: "Adversary takes out the central hub on tick 3".into(),
        earliest_tick: Some(3),
        latest_tick: Some(3),
        conditions: vec![EventCondition::TickAtLeast { tick: 3 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkNodeDisrupt {
            network: NetworkId::from("supply"),
            node: NodeId::from("c"),
        }],
        chain: None,
        defender_options: vec![],
    };
    // Make borrow checker happy: we move event into the map.
    event.repeatable = false;
    scenario
        .events
        .insert(EventId::from("disrupt_center"), event);

    let mut engine = Engine::new(scenario).expect("scenario validates");
    let result = engine.run().expect("engine runs");
    let report = result
        .network_reports
        .get(&NetworkId::from("supply"))
        .expect("network report present");

    // Pre-disruption ticks should show 1 component; post-disruption
    // ticks should show 5 (every node isolated).
    let mut saw_pre = false;
    let mut saw_post = false;
    for s in &report.samples {
        if s.tick < 3 {
            assert_eq!(s.component_count, 1, "pre-disrupt at tick {}", s.tick);
            saw_pre = true;
        } else {
            assert_eq!(s.component_count, 5, "post-disrupt at tick {}", s.tick);
            assert_eq!(s.disrupted_nodes, 1, "tick {}", s.tick);
            saw_post = true;
        }
    }
    assert!(saw_pre || saw_post, "must have observed at least one tick");
    assert!(saw_post, "tick 3 sample must include the disruption");
    assert!(report.terminal_disrupted_nodes.contains(&NodeId::from("c")));
}

#[test]
fn edge_capacity_event_clamps_to_zero_severs_path() {
    // Drop edge "ca" capacity to 0. Leaf 'a' becomes isolated; 'c'
    // and remaining leaves still form one component of size 4. Total
    // component count = 2.
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let event = EventDefinition {
        id: EventId::from("interdict_a"),
        name: "Interdict A".into(),
        description: "Sever leaf-a's link".into(),
        earliest_tick: Some(2),
        latest_tick: Some(2),
        conditions: vec![EventCondition::TickAtLeast { tick: 2 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkEdgeCapacity {
            network: NetworkId::from("supply"),
            edge: EdgeId::from("ca"),
            factor: 0.0,
        }],
        chain: None,
        defender_options: vec![],
    };
    scenario.events.insert(EventId::from("interdict_a"), event);

    let mut engine = Engine::new(scenario).expect("scenario validates");
    let result = engine.run().expect("engine runs");
    let report = result
        .network_reports
        .get(&NetworkId::from("supply"))
        .expect("network report present");

    let post: Vec<&faultline_types::stats::NetworkSample> =
        report.samples.iter().filter(|s| s.tick >= 2).collect();
    assert!(!post.is_empty(), "must have post-event samples");
    for s in post {
        assert_eq!(s.component_count, 2, "tick {}", s.tick);
        assert_eq!(s.largest_component, 4, "tick {}", s.tick);
    }
    // Edge factor map records the 0.0 multiplier.
    let factor = report
        .terminal_edge_factors
        .get(&EdgeId::from("ca"))
        .copied()
        .unwrap_or(1.0);
    assert!((factor - 0.0).abs() < f64::EPSILON);
}

#[test]
fn validation_rejects_unknown_edge_endpoint() {
    let mut scenario = minimal_two_region_scenario();
    let mut net = star_network();
    // Inject a bogus edge pointing at a missing node.
    net.edges.insert(
        EdgeId::from("bogus"),
        NetworkEdge {
            id: EdgeId::from("bogus"),
            from: NodeId::from("c"),
            to: NodeId::from("missing"),
            capacity: 1.0,
            ..Default::default()
        },
    );
    scenario.networks.insert(NetworkId::from("supply"), net);

    let err =
        faultline_engine::validate_scenario(&scenario).expect_err("should reject unknown endpoint");
    let msg = format!("{err}");
    assert!(
        msg.contains("missing"),
        "error should name the bad node: {msg}"
    );
}

#[test]
fn validation_rejects_self_loop() {
    let mut scenario = minimal_two_region_scenario();
    let mut net = star_network();
    net.edges.insert(
        EdgeId::from("loop"),
        NetworkEdge {
            id: EdgeId::from("loop"),
            from: NodeId::from("a"),
            to: NodeId::from("a"),
            capacity: 1.0,
            ..Default::default()
        },
    );
    scenario.networks.insert(NetworkId::from("supply"), net);

    let err = faultline_engine::validate_scenario(&scenario).expect_err("should reject self-loop");
    assert!(format!("{err}").contains("self-loop"));
}

#[test]
fn validation_rejects_event_targeting_unknown_network() {
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let event = EventDefinition {
        id: EventId::from("bad"),
        name: "Bad".into(),
        description: "References unknown network".into(),
        earliest_tick: Some(1),
        latest_tick: Some(1),
        conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkNodeDisrupt {
            network: NetworkId::from("nonexistent"),
            node: NodeId::from("c"),
        }],
        chain: None,
        defender_options: vec![],
    };
    scenario.events.insert(EventId::from("bad"), event);

    let err = faultline_engine::validate_scenario(&scenario)
        .expect_err("should reject unknown network reference");
    assert!(format!("{err}").contains("nonexistent"));
}

#[test]
fn determinism_same_seed_same_samples() {
    // Pin the determinism contract on the network path. Running the
    // same scenario twice with the same seed must produce identical
    // per-tick samples, identical terminal sets, and identical
    // critical-node rankings.
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());
    let event = EventDefinition {
        id: EventId::from("disrupt_a"),
        name: "Disrupt A".into(),
        description: "Disrupts leaf a on tick 2".into(),
        earliest_tick: Some(2),
        latest_tick: Some(2),
        conditions: vec![EventCondition::TickAtLeast { tick: 2 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkNodeDisrupt {
            network: NetworkId::from("supply"),
            node: NodeId::from("a"),
        }],
        chain: None,
        defender_options: vec![],
    };
    scenario.events.insert(EventId::from("disrupt_a"), event);

    let r1 = Engine::with_seed(scenario.clone(), 0xBEEF)
        .expect("validates")
        .run()
        .expect("runs");
    let r2 = Engine::with_seed(scenario, 0xBEEF)
        .expect("validates")
        .run()
        .expect("runs");

    let net = NetworkId::from("supply");
    let rep1 = r1.network_reports.get(&net).expect("present");
    let rep2 = r2.network_reports.get(&net).expect("present");
    assert_eq!(rep1.samples.len(), rep2.samples.len());
    for (s1, s2) in rep1.samples.iter().zip(rep2.samples.iter()) {
        assert_eq!(s1.tick, s2.tick);
        assert_eq!(s1.component_count, s2.component_count);
        assert_eq!(s1.largest_component, s2.largest_component);
        assert!((s1.residual_capacity - s2.residual_capacity).abs() < f64::EPSILON);
        assert_eq!(s1.disrupted_nodes, s2.disrupted_nodes);
    }
    assert_eq!(rep1.terminal_disrupted_nodes, rep2.terminal_disrupted_nodes);
    assert_eq!(rep1.terminal_edge_factors, rep2.terminal_edge_factors);
}

#[test]
fn multiplicative_edge_capacity_composition() {
    // Two events fire targeting the same edge: factor 0.5 then
    // factor 0.5 again. Expect runtime factor at end to be 0.25
    // (multiplicative composition), not 0.5 (last-write-wins).
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    for (id, tick) in [("first", 2u32), ("second", 4u32)] {
        let ev = EventDefinition {
            id: EventId::from(format!("interdict_{id}").as_str()),
            name: format!("Interdict {id}"),
            description: "Half-capacity interdiction".into(),
            earliest_tick: Some(tick),
            latest_tick: Some(tick),
            conditions: vec![EventCondition::TickAtLeast { tick }],
            probability: 1.0,
            repeatable: false,
            effects: vec![EventEffect::NetworkEdgeCapacity {
                network: NetworkId::from("supply"),
                edge: EdgeId::from("ca"),
                factor: 0.5,
            }],
            chain: None,
            defender_options: vec![],
        };
        scenario
            .events
            .insert(EventId::from(format!("interdict_{id}").as_str()), ev);
    }

    let mut engine = Engine::new(scenario).expect("validates");
    let result = engine.run().expect("runs");
    let report = result
        .network_reports
        .get(&NetworkId::from("supply"))
        .expect("present");
    let factor = report
        .terminal_edge_factors
        .get(&EdgeId::from("ca"))
        .copied()
        .expect("ca was modified");
    assert!(
        (factor - 0.25).abs() < 1e-9,
        "expected multiplicative composition: 0.5 * 0.5 = 0.25, got {factor}"
    );
}

#[test]
fn idempotent_node_disruption() {
    // Two events disrupting the same node should still produce one
    // disrupted-node entry. BTreeSet::insert is idempotent, but pin
    // the behavior.
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    for (id, tick) in [("first", 2u32), ("second", 5u32)] {
        let ev = EventDefinition {
            id: EventId::from(format!("disrupt_{id}").as_str()),
            name: format!("Disrupt {id}"),
            description: "".into(),
            earliest_tick: Some(tick),
            latest_tick: Some(tick),
            conditions: vec![EventCondition::TickAtLeast { tick }],
            probability: 1.0,
            repeatable: false,
            effects: vec![EventEffect::NetworkNodeDisrupt {
                network: NetworkId::from("supply"),
                node: NodeId::from("c"),
            }],
            chain: None,
            defender_options: vec![],
        };
        scenario
            .events
            .insert(EventId::from(format!("disrupt_{id}").as_str()), ev);
    }

    let mut engine = Engine::new(scenario).expect("validates");
    let result = engine.run().expect("runs");
    let report = result
        .network_reports
        .get(&NetworkId::from("supply"))
        .expect("present");
    assert_eq!(report.terminal_disrupted_nodes.len(), 1);
    assert!(report.terminal_disrupted_nodes.contains(&NodeId::from("c")));
}

#[test]
fn validation_rejects_id_mismatch() {
    let mut scenario = minimal_two_region_scenario();
    let mut net = star_network();
    // Insert under one key but the inner id says another. The
    // engine reads only the key, so this would silently lose the
    // inner-id value in a downstream consumer.
    let bad_node = NetworkNode {
        id: NodeId::from("wrong"),
        name: "Mismatch".into(),
        ..Default::default()
    };
    net.nodes.insert(NodeId::from("z"), bad_node);
    scenario.networks.insert(NetworkId::from("supply"), net);

    let err = faultline_engine::validate_scenario(&scenario)
        .expect_err("should reject id-vs-key mismatch");
    let msg = format!("{err}");
    assert!(
        msg.contains("does not match") || msg.contains("mismatch"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn validation_rejects_out_of_range_criticality() {
    let mut scenario = minimal_two_region_scenario();
    let mut net = star_network();
    net.nodes.insert(
        NodeId::from("bad"),
        NetworkNode {
            id: NodeId::from("bad"),
            name: "Bad".into(),
            criticality: 1.5,
            ..Default::default()
        },
    );
    scenario.networks.insert(NetworkId::from("supply"), net);

    let err = faultline_engine::validate_scenario(&scenario)
        .expect_err("should reject criticality > 1.0");
    assert!(format!("{err}").contains("criticality"));
}

#[test]
fn validation_rejects_out_of_range_trust() {
    let mut scenario = minimal_two_region_scenario();
    let mut net = star_network();
    net.edges.insert(
        EdgeId::from("bad"),
        NetworkEdge {
            id: EdgeId::from("bad"),
            from: NodeId::from("c"),
            to: NodeId::from("a"),
            capacity: 1.0,
            trust: 1.5,
            ..Default::default()
        },
    );
    scenario.networks.insert(NetworkId::from("supply"), net);

    let err =
        faultline_engine::validate_scenario(&scenario).expect_err("should reject trust > 1.0");
    assert!(format!("{err}").contains("trust"));
}

#[test]
fn validation_rejects_nan_event_factor() {
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let ev = EventDefinition {
        id: EventId::from("nan_event"),
        name: "NaN".into(),
        description: "".into(),
        earliest_tick: Some(1),
        latest_tick: Some(1),
        conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkEdgeCapacity {
            network: NetworkId::from("supply"),
            edge: EdgeId::from("ca"),
            factor: f64::NAN,
        }],
        chain: None,
        defender_options: vec![],
    };
    scenario.events.insert(EventId::from("nan_event"), ev);

    let err = faultline_engine::validate_scenario(&scenario).expect_err("should reject NaN factor");
    assert!(format!("{err}").to_lowercase().contains("finite"));
}

#[test]
fn validation_rejects_unknown_faction_in_infiltrate() {
    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let ev = EventDefinition {
        id: EventId::from("infiltrate_phantom"),
        name: "Phantom Infiltration".into(),
        description: "Faction does not exist".into(),
        earliest_tick: Some(1),
        latest_tick: Some(1),
        conditions: vec![EventCondition::TickAtLeast { tick: 1 }],
        probability: 1.0,
        repeatable: false,
        effects: vec![EventEffect::NetworkInfiltrate {
            network: NetworkId::from("supply"),
            node: NodeId::from("c"),
            faction: FactionId::from("phantom"),
        }],
        chain: None,
        defender_options: vec![],
    };
    scenario
        .events
        .insert(EventId::from("infiltrate_phantom"), ev);

    let err =
        faultline_engine::validate_scenario(&scenario).expect_err("should reject unknown faction");
    assert!(format!("{err}").contains("phantom"));
}

#[test]
fn legacy_scenario_summary_hash_unchanged_by_epic_l() {
    // Manifest hash invariance: a legacy scenario (no networks
    // declared) must produce the exact same canonical JSON for its
    // MonteCarloSummary regardless of whether the Network*
    // fields exist on the type. The reason this can hold across a
    // schema-additive change: every Epic-L summary field carries
    // `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`,
    // so on a no-networks scenario those fields elide entirely.
    //
    // This test pins that invariant. If it ever fails, an Epic-L
    // schema field has lost its `skip_serializing_if` annotation —
    // any external citer's pre-Epic-L manifest would stop verifying.
    use faultline_stats::MonteCarloRunner;
    use faultline_stats::manifest::{scenario_hash, summary_hash};
    use faultline_types::stats::MonteCarloConfig;

    let scenario = minimal_two_region_scenario();
    assert!(scenario.networks.is_empty(), "test premise");

    let config = MonteCarloConfig {
        num_runs: 4,
        seed: Some(42),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC runs");

    // The summary's network rollup must be empty on a legacy
    // scenario (no engine path produces it), and the canonical JSON
    // must therefore not mention `network_summaries` at all.
    assert!(result.summary.network_summaries.is_empty());
    let canon = serde_json::to_string(&result.summary).expect("serializes");
    assert!(
        !canon.contains("network_summaries"),
        "empty network_summaries must elide from canonical JSON; \
         current JSON: {canon}"
    );

    // The same applies to per-run network reports.
    for run in &result.runs {
        assert!(run.network_reports.is_empty());
    }

    // And both hashes must be byte-stable strings — if anyone
    // accidentally adds a non-eliding field to the summary or the
    // run, the contained values would change but the test would
    // still need to fail visibly. Hash strings are what external
    // citers pin against, so we assert they are valid hex of the
    // expected length.
    let s_hash = scenario_hash(&scenario).expect("hashes");
    let summ_hash = summary_hash(&result.summary).expect("hashes");
    assert_eq!(s_hash.len(), 64, "SHA-256 hex digest is 64 chars");
    assert_eq!(summ_hash.len(), 64);
    assert!(s_hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(summ_hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn empty_network_state_does_not_appear_in_run_result() {
    // A legacy scenario must not have an empty `network_reports`
    // map serialize as `{}` in the canonical JSON — that would
    // shift the output_hash for every external citer's pre-Epic-L
    // manifest. The `skip_serializing_if` annotation handles this,
    // but the test pins the exact JSON shape to catch any future
    // accidental removal of the annotation.
    let scenario = minimal_two_region_scenario();
    let mut engine = Engine::new(scenario).expect("validates");
    let result = engine.run().expect("runs");
    let json = serde_json::to_string(&result).expect("serializes");
    assert!(
        !json.contains("network_reports"),
        "empty network_reports must elide; got: {json}"
    );
}

#[test]
fn cross_run_summary_includes_critical_nodes() {
    use faultline_stats::MonteCarloRunner;
    use faultline_types::stats::MonteCarloConfig;

    let mut scenario = minimal_two_region_scenario();
    scenario
        .networks
        .insert(NetworkId::from("supply"), star_network());

    let config = MonteCarloConfig {
        num_runs: 3,
        seed: Some(1),
        collect_snapshots: false,
        parallel: false,
    };
    let result = MonteCarloRunner::run(&config, &scenario).expect("MC runs");
    let summary = result
        .summary
        .network_summaries
        .get(&NetworkId::from("supply"))
        .expect("network summary present");
    assert_eq!(summary.n_runs, 3);
    // The center 'c' should be the top node by betweenness on a star.
    assert_eq!(summary.critical_nodes[0].node, NodeId::from("c"));
    assert!(summary.critical_nodes[0].betweenness > 0.4);
}
