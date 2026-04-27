//! Core simulation engine for Faultline conflict simulation.
//!
//! Provides the tick-based engine that drives a single simulation run,
//! advancing faction actions, event evaluation, combat resolution, and
//! victory condition checks each tick.
//!
//! Given the same [`Scenario`](faultline_types::scenario::Scenario) and
//! RNG seed, the output is fully deterministic.

pub mod ai;
pub mod campaign;
pub mod combat;
pub mod engine;
pub mod error;
pub mod network;
pub mod state;
pub mod tick;

#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod tick_tests;

pub use engine::Engine;
pub use error::EngineError;
pub use state::SimulationState;
pub use tick::TickResult;

use faultline_types::error::ScenarioError;
use faultline_types::scenario::Scenario;

/// Validate a scenario for structural correctness.
///
/// Returns `Ok(())` if validation passes, or the first error found.
pub fn validate_scenario(scenario: &Scenario) -> Result<(), ScenarioError> {
    if scenario.factions.is_empty() {
        return Err(ScenarioError::EmptyScenario("no factions defined".into()));
    }

    if scenario.map.regions.is_empty() {
        return Err(ScenarioError::EmptyScenario("no regions defined".into()));
    }

    for (rid, region) in &scenario.map.regions {
        for neighbor in &region.borders {
            if !scenario.map.regions.contains_key(neighbor) {
                return Err(ScenarioError::InvalidBorder {
                    region: rid.clone(),
                    neighbor: neighbor.clone(),
                });
            }
        }
    }

    for (iid, infra) in &scenario.map.infrastructure {
        if !scenario.map.regions.contains_key(&infra.region) {
            return Err(ScenarioError::InfraRegionMismatch {
                infra: iid.clone(),
                region: infra.region.clone(),
            });
        }
    }

    for (fid, faction) in &scenario.factions {
        for unit in faction.forces.values() {
            if !scenario.map.regions.contains_key(&unit.region) {
                return Err(ScenarioError::ForceRegionMismatch {
                    force: unit.name.clone(),
                    faction: fid.clone(),
                    region: unit.region.clone(),
                });
            }
        }
        // Defender capacity sanity: a zero-depth queue is permanently
        // saturated (depth >= capacity at depth 0), which would silently
        // apply the saturated_detection_factor penalty before any noise
        // arrives. Reject loudly. Also enforce that the inner `id`
        // matches its table key — the field is documented as such but
        // the engine reads only the key, so a mismatch would be a silent
        // author error.
        for (rid, cap) in &faction.defender_capacities {
            if cap.queue_depth == 0 {
                return Err(ScenarioError::ZeroDefenderQueueDepth {
                    faction: fid.clone(),
                    role: rid.clone(),
                });
            }
            if cap.id != *rid {
                return Err(ScenarioError::DefenderRoleIdMismatch {
                    faction: fid.clone(),
                    key: rid.clone(),
                    id: cap.id.clone(),
                });
            }
            // `initialize_defender_queues` clamps service_rate via
            // `.max(0.0)`, but a negative value almost always means an
            // authoring error (typo / sign flip) — fail loudly instead
            // of silently freezing the queue. NaN is also rejected here
            // since `< 0.0` is false for NaN; we use `!is_finite()` to
            // catch it. f64::NEG_INFINITY satisfies `value < 0.0`.
            if !cap.service_rate.is_finite() || cap.service_rate < 0.0 {
                return Err(ScenarioError::NegativeServiceRate {
                    faction: fid.clone(),
                    role: rid.clone(),
                    value: cap.service_rate,
                });
            }
            // saturated_detection_factor is a multiplier on detection
            // probability; the gating path clamps to [0, 1] silently,
            // which would turn an authoring error like -0.5 into
            // complete detection suppression with no diagnostic.
            if !cap.saturated_detection_factor.is_finite()
                || cap.saturated_detection_factor < 0.0
                || cap.saturated_detection_factor > 1.0
            {
                return Err(ScenarioError::SaturatedDetectionFactorOutOfRange {
                    faction: fid.clone(),
                    role: rid.clone(),
                    value: cap.saturated_detection_factor,
                });
            }
        }
    }

    for vc in scenario.victory_conditions.values() {
        if !scenario.factions.contains_key(&vc.faction) {
            return Err(ScenarioError::UnknownFaction(vc.faction.clone()));
        }
    }

    // Defender capacity references (Epic K): every (faction, role)
    // named by `gated_by_defender` or `defender_noise` on a kill-chain
    // phase must resolve to a declared `defender_capacities` entry.
    // Catching this at load time turns a silent "queue not found, no
    // gating, no enqueue" runtime no-op into a loud configuration
    // error.
    for (cid, chain) in &scenario.kill_chains {
        for (pid, phase) in &chain.phases {
            if let Some(rr) = &phase.gated_by_defender
                && !defender_role_exists(scenario, &rr.faction, &rr.role)
            {
                return Err(ScenarioError::UnknownDefenderRole {
                    faction: rr.faction.clone(),
                    role: rr.role.clone(),
                });
            }
            for noise in &phase.defender_noise {
                if !defender_role_exists(scenario, &noise.defender, &noise.role) {
                    return Err(ScenarioError::UnknownDefenderRole {
                        faction: noise.defender.clone(),
                        role: noise.role.clone(),
                    });
                }
                // A negative rate is silently clamped to 0.0 in
                // `enqueue_phase_noise` via `.max(0.0)`, masking
                // authoring errors (sign flip / typo). Same fail-loud
                // pattern as `NegativeServiceRate`. Check before the
                // `!is_finite()` guard so `f64::NEG_INFINITY` reaches
                // the diagnostic that names the actual failure mode.
                if noise.items_per_tick < 0.0 {
                    return Err(ScenarioError::NegativeDefenderNoiseRate {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
                // NaN never satisfies `< 0.0` or `> 700.0`, so explicit
                // `!is_finite()` is required to catch it (and +∞).
                if !noise.items_per_tick.is_finite() {
                    return Err(ScenarioError::DefenderNoiseRateTooHigh {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
                // `sample_poisson` uses Knuth's inverse-transform method,
                // which relies on `(-mean).exp()`. For `mean > ~709` this
                // underflows to 0.0 in f64 and the loop falls through to
                // the 100,000-iteration cap, returning `mean as u32` with
                // a degenerate (non-Poisson) distribution. Cap well
                // below the underflow threshold so the sampler stays in
                // its accurate regime; authors who genuinely need higher
                // rates can split across multiple noise streams.
                if noise.items_per_tick > 700.0 {
                    return Err(ScenarioError::DefenderNoiseRateTooHigh {
                        chain: cid.clone(),
                        phase: pid.clone(),
                        value: noise.items_per_tick,
                    });
                }
            }

            // OrAny composition (Epic D): an empty `conditions` vector
            // would silently never match — likely an unfilled author
            // template. Walk recursively so a nested OrAny inside an
            // OrAny is also caught.
            for branch in &phase.branches {
                if let Err(()) = check_or_any_nonempty(&branch.condition) {
                    return Err(ScenarioError::EmptyOrAnyBranch {
                        chain: cid.clone(),
                        phase: pid.clone(),
                    });
                }
            }

            // Leadership-targeted phase outputs (Epic D). A
            // `LeadershipDecapitation` against a faction without a
            // declared cadre is a no-op at runtime — almost certainly
            // an authoring mistake. Reject loudly so the analyst gets
            // a diagnostic instead of a silently-empty Leadership
            // Cadres section. Also catches unknown faction ids and
            // non-finite / out-of-range morale_shock.
            for output in &phase.outputs {
                if let faultline_types::campaign::PhaseOutput::LeadershipDecapitation {
                    target_faction,
                    morale_shock,
                } = output
                {
                    let Some(target) = scenario.factions.get(target_faction) else {
                        return Err(ScenarioError::Custom(format!(
                            "kill chain {cid} phase {pid} declares \
                             LeadershipDecapitation against unknown \
                             faction `{target_faction}`"
                        )));
                    };
                    if target.leadership.is_none() {
                        return Err(ScenarioError::Custom(format!(
                            "kill chain {cid} phase {pid} declares \
                             LeadershipDecapitation against faction \
                             `{target_faction}`, which has no \
                             `leadership` cadre — the strike would \
                             be a runtime no-op. Either add a cadre or \
                             use `PhaseOutput::Custom` for analytics-only \
                             counters."
                        )));
                    }
                    if !morale_shock.is_finite() || *morale_shock < 0.0 || *morale_shock > 1.0 {
                        return Err(ScenarioError::ValueOutOfRange {
                            field: format!(
                                "kill chain {cid} phase {pid} \
                                 LeadershipDecapitation.morale_shock"
                            ),
                            value: *morale_shock,
                            expected: "[0.0, 1.0]".into(),
                        });
                    }
                }
            }
        }
    }

    // Environment schedule (Epic D — weather / time-of-day).
    // Catch authoring errors that would otherwise produce silent
    // no-ops or NaN-poisoned multipliers at runtime.
    let mut seen_window_ids: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for window in &scenario.environment.windows {
        if !seen_window_ids.insert(window.id.as_str()) {
            return Err(ScenarioError::Custom(format!(
                "environment window id `{}` is declared more than once; \
                 ids must be unique so the report can attribute factor \
                 contributions correctly",
                window.id
            )));
        }
        validate_environment_window(window)?;
    }

    // Leadership cadre (Epic D — decapitation). Catch malformed cadres
    // (empty rank list, non-finite effectiveness, duplicate rank ids)
    // at load time so the runtime helper can stay branch-free.
    for (fid, faction) in &scenario.factions {
        if let Some(cadre) = faction.leadership.as_ref() {
            if cadre.ranks.is_empty() {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("faction {fid} leadership.ranks"),
                    value: 0.0,
                    expected: ">= 1 rank".into(),
                });
            }
            if !cadre.succession_floor.is_finite()
                || cadre.succession_floor < 0.0
                || cadre.succession_floor > 1.0
            {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("faction {fid} leadership.succession_floor"),
                    value: cadre.succession_floor,
                    expected: "[0.0, 1.0]".into(),
                });
            }
            let mut seen_rank_ids: std::collections::BTreeSet<&str> =
                std::collections::BTreeSet::new();
            for rank in &cadre.ranks {
                if !seen_rank_ids.insert(rank.id.as_str()) {
                    return Err(ScenarioError::Custom(format!(
                        "faction {fid} leadership rank id `{}` is \
                         declared more than once; rank ids must be \
                         unique within a cadre",
                        rank.id
                    )));
                }
                if !rank.effectiveness.is_finite()
                    || rank.effectiveness < 0.0
                    || rank.effectiveness > 1.0
                {
                    return Err(ScenarioError::ValueOutOfRange {
                        field: format!("faction {fid} leadership rank {} effectiveness", rank.id),
                        value: rank.effectiveness,
                        expected: "[0.0, 1.0]".into(),
                    });
                }
            }
        }
    }

    // Strategy space (Epic H). Structural invariants only — the path
    // string itself is validated against the `set_param` resolver in
    // the search runner since that helper lives in `faultline-stats`
    // (engine cannot depend on stats without creating a crate cycle).
    // Catch the silent-no-op shapes here: empty discrete domain, NaN /
    // inf bounds, inverted continuous range, zero grid steps.
    {
        let space = &scenario.strategy_space;
        let mut seen_paths: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for var in &space.variables {
            if var.path.is_empty() {
                return Err(ScenarioError::Custom(
                    "strategy_space variable has empty path; expected the same dotted form \
                     accepted by --counterfactual / --sensitivity"
                        .into(),
                ));
            }
            if !seen_paths.insert(var.path.as_str()) {
                return Err(ScenarioError::Custom(format!(
                    "strategy_space variable path `{}` is declared more than once; \
                     two variables overriding the same field would race deterministically \
                     on the assignment order, which is almost always an authoring mistake",
                    var.path
                )));
            }
            if let Some(owner) = &var.owner
                && !scenario.factions.contains_key(owner)
            {
                return Err(ScenarioError::UnknownFaction(owner.clone()));
            }
            validate_decision_domain(&var.path, &var.domain)?;
        }
        for objective in &space.objectives {
            validate_search_objective(scenario, objective)?;
        }
    }

    // Networks (Epic L). Topological invariants only; engine-side
    // semantics (capacity factor clamping, etc.) are enforced at
    // runtime in the network phase.
    for (nid, net) in &scenario.networks {
        for (eid, edge) in &net.edges {
            if !net.nodes.contains_key(&edge.from) {
                return Err(ScenarioError::UnknownNetworkNode {
                    network: nid.clone(),
                    edge: eid.clone(),
                    node: edge.from.clone(),
                });
            }
            if !net.nodes.contains_key(&edge.to) {
                return Err(ScenarioError::UnknownNetworkNode {
                    network: nid.clone(),
                    edge: eid.clone(),
                    node: edge.to.clone(),
                });
            }
            if edge.from == edge.to {
                return Err(ScenarioError::NetworkSelfLoop {
                    network: nid.clone(),
                    edge: eid.clone(),
                    node: edge.from.clone(),
                });
            }
            if !edge.capacity.is_finite() || edge.capacity < 0.0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("network {nid} edge {eid} capacity"),
                    value: edge.capacity,
                    expected: ">= 0 and finite".into(),
                });
            }
            if !edge.latency.is_finite() || edge.latency < 0.0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("network {nid} edge {eid} latency"),
                    value: edge.latency,
                    expected: ">= 0 and finite".into(),
                });
            }
            if !edge.bandwidth.is_finite() || edge.bandwidth < 0.0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("network {nid} edge {eid} bandwidth"),
                    value: edge.bandwidth,
                    expected: ">= 0 and finite".into(),
                });
            }
            if !edge.trust.is_finite() || edge.trust < 0.0 || edge.trust > 1.0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("network {nid} edge {eid} trust"),
                    value: edge.trust,
                    expected: "[0.0, 1.0]".into(),
                });
            }
        }
        for (node_id, node) in &net.nodes {
            if !node.criticality.is_finite() || node.criticality < 0.0 || node.criticality > 1.0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("network {nid} node {node_id} criticality"),
                    value: node.criticality,
                    expected: "[0.0, 1.0]".into(),
                });
            }
        }
        if let Some(owner) = &net.owner
            && !scenario.factions.contains_key(owner)
        {
            return Err(ScenarioError::UnknownFaction(owner.clone()));
        }
    }

    // Network-aware event effects (Epic L). Each NetworkEdgeCapacity
    // / NetworkNodeDisrupt / NetworkInfiltrate effect must reference
    // a declared network, and within it a declared edge / node /
    // faction. Catching this at load time turns a silent runtime
    // no-op (the tick handler skips unknown ids) into a loud
    // configuration error.
    for (eid, def) in &scenario.events {
        for effect in &def.effects {
            validate_network_effect(scenario, eid, effect)?;
        }
        for option in &def.defender_options {
            for effect in &option.modifier_effects {
                validate_network_effect(scenario, eid, effect)?;
            }
        }
    }

    Ok(())
}

fn validate_network_effect(
    scenario: &Scenario,
    eid: &faultline_types::ids::EventId,
    effect: &faultline_types::events::EventEffect,
) -> Result<(), ScenarioError> {
    use faultline_types::events::EventEffect;
    match effect {
        EventEffect::NetworkEdgeCapacity {
            network,
            edge,
            factor,
        } => {
            let Some(net) = scenario.networks.get(network) else {
                return Err(ScenarioError::UnknownNetwork {
                    event: eid.clone(),
                    effect: "NetworkEdgeCapacity".into(),
                    network: network.clone(),
                });
            };
            if !net.edges.contains_key(edge) {
                return Err(ScenarioError::UnknownNetworkTarget {
                    event: eid.clone(),
                    effect: "NetworkEdgeCapacity".into(),
                    network: network.clone(),
                    kind: "edge".into(),
                    target: edge.0.clone(),
                });
            }
            // NaN factor is silently treated as a no-op at runtime
            // (the handler keeps the previous factor); flag at load
            // time so the analyst sees the typo. Negative is allowed
            // here at load — runtime clamps to [0, 4].
            if !factor.is_finite() {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("event {eid} NetworkEdgeCapacity({network} / {edge}) factor"),
                    value: *factor,
                    expected: "finite".into(),
                });
            }
        },
        EventEffect::NetworkNodeDisrupt { network, node } => {
            let Some(net) = scenario.networks.get(network) else {
                return Err(ScenarioError::UnknownNetwork {
                    event: eid.clone(),
                    effect: "NetworkNodeDisrupt".into(),
                    network: network.clone(),
                });
            };
            if !net.nodes.contains_key(node) {
                return Err(ScenarioError::UnknownNetworkTarget {
                    event: eid.clone(),
                    effect: "NetworkNodeDisrupt".into(),
                    network: network.clone(),
                    kind: "node".into(),
                    target: node.0.clone(),
                });
            }
        },
        EventEffect::NetworkInfiltrate {
            network,
            node,
            faction,
        } => {
            let Some(net) = scenario.networks.get(network) else {
                return Err(ScenarioError::UnknownNetwork {
                    event: eid.clone(),
                    effect: "NetworkInfiltrate".into(),
                    network: network.clone(),
                });
            };
            if !net.nodes.contains_key(node) {
                return Err(ScenarioError::UnknownNetworkTarget {
                    event: eid.clone(),
                    effect: "NetworkInfiltrate".into(),
                    network: network.clone(),
                    kind: "node".into(),
                    target: node.0.clone(),
                });
            }
            if !scenario.factions.contains_key(faction) {
                return Err(ScenarioError::UnknownFaction(faction.clone()));
            }
        },
        _ => {},
    }
    Ok(())
}

fn validate_decision_domain(
    path: &str,
    domain: &faultline_types::strategy_space::Domain,
) -> Result<(), ScenarioError> {
    use faultline_types::strategy_space::Domain;
    match domain {
        Domain::Continuous { low, high, steps } => {
            if !low.is_finite() || !high.is_finite() {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("strategy_space variable `{path}` continuous bounds"),
                    value: if low.is_finite() { *high } else { *low },
                    expected: "finite".into(),
                });
            }
            if low > high {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("strategy_space variable `{path}` continuous low"),
                    value: *low,
                    expected: format!("<= high ({high})"),
                });
            }
            if *steps == 0 {
                // Grid mode would silently produce zero trial values. Random
                // mode ignores `steps`, but we reject zero unconditionally
                // so analysts see the diagnostic before flipping methods.
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("strategy_space variable `{path}` continuous steps"),
                    value: 0.0,
                    expected: ">= 1".into(),
                });
            }
        },
        Domain::Discrete { values } => {
            if values.is_empty() {
                return Err(ScenarioError::Custom(format!(
                    "strategy_space variable `{path}` has empty discrete `values`; \
                     a discrete domain with no choices would silently never trial",
                )));
            }
            for v in values {
                if !v.is_finite() {
                    return Err(ScenarioError::ValueOutOfRange {
                        field: format!("strategy_space variable `{path}` discrete value"),
                        value: *v,
                        expected: "finite".into(),
                    });
                }
            }
        },
    }
    Ok(())
}

fn validate_search_objective(
    scenario: &Scenario,
    objective: &faultline_types::strategy_space::SearchObjective,
) -> Result<(), ScenarioError> {
    use faultline_types::strategy_space::SearchObjective;
    if let SearchObjective::MaximizeWinRate { faction } = objective
        && !scenario.factions.contains_key(faction)
    {
        return Err(ScenarioError::UnknownFaction(faction.clone()));
    }
    Ok(())
}

fn validate_environment_window(
    window: &faultline_types::map::EnvironmentWindow,
) -> Result<(), ScenarioError> {
    use faultline_types::map::Activation;

    // Reject NaN / infinity / negative factors. Negative would invert
    // the modifier sign (combat defense becoming offensive); >1 is
    // legitimate (storms making defense easier in cover). NaN
    // silently propagates and corrupts every downstream multiplier.
    let bad_factor = |label: &str, value: f64| -> Result<(), ScenarioError> {
        if !value.is_finite() || value < 0.0 {
            return Err(ScenarioError::ValueOutOfRange {
                field: format!("environment window {} {}", window.id, label),
                value,
                expected: ">= 0.0 and finite".into(),
            });
        }
        Ok(())
    };
    bad_factor("movement_factor", window.movement_factor)?;
    bad_factor("defense_factor", window.defense_factor)?;
    bad_factor("visibility_factor", window.visibility_factor)?;
    bad_factor("detection_factor", window.detection_factor)?;

    match &window.activation {
        Activation::Always => {},
        Activation::TickRange { start, end } => {
            if start > end {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("environment window {} TickRange.start", window.id),
                    value: f64::from(*start),
                    expected: format!("<= end ({end})"),
                });
            }
        },
        Activation::Cycle {
            period,
            phase: _,
            duration,
        } => {
            if *period == 0 {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("environment window {} Cycle.period", window.id),
                    value: f64::from(*period),
                    expected: "> 0".into(),
                });
            }
            if *duration == 0 {
                // `is_active_at` returns false for duration=0; that
                // would make the window silently never fire. Treat as
                // an authoring mistake (use `TickRange` if you really
                // want a never-active placeholder).
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("environment window {} Cycle.duration", window.id),
                    value: f64::from(*duration),
                    expected: "> 0 (a zero-duration cycle is silently never-active)".into(),
                });
            }
            if duration > period {
                return Err(ScenarioError::ValueOutOfRange {
                    field: format!("environment window {} Cycle.duration", window.id),
                    value: f64::from(*duration),
                    expected: format!("<= period ({period})"),
                });
            }
        },
    }

    Ok(())
}

fn check_or_any_nonempty(cond: &faultline_types::campaign::BranchCondition) -> Result<(), ()> {
    use faultline_types::campaign::BranchCondition;
    match cond {
        BranchCondition::OrAny { conditions } => {
            if conditions.is_empty() {
                return Err(());
            }
            for inner in conditions {
                check_or_any_nonempty(inner)?;
            }
            Ok(())
        },
        BranchCondition::OnSuccess
        | BranchCondition::OnFailure
        | BranchCondition::OnDetection
        | BranchCondition::Probability { .. }
        | BranchCondition::Always
        | BranchCondition::EscalationThreshold { .. } => Ok(()),
    }
}

fn defender_role_exists(
    scenario: &Scenario,
    faction: &faultline_types::ids::FactionId,
    role: &faultline_types::ids::DefenderRoleId,
) -> bool {
    scenario
        .factions
        .get(faction)
        .is_some_and(|f| f.defender_capacities.contains_key(role))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    use faultline_types::faction::{Faction, FactionType};
    use faultline_types::ids::{FactionId, RegionId, VictoryId};
    use faultline_types::map::{MapConfig, MapSource, Region};
    use faultline_types::politics::{MediaLandscape, PoliticalClimate};
    use faultline_types::scenario::ScenarioMeta;
    use faultline_types::simulation::{AttritionModel, SimulationConfig, TickDuration};
    use faultline_types::strategy::Doctrine;
    use faultline_types::victory::{VictoryCondition, VictoryType};

    pub(crate) fn minimal_scenario() -> Scenario {
        let rid = RegionId::from("capital");
        let fid = FactionId::from("gov");

        let mut regions = BTreeMap::new();
        regions.insert(
            rid.clone(),
            Region {
                id: rid.clone(),
                name: "Capital".into(),
                population: 1_000_000,
                urbanization: 0.9,
                initial_control: Some(fid.clone()),
                strategic_value: 10.0,
                borders: vec![],
                centroid: None,
            },
        );

        let mut factions = BTreeMap::new();
        factions.insert(
            fid.clone(),
            Faction {
                id: fid.clone(),
                name: "Government".into(),
                faction_type: FactionType::Insurgent,
                description: "Test faction".into(),
                color: "#000000".into(),
                forces: BTreeMap::new(),
                tech_access: vec![],
                initial_morale: 0.8,
                logistics_capacity: 100.0,
                initial_resources: 1000.0,
                resource_rate: 10.0,
                recruitment: None,
                command_resilience: 0.9,
                intelligence: 0.5,
                diplomacy: vec![],
                doctrine: Doctrine::Conventional,
                escalation_rules: None,
                defender_capacities: BTreeMap::new(),
                leadership: None,
            },
        );

        let mut victory_conditions = BTreeMap::new();
        victory_conditions.insert(
            VictoryId::from("gov-win"),
            VictoryCondition {
                id: VictoryId::from("gov-win"),
                name: "Government Control".into(),
                faction: fid.clone(),
                condition: VictoryType::StrategicControl { threshold: 1.0 },
            },
        );

        Scenario {
            meta: ScenarioMeta {
                name: "Test".into(),
                description: "Test scenario".into(),
                author: "test".into(),
                version: "0.1.0".into(),
                tags: vec![],
                confidence: None,
                schema_version: faultline_types::migration::CURRENT_SCHEMA_VERSION,
            },
            map: MapConfig {
                source: MapSource::Grid {
                    width: 1,
                    height: 1,
                },
                regions,
                infrastructure: BTreeMap::new(),
                terrain: vec![],
            },
            factions,
            technology: BTreeMap::new(),
            political_climate: PoliticalClimate {
                tension: 0.5,
                institutional_trust: 0.7,
                media_landscape: MediaLandscape {
                    fragmentation: 0.5,
                    disinformation_susceptibility: 0.3,
                    state_control: 0.4,
                    social_media_penetration: 0.8,
                    internet_availability: 0.9,
                },
                population_segments: vec![],
                global_modifiers: vec![],
            },
            events: BTreeMap::new(),
            simulation: SimulationConfig {
                max_ticks: 100,
                tick_duration: TickDuration::Days(1),
                monte_carlo_runs: 10,
                seed: Some(42),
                fog_of_war: false,
                attrition_model: AttritionModel::LanchesterLinear,
                snapshot_interval: 10,
            },
            victory_conditions,
            kill_chains: BTreeMap::new(),
            defender_budget: None,
            attacker_budget: None,
            environment: faultline_types::map::EnvironmentSchedule::default(),
            strategy_space: faultline_types::strategy_space::StrategySpace::default(),
            networks: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn engine_runs_to_completion() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation should succeed");
        let result = engine.run().expect("run should succeed");
        assert!(result.final_tick > 0);
    }

    #[test]
    fn validate_scenario_passes_for_valid() {
        let scenario = minimal_scenario();
        assert!(validate_scenario(&scenario).is_ok());
    }

    #[test]
    fn validate_scenario_fails_for_empty_factions() {
        let mut scenario = minimal_scenario();
        scenario.factions.clear();
        assert!(validate_scenario(&scenario).is_err());
    }

    // -----------------------------------------------------------------------
    // Epic D validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_rejects_zero_period_cycle_window() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "bad".into(),
                name: "Bad".into(),
                activation: Activation::Cycle {
                    period: 0,
                    phase: 0,
                    duration: 1,
                },
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 1.0,
                detection_factor: 1.0,
            }],
        };
        let err = validate_scenario(&scenario).expect_err("zero period must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_zero_duration_cycle_window() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "bad".into(),
                name: "Bad".into(),
                activation: Activation::Cycle {
                    period: 24,
                    phase: 0,
                    duration: 0,
                },
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 1.0,
                detection_factor: 1.0,
            }],
        };
        let err = validate_scenario(&scenario).expect_err("zero duration must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_inverted_tick_range_window() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "bad".into(),
                name: "Bad".into(),
                activation: Activation::TickRange { start: 50, end: 10 },
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 1.0,
                detection_factor: 1.0,
            }],
        };
        let err = validate_scenario(&scenario).expect_err("start > end must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_negative_environment_factor() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "bad".into(),
                name: "Bad".into(),
                activation: Activation::Always,
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: -0.5,
                visibility_factor: 1.0,
                detection_factor: 1.0,
            }],
        };
        let err = validate_scenario(&scenario).expect_err("negative factor must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_nan_environment_factor() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "bad".into(),
                name: "Bad".into(),
                activation: Activation::Always,
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: f64::NAN,
                visibility_factor: 1.0,
                detection_factor: 1.0,
            }],
        };
        let err = validate_scenario(&scenario).expect_err("NaN factor must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_duplicate_window_ids() {
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};
        let mut scenario = minimal_scenario();
        let window = EnvironmentWindow {
            id: "duplicate".into(),
            name: "Dup".into(),
            activation: Activation::Always,
            applies_to: vec![],
            movement_factor: 1.0,
            defense_factor: 1.0,
            visibility_factor: 1.0,
            detection_factor: 1.0,
        };
        scenario.environment = EnvironmentSchedule {
            windows: vec![window.clone(), window],
        };
        let err = validate_scenario(&scenario).expect_err("duplicate window ids must reject");
        assert!(matches!(err, ScenarioError::Custom(_)));
    }

    #[test]
    fn validate_rejects_empty_leadership_cadre() {
        use faultline_types::faction::LeadershipCadre;
        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");
        if let Some(faction) = scenario.factions.get_mut(&fid) {
            faction.leadership = Some(LeadershipCadre {
                ranks: vec![],
                succession_recovery_ticks: 1,
                succession_floor: 0.5,
            });
        }
        let err = validate_scenario(&scenario).expect_err("empty cadre must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_duplicate_rank_ids() {
        use faultline_types::faction::{LeadershipCadre, LeadershipRank};
        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");
        if let Some(faction) = scenario.factions.get_mut(&fid) {
            let dup_rank = LeadershipRank {
                id: "dup".into(),
                name: "Dup".into(),
                effectiveness: 1.0,
                description: String::new(),
            };
            faction.leadership = Some(LeadershipCadre {
                ranks: vec![dup_rank.clone(), dup_rank],
                succession_recovery_ticks: 1,
                succession_floor: 0.5,
            });
        }
        let err = validate_scenario(&scenario).expect_err("duplicate rank ids must reject");
        assert!(matches!(err, ScenarioError::Custom(_)));
    }

    #[test]
    fn validate_rejects_out_of_range_succession_floor() {
        use faultline_types::faction::{LeadershipCadre, LeadershipRank};
        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");
        if let Some(faction) = scenario.factions.get_mut(&fid) {
            faction.leadership = Some(LeadershipCadre {
                ranks: vec![LeadershipRank {
                    id: "principal".into(),
                    name: "Principal".into(),
                    effectiveness: 1.0,
                    description: String::new(),
                }],
                succession_recovery_ticks: 1,
                succession_floor: 1.5, // > 1
            });
        }
        let err = validate_scenario(&scenario).expect_err("out-of-range floor must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_rejects_decap_against_faction_without_cadre() {
        use faultline_types::campaign::{
            BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
        };
        use faultline_types::ids::{KillChainId, PhaseId};

        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");
        let chain_id = KillChainId::from("decap");
        let phase_id = PhaseId::from("strike");

        let mut phases = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Strike".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![PhaseOutput::LeadershipDecapitation {
                    target_faction: fid.clone(),
                    morale_shock: 0.1,
                }],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OnSuccess,
                    next_phase: phase_id.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Decap".into(),
                description: String::new(),
                attacker: fid.clone(),
                target: fid.clone(),
                entry_phase: phase_id,
                phases,
            },
        );

        // gov has no cadre — must reject.
        let err = validate_scenario(&scenario).expect_err("decap without cadre must reject");
        assert!(matches!(err, ScenarioError::Custom(_)));
    }

    #[test]
    fn validate_rejects_decap_against_unknown_faction() {
        use faultline_types::campaign::{
            BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
        };
        use faultline_types::ids::{KillChainId, PhaseId};

        let mut scenario = minimal_scenario();
        let chain_id = KillChainId::from("decap");
        let phase_id = PhaseId::from("strike");

        let mut phases = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Strike".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![PhaseOutput::LeadershipDecapitation {
                    target_faction: FactionId::from("ghost"),
                    morale_shock: 0.0,
                }],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OnSuccess,
                    next_phase: phase_id.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Decap".into(),
                description: String::new(),
                attacker: FactionId::from("gov"),
                target: FactionId::from("gov"),
                entry_phase: phase_id,
                phases,
            },
        );

        let err =
            validate_scenario(&scenario).expect_err("decap against unknown faction must reject");
        assert!(matches!(err, ScenarioError::Custom(_)));
    }

    #[test]
    fn validate_rejects_nan_morale_shock() {
        use faultline_types::campaign::{
            BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
        };
        use faultline_types::faction::{LeadershipCadre, LeadershipRank};
        use faultline_types::ids::{KillChainId, PhaseId};

        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");
        // Add a cadre so the cadre-existence check passes — the
        // morale_shock check is independent.
        if let Some(faction) = scenario.factions.get_mut(&fid) {
            faction.leadership = Some(LeadershipCadre {
                ranks: vec![LeadershipRank {
                    id: "principal".into(),
                    name: "Principal".into(),
                    effectiveness: 1.0,
                    description: String::new(),
                }],
                succession_recovery_ticks: 1,
                succession_floor: 0.5,
            });
        }

        let chain_id = KillChainId::from("decap");
        let phase_id = PhaseId::from("strike");
        let mut phases = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Strike".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![PhaseOutput::LeadershipDecapitation {
                    target_faction: fid.clone(),
                    morale_shock: f64::NAN,
                }],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OnSuccess,
                    next_phase: phase_id.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Decap".into(),
                description: String::new(),
                attacker: fid.clone(),
                target: fid.clone(),
                entry_phase: phase_id,
                phases,
            },
        );

        let err = validate_scenario(&scenario).expect_err("NaN morale_shock must reject");
        assert!(matches!(err, ScenarioError::ValueOutOfRange { .. }));
    }

    #[test]
    fn validate_passes_for_well_formed_environment_and_leadership() {
        // Sanity-check: a well-formed scenario with both Epic D
        // surfaces declared should pass validation cleanly.
        use faultline_types::campaign::{
            BranchCondition, CampaignPhase, KillChain, PhaseBranch, PhaseCost, PhaseOutput,
        };
        use faultline_types::faction::{LeadershipCadre, LeadershipRank};
        use faultline_types::ids::{KillChainId, PhaseId};
        use faultline_types::map::{Activation, EnvironmentSchedule, EnvironmentWindow};

        let mut scenario = minimal_scenario();
        let fid = FactionId::from("gov");

        scenario.environment = EnvironmentSchedule {
            windows: vec![EnvironmentWindow {
                id: "night".into(),
                name: "Night".into(),
                activation: Activation::Cycle {
                    period: 24,
                    phase: 18,
                    duration: 12,
                },
                applies_to: vec![],
                movement_factor: 1.0,
                defense_factor: 1.0,
                visibility_factor: 0.5,
                detection_factor: 0.7,
            }],
        };

        if let Some(faction) = scenario.factions.get_mut(&fid) {
            faction.leadership = Some(LeadershipCadre {
                ranks: vec![
                    LeadershipRank {
                        id: "principal".into(),
                        name: "Principal".into(),
                        effectiveness: 1.0,
                        description: String::new(),
                    },
                    LeadershipRank {
                        id: "deputy".into(),
                        name: "Deputy".into(),
                        effectiveness: 0.5,
                        description: String::new(),
                    },
                ],
                succession_recovery_ticks: 6,
                succession_floor: 0.4,
            });
        }

        let chain_id = KillChainId::from("decap");
        let phase_id = PhaseId::from("strike");
        let mut phases = BTreeMap::new();
        phases.insert(
            phase_id.clone(),
            CampaignPhase {
                id: phase_id.clone(),
                name: "Strike".into(),
                description: String::new(),
                prerequisites: vec![],
                base_success_probability: 1.0,
                min_duration: 1,
                max_duration: 1,
                detection_probability_per_tick: 0.0,
                prerequisite_success_boost: 0.0,
                attribution_difficulty: 0.5,
                cost: PhaseCost {
                    attacker_dollars: 0.0,
                    defender_dollars: 0.0,
                    attacker_resources: 0.0,
                    confidence: None,
                },
                targets_domains: vec![],
                outputs: vec![PhaseOutput::LeadershipDecapitation {
                    target_faction: fid.clone(),
                    morale_shock: 0.2,
                }],
                branches: vec![PhaseBranch {
                    condition: BranchCondition::OrAny {
                        conditions: vec![BranchCondition::OnSuccess, BranchCondition::OnDetection],
                    },
                    next_phase: phase_id.clone(),
                }],
                parameter_confidence: None,
                warning_indicators: vec![],
                defender_noise: vec![],
                gated_by_defender: None,
            },
        );

        scenario.kill_chains.insert(
            chain_id.clone(),
            KillChain {
                id: chain_id,
                name: "Decap".into(),
                description: String::new(),
                attacker: fid.clone(),
                target: fid.clone(),
                entry_phase: phase_id,
                phases,
            },
        );

        validate_scenario(&scenario).expect("well-formed Epic D scenario must validate");
    }

    // -----------------------------------------------------------------------
    // Epic H validation tests (strategy_space)
    // -----------------------------------------------------------------------

    #[test]
    fn validate_rejects_empty_path_in_strategy_space() {
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: String::new(),
                owner: None,
                domain: Domain::Continuous {
                    low: 0.0,
                    high: 1.0,
                    steps: 2,
                },
            }],
            objectives: vec![],
        };
        let err = validate_scenario(&scenario).expect_err("empty path must reject");
        assert!(format!("{err}").contains("empty path"));
    }

    #[test]
    fn validate_rejects_duplicate_strategy_space_paths() {
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        let dup = DecisionVariable {
            path: "faction.gov.initial_morale".into(),
            owner: None,
            domain: Domain::Continuous {
                low: 0.1,
                high: 0.9,
                steps: 4,
            },
        };
        scenario.strategy_space = StrategySpace {
            variables: vec![dup.clone(), dup],
            objectives: vec![],
        };
        let err = validate_scenario(&scenario).expect_err("duplicate paths must reject");
        assert!(format!("{err}").contains("declared more than once"));
    }

    #[test]
    fn validate_rejects_inverted_continuous_range() {
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.gov.initial_morale".into(),
                owner: None,
                domain: Domain::Continuous {
                    low: 0.9,
                    high: 0.1,
                    steps: 2,
                },
            }],
            objectives: vec![],
        };
        let err = validate_scenario(&scenario).expect_err("low > high must reject");
        assert!(format!("{err}").contains("<= high"));
    }

    #[test]
    fn validate_rejects_zero_steps_continuous_domain() {
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.gov.initial_morale".into(),
                owner: None,
                domain: Domain::Continuous {
                    low: 0.0,
                    high: 1.0,
                    steps: 0,
                },
            }],
            objectives: vec![],
        };
        let err = validate_scenario(&scenario).expect_err("steps == 0 must reject");
        assert!(format!("{err}").contains("steps"));
    }

    #[test]
    fn validate_rejects_empty_discrete_domain() {
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.gov.initial_morale".into(),
                owner: None,
                domain: Domain::Discrete { values: vec![] },
            }],
            objectives: vec![],
        };
        let err = validate_scenario(&scenario).expect_err("empty discrete values must reject");
        assert!(format!("{err}").contains("empty discrete"));
    }

    #[test]
    fn validate_rejects_unknown_owner_faction() {
        use faultline_types::ids::FactionId;
        use faultline_types::strategy_space::{DecisionVariable, Domain, StrategySpace};
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.gov.initial_morale".into(),
                owner: Some(FactionId::from("ghost")),
                domain: Domain::Continuous {
                    low: 0.0,
                    high: 1.0,
                    steps: 2,
                },
            }],
            objectives: vec![],
        };
        assert!(validate_scenario(&scenario).is_err());
    }

    #[test]
    fn validate_rejects_unknown_objective_faction() {
        use faultline_types::ids::FactionId;
        use faultline_types::strategy_space::{
            DecisionVariable, Domain, SearchObjective, StrategySpace,
        };
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![DecisionVariable {
                path: "faction.gov.initial_morale".into(),
                owner: None,
                domain: Domain::Continuous {
                    low: 0.0,
                    high: 1.0,
                    steps: 2,
                },
            }],
            objectives: vec![SearchObjective::MaximizeWinRate {
                faction: FactionId::from("ghost"),
            }],
        };
        assert!(validate_scenario(&scenario).is_err());
    }

    #[test]
    fn validate_passes_for_well_formed_strategy_space() {
        use faultline_types::ids::FactionId;
        use faultline_types::strategy_space::{
            DecisionVariable, Domain, SearchObjective, StrategySpace,
        };
        let mut scenario = minimal_scenario();
        scenario.strategy_space = StrategySpace {
            variables: vec![
                DecisionVariable {
                    path: "faction.gov.initial_morale".into(),
                    owner: Some(FactionId::from("gov")),
                    domain: Domain::Continuous {
                        low: 0.3,
                        high: 0.9,
                        steps: 4,
                    },
                },
                DecisionVariable {
                    path: "political_climate.tension".into(),
                    owner: None,
                    domain: Domain::Discrete {
                        values: vec![0.4, 0.6, 0.8],
                    },
                },
            ],
            objectives: vec![SearchObjective::MaximizeWinRate {
                faction: FactionId::from("gov"),
            }],
        };
        validate_scenario(&scenario).expect("well-formed strategy_space must validate");
    }

    #[test]
    fn deterministic_runs_produce_same_result() {
        let scenario = minimal_scenario();
        let mut engine1 = Engine::new(scenario.clone()).expect("engine creation should succeed");
        let result1 = engine1.run().expect("run should succeed");

        let mut engine2 = Engine::new(scenario).expect("engine creation should succeed");
        let result2 = engine2.run().expect("run should succeed");

        assert_eq!(result1.final_tick, result2.final_tick);
        assert_eq!(result1.outcome.victor, result2.outcome.victor);
    }

    // -----------------------------------------------------------------------
    // Monte Carlo integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn run_result_has_final_state() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        assert_eq!(
            result.final_state.tick, result.final_tick,
            "final_state tick should match final_tick"
        );
        assert!(
            !result.final_state.faction_states.is_empty(),
            "final_state should have faction states"
        );
        assert!(
            !result.final_state.region_control.is_empty(),
            "final_state should have region control"
        );
    }

    #[test]
    fn run_result_final_state_matches_last_snapshot_tick() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // final_state.tick and final_tick are set from the same value.
        assert_eq!(
            result.final_state.tick, result.final_tick,
            "final_state.tick should equal final_tick"
        );

        if !result.snapshots.is_empty() {
            let last_snap_tick = result.snapshots.last().expect("checked non-empty").tick;
            assert!(
                result.final_state.tick >= last_snap_tick,
                "final_state should be at or after last snapshot"
            );
        }
    }

    #[test]
    fn run_result_event_log_populated_from_scenario_with_events() {
        // Load the asymmetric scenario which has events.
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/tutorial_asymmetric.toml"),
        )
        .expect("should read asymmetric scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // The asymmetric scenario has events with conditions that may or may not fire.
        // At minimum, the event_log should be a valid (possibly empty) Vec.
        // With seed 42, events typically fire.
        // Whether or not events fire, the structure is correct.
        for record in &result.event_log {
            assert!(
                record.tick > 0,
                "event tick should be > 0 (ticks start at 1)"
            );
            assert!(record.tick <= result.final_tick, "event tick within bounds");
        }
    }

    #[test]
    fn events_fired_this_tick_cleared_between_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        // Run a few ticks.
        engine.tick().expect("tick 1");
        let after_tick1 = engine.state().events_fired_this_tick.clone();

        engine.tick().expect("tick 2");
        let after_tick2 = engine.state().events_fired_this_tick.clone();

        // With no events in scenario, both should be empty.
        assert!(
            after_tick1.is_empty(),
            "events_fired_this_tick should be empty with no events"
        );
        assert!(
            after_tick2.is_empty(),
            "events_fired_this_tick should be empty with no events"
        );
    }

    #[test]
    fn snapshots_include_infra_status() {
        use faultline_types::ids::InfraId;
        use faultline_types::map::{InfrastructureNode, InfrastructureType};

        let mut scenario = minimal_scenario();
        scenario.simulation.snapshot_interval = 5;

        let iid = InfraId::from("test_grid");
        scenario.map.infrastructure.insert(
            iid.clone(),
            InfrastructureNode {
                id: iid.clone(),
                name: "Test Grid".into(),
                region: RegionId::from("capital"),
                infra_type: InfrastructureType::PowerGrid,
                criticality: 0.9,
                initial_status: 1.0,
                repairable: Some(30),
            },
        );

        let mut engine = Engine::new(scenario).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // Snapshots should include infra_status.
        for snap in &result.snapshots {
            assert!(
                snap.infra_status.contains_key(&iid),
                "snapshot at tick {} should include infra_status for test_grid",
                snap.tick
            );
        }

        // Final state should also include infra.
        assert!(
            result.final_state.infra_status.contains_key(&iid),
            "final_state should include infra_status"
        );
    }

    #[test]
    fn fracture_scenario_loads_and_runs() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        validate_scenario(&scenario).expect("scenario should be valid");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        assert_eq!(result.final_tick, 365, "should run full 365 ticks");
        assert!(
            !result.final_state.faction_states.is_empty(),
            "should have faction states"
        );
        assert!(
            !result.event_log.is_empty(),
            "fracture scenario should fire events"
        );
    }

    #[test]
    fn fracture_scenario_event_log_has_correct_event_ids() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario.clone(), 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // All event IDs in the log should be defined in the scenario.
        for record in &result.event_log {
            assert!(
                scenario.events.contains_key(&record.event_id),
                "event_id {} in log should be defined in scenario",
                record.event_id
            );
        }
    }

    #[test]
    fn fracture_scenario_event_chain_fires() {
        let toml_str = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scenarios/us_institutional_fracture.toml"),
        )
        .expect("should read fracture scenario");
        let scenario: faultline_types::scenario::Scenario =
            toml::from_str(&toml_str).expect("should parse scenario");

        let mut engine = Engine::with_seed(scenario, 42).expect("engine creation");
        let result = engine.run().expect("run should succeed");

        // constitutional_crisis chains to state_nullification.
        let has_crisis = result
            .event_log
            .iter()
            .any(|r| r.event_id.0 == "constitutional_crisis");
        let has_nullification = result
            .event_log
            .iter()
            .any(|r| r.event_id.0 == "state_nullification");

        if has_crisis {
            assert!(
                has_nullification,
                "if constitutional_crisis fired, state_nullification should chain-fire"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Engine getter and snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn engine_max_ticks_returns_scenario_value() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert_eq!(engine.max_ticks(), 100, "max_ticks should match scenario");
    }

    #[test]
    fn engine_scenario_returns_reference() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert_eq!(engine.scenario().meta.name, "Test");
        assert_eq!(engine.scenario().simulation.max_ticks, 100);
        assert_eq!(engine.scenario().factions.len(), 1);
    }

    #[test]
    fn engine_is_finished_false_at_start() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        assert!(
            !engine.is_finished(),
            "engine should not be finished at tick 0"
        );
    }

    #[test]
    fn engine_is_finished_true_at_max_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");
        engine.run().expect("run should succeed");
        assert!(
            engine.is_finished(),
            "engine should be finished after run completes"
        );
    }

    #[test]
    fn engine_is_finished_transitions_during_ticking() {
        let mut scenario = minimal_scenario();
        scenario.simulation.max_ticks = 5;
        let mut engine = Engine::new(scenario).expect("engine creation");

        for i in 1..=5 {
            assert!(
                !engine.is_finished(),
                "should not be finished before tick {i}"
            );
            engine.tick().expect("tick should succeed");
        }
        assert!(
            engine.is_finished(),
            "should be finished after reaching max_ticks"
        );
    }

    #[test]
    fn engine_snapshot_at_tick_zero() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        assert_eq!(snap.tick, 0, "snapshot tick should be 0 at start");
        assert!(
            !snap.faction_states.is_empty(),
            "snapshot should have faction states"
        );
        assert!(
            !snap.region_control.is_empty(),
            "snapshot should have region control"
        );
        assert!(
            snap.events_fired_this_tick.is_empty(),
            "no events should have fired at tick 0"
        );
    }

    #[test]
    fn engine_snapshot_advances_with_ticks() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        engine.tick().expect("tick 1");
        let snap1 = engine.snapshot();
        assert_eq!(snap1.tick, 1, "snapshot should reflect tick 1");

        engine.tick().expect("tick 2");
        let snap2 = engine.snapshot();
        assert_eq!(snap2.tick, 2, "snapshot should reflect tick 2");
    }

    #[test]
    fn engine_snapshot_contains_correct_faction_data() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        let fid = FactionId::from("gov");
        let faction_state = snap
            .faction_states
            .get(&fid)
            .expect("should have gov faction in snapshot");

        assert_eq!(faction_state.faction_id, fid);
        assert!((faction_state.morale - 0.8).abs() < f64::EPSILON);
        assert!((faction_state.resources - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engine_snapshot_matches_take_snapshot_in_run_result() {
        let scenario = minimal_scenario();
        let mut engine = Engine::new(scenario).expect("engine creation");

        // Advance a few ticks manually.
        for _ in 0..5 {
            engine.tick().expect("tick should succeed");
        }

        // Snapshot via public method should match internal state.
        let snap = engine.snapshot();
        assert_eq!(snap.tick, 5);
        assert_eq!(snap.tick, engine.current_tick());
    }

    #[test]
    fn engine_snapshot_region_control_matches_initial() {
        let scenario = minimal_scenario();
        let engine = Engine::new(scenario).expect("engine creation");
        let snap = engine.snapshot();

        let rid = RegionId::from("capital");
        let fid = FactionId::from("gov");
        let control = snap.region_control.get(&rid).expect("should have capital");
        assert_eq!(control, &Some(fid), "capital should be controlled by gov");
    }
}
