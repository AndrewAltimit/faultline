//! The main simulation engine that drives the deterministic tick loop.

use std::collections::{BTreeMap, BTreeSet};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use faultline_events::EventEvaluator;
use faultline_geo::{self, GameMap};
use faultline_types::ids::{EventId, FactionId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{Outcome, RunResult, StateSnapshot};
use faultline_types::strategy::FactionState;

use crate::error::EngineError;
use crate::state::{RuntimeFactionState, SimulationState};
use crate::tick::{self, TickResult};

/// The core simulation engine.
///
/// Given the same [`Scenario`] and RNG seed, the engine produces
/// identical output (deterministic).
pub struct Engine {
    scenario: Scenario,
    state: SimulationState,
    rng: ChaCha8Rng,
    map: GameMap,
    event_evaluator: EventEvaluator,
}

impl Engine {
    /// Create a new engine from a scenario definition.
    ///
    /// Initializes all runtime state from the scenario and seeds
    /// the RNG. Uses `seed = 0` if none is provided.
    /// Create an engine with an explicit seed override.
    ///
    /// The provided seed takes precedence over the scenario's
    /// `simulation.seed` field.
    pub fn with_seed(mut scenario: Scenario, seed: u64) -> Result<Self, EngineError> {
        scenario.simulation.seed = Some(seed);
        Self::new(scenario)
    }

    pub fn new(scenario: Scenario) -> Result<Self, EngineError> {
        if scenario.factions.is_empty() {
            return Err(EngineError::NoFactions);
        }
        if scenario.map.regions.is_empty() {
            return Err(EngineError::NoRegions);
        }

        let seed = scenario.simulation.seed.unwrap_or(0);
        let rng = ChaCha8Rng::seed_from_u64(seed);

        let map = faultline_geo::load_map(&scenario.map)?;

        let event_defs: Vec<_> = scenario.events.values().cloned().collect();
        let event_evaluator = EventEvaluator::new(event_defs)?;

        let state = initialize_state(&scenario)?;

        Ok(Self {
            scenario,
            state,
            rng,
            map,
            event_evaluator,
        })
    }

    /// Execute a single simulation tick.
    ///
    /// Runs all phases in order: events, decision, movement, combat,
    /// attrition, political, information, victory check.
    pub fn tick(&mut self) -> Result<TickResult, EngineError> {
        self.state.tick += 1;
        let current_tick = self.state.tick;

        tracing::debug!(tick = current_tick, "tick start");

        // Phase 1: Events.
        let events_fired = tick::event_phase(&mut self.state, &self.event_evaluator, &mut self.rng);

        // Phase 2: Decision (AI).
        let queued_actions =
            tick::decision_phase(&mut self.state, &self.scenario, &self.map, &mut self.rng);

        // Phase 3: Movement.
        tick::movement_phase(&mut self.state, &self.map, &queued_actions);

        // Phase 4: Combat.
        let combats_resolved = tick::combat_phase(&mut self.state, &self.scenario, &mut self.rng);

        // Phase 5: Attrition (resources, recruitment, repairs).
        tick::attrition_phase(&mut self.state, &self.scenario);

        // Phase 6: Political.
        tick::political_phase(&mut self.state, &self.scenario, &mut self.rng);

        // Phase 7: Information warfare.
        tick::information_phase(&mut self.state, &self.scenario);

        // Update region control after all modifications.
        tick::update_region_control(&mut self.state, &self.scenario);

        // Take snapshot if interval is hit.
        let interval = self.scenario.simulation.snapshot_interval;
        if interval > 0 && current_tick.is_multiple_of(interval) {
            self.state.snapshots.push(take_snapshot(&self.state));
        }

        // Phase 8: Victory check.
        let outcome = tick::victory_check(&self.state, &self.scenario);

        Ok(TickResult {
            tick: current_tick,
            events_fired,
            combats_resolved,
            outcome,
        })
    }

    /// Run the simulation until a victory condition is met or
    /// `max_ticks` is reached.
    pub fn run(&mut self) -> Result<RunResult, EngineError> {
        let max_ticks = self.scenario.simulation.max_ticks;
        let seed = self.scenario.simulation.seed.unwrap_or(0);

        loop {
            let result = self.tick()?;

            if let Some(outcome) = result.outcome {
                return Ok(RunResult {
                    run_index: 0,
                    seed,
                    outcome,
                    final_tick: self.state.tick,
                    snapshots: self.state.snapshots.clone(),
                });
            }

            if self.state.tick >= max_ticks {
                let outcome = Outcome {
                    victor: None,
                    victory_condition: None,
                    final_tension: self.state.political_climate.tension,
                };
                return Ok(RunResult {
                    run_index: 0,
                    seed,
                    outcome,
                    final_tick: self.state.tick,
                    snapshots: self.state.snapshots.clone(),
                });
            }
        }
    }

    /// Read-only access to the current simulation state.
    pub fn state(&self) -> &SimulationState {
        &self.state
    }

    /// Return the current tick number.
    pub fn current_tick(&self) -> u32 {
        self.state.tick
    }
}

// -----------------------------------------------------------------------
// Initialization
// -----------------------------------------------------------------------

/// Build the initial [`SimulationState`] from a [`Scenario`].
fn initialize_state(scenario: &Scenario) -> Result<SimulationState, EngineError> {
    let mut faction_states = BTreeMap::new();

    for (fid, faction) in &scenario.factions {
        let controlled_regions: Vec<_> = scenario
            .map
            .regions
            .iter()
            .filter(|(_, r)| r.initial_control.as_ref().is_some_and(|ctrl| ctrl == fid))
            .map(|(rid, _)| rid.clone())
            .collect();

        let total_strength: f64 = faction.forces.values().map(|f| f.strength).sum();

        faction_states.insert(
            fid.clone(),
            RuntimeFactionState {
                faction_id: fid.clone(),
                total_strength,
                morale: faction.initial_morale,
                resources: faction.initial_resources,
                resource_rate: faction.resource_rate,
                logistics_capacity: faction.logistics_capacity,
                controlled_regions,
                forces: faction.forces.clone(),
                tech_deployed: faction.tech_access.clone(),
                region_hold_ticks: BTreeMap::new(),
                eliminated: false,
            },
        );
    }

    let region_control: BTreeMap<_, _> = scenario
        .map
        .regions
        .iter()
        .map(|(rid, region)| (rid.clone(), region.initial_control.clone()))
        .collect();

    let infra_status: BTreeMap<_, _> = scenario
        .map
        .infrastructure
        .iter()
        .map(|(iid, node)| (iid.clone(), node.initial_status))
        .collect();

    let mut institution_loyalty = BTreeMap::new();
    for faction in scenario.factions.values() {
        if let faultline_types::faction::FactionType::Government { institutions } =
            &faction.faction_type
        {
            for (inst_id, inst) in institutions {
                institution_loyalty.insert(inst_id.clone(), inst.loyalty);
            }
        }
    }

    Ok(SimulationState {
        tick: 0,
        faction_states,
        region_control,
        infra_status,
        institution_loyalty,
        political_climate: scenario.political_climate.clone(),
        events_fired: BTreeSet::new(),
        snapshots: Vec::new(),
    })
}

/// Take a snapshot of the current simulation state.
fn take_snapshot(state: &SimulationState) -> StateSnapshot {
    let faction_states: BTreeMap<FactionId, FactionState> = state
        .faction_states
        .iter()
        .map(|(fid, rfs)| {
            (
                fid.clone(),
                FactionState {
                    faction_id: fid.clone(),
                    morale: rfs.morale,
                    resources: rfs.resources,
                    logistics_capacity: rfs.logistics_capacity,
                    tech_deployed: rfs.tech_deployed.clone(),
                    controlled_regions: rfs.controlled_regions.clone(),
                    total_strength: rfs.total_strength,
                    institution_loyalty: state.institution_loyalty.clone(),
                },
            )
        })
        .collect();

    let events_this_tick: Vec<EventId> = state.events_fired.iter().cloned().collect();

    StateSnapshot {
        tick: state.tick,
        faction_states,
        region_control: state.region_control.clone(),
        tension: state.political_climate.tension,
        events_fired_this_tick: events_this_tick,
    }
}
