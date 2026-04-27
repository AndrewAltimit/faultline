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
