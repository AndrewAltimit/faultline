//! The main simulation engine that drives the deterministic tick loop.

use std::collections::{BTreeMap, BTreeSet};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use faultline_events::EventEvaluator;
use faultline_geo::{self, GameMap};
use faultline_types::campaign::BranchCondition;
use faultline_types::ids::{EventId, FactionId, KillChainId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{EventRecord, Outcome, RunResult, StateSnapshot};
use faultline_types::strategy::FactionState;

use crate::campaign::{self, CampaignState};
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
    outcome_reached: bool,
    campaigns: BTreeMap<KillChainId, CampaignState>,
    /// Length of the metric history buffer required to evaluate every
    /// `BranchCondition::EscalationThreshold` in the scenario. Computed
    /// once at construction; `0` when no escalation branches exist (the
    /// hot path skips snapshot capture entirely).
    metric_history_depth: usize,
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
        let campaigns = campaign::initialize_campaigns(&scenario);
        let metric_history_depth = max_escalation_window(&scenario);

        Ok(Self {
            scenario,
            state,
            rng,
            map,
            event_evaluator,
            outcome_reached: false,
            campaigns,
            metric_history_depth,
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

        // Clear per-tick event log before event phase populates it.
        self.state.events_fired_this_tick.clear();

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

        // Capture an escalation-metric snapshot *before* the campaign
        // phase so a phase that resolves this tick reads `sustained_ticks`
        // counts that include the current tick. The snapshot is dropped
        // immediately when no scenario branch needs it.
        self.state.push_metric_snapshot(self.metric_history_depth);

        // Phase 7b: Campaigns / kill chains.
        if !self.scenario.kill_chains.is_empty() {
            campaign::campaign_phase(
                &mut self.state,
                &self.scenario,
                &mut self.campaigns,
                &mut self.rng,
            );
        }

        // Update region control after all modifications.
        tick::update_region_control(&mut self.state, &self.scenario);

        // Take snapshot if interval is hit.
        let interval = self.scenario.simulation.snapshot_interval;
        if interval > 0 && current_tick.is_multiple_of(interval) {
            self.state.snapshots.push(take_snapshot(&self.state));
        }

        // Phase 8: Victory check.
        let outcome = tick::victory_check(&self.state, &self.scenario);
        if outcome.is_some() {
            self.outcome_reached = true;
        }

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
        let mut event_log = Vec::new();

        loop {
            let result = self.tick()?;

            // Collect event records from this tick.
            let current_tick = self.state.tick;
            for eid in &self.state.events_fired_this_tick {
                event_log.push(EventRecord {
                    tick: current_tick,
                    event_id: eid.clone(),
                });
            }

            if let Some(outcome) = result.outcome {
                let final_state = take_snapshot(&self.state);
                return Ok(RunResult {
                    run_index: 0,
                    seed,
                    outcome,
                    final_tick: self.state.tick,
                    final_state,
                    snapshots: self.state.snapshots.clone(),
                    event_log,
                    campaign_reports: campaign::reports(&self.campaigns),
                });
            }

            if self.state.tick >= max_ticks {
                let outcome = Outcome {
                    victor: None,
                    victory_condition: None,
                    final_tension: self.state.political_climate.tension,
                };
                let final_state = take_snapshot(&self.state);
                return Ok(RunResult {
                    run_index: 0,
                    seed,
                    outcome,
                    final_tick: self.state.tick,
                    final_state,
                    snapshots: self.state.snapshots.clone(),
                    event_log,
                    campaign_reports: campaign::reports(&self.campaigns),
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

    /// Return the maximum tick count from the scenario.
    pub fn max_ticks(&self) -> u32 {
        self.scenario.simulation.max_ticks
    }

    /// Read-only access to the scenario.
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Take a snapshot of the current simulation state.
    pub fn snapshot(&self) -> StateSnapshot {
        take_snapshot(&self.state)
    }

    /// Read-only access to in-flight campaign state.
    pub fn campaigns(&self) -> &BTreeMap<KillChainId, CampaignState> {
        &self.campaigns
    }

    /// Check whether the simulation has finished (victory or max ticks).
    pub fn is_finished(&self) -> bool {
        self.outcome_reached || self.state.tick >= self.scenario.simulation.max_ticks
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
        events_fired_this_tick: Vec::new(),
        snapshots: Vec::new(),
        non_kinetic: Default::default(),
        metric_history: Vec::new(),
    })
}

/// Walk the scenario's branch graph and return the longest
/// `sustained_ticks` window any `EscalationThreshold` asks for.
///
/// `0` means no escalation-threshold branches exist anywhere — the
/// engine skips the per-tick metric snapshot in that case to keep the
/// hot path allocation-free for legacy scenarios.
fn max_escalation_window(scenario: &Scenario) -> usize {
    let mut max_window: u32 = 0;
    for chain in scenario.kill_chains.values() {
        for phase in chain.phases.values() {
            for branch in &phase.branches {
                if let BranchCondition::EscalationThreshold {
                    sustained_ticks, ..
                } = &branch.condition
                {
                    max_window = max_window.max(*sustained_ticks);
                }
            }
        }
    }
    // The hysteresis check needs the last `sustained_ticks` snapshots,
    // so size the buffer exactly to that. `0` is the no-branches
    // fallthrough — `push_metric_snapshot(0)` is a documented no-op.
    max_window as usize
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

    let events_this_tick: Vec<EventId> = state.events_fired_this_tick.clone();

    StateSnapshot {
        tick: state.tick,
        faction_states,
        region_control: state.region_control.clone(),
        infra_status: state.infra_status.clone(),
        tension: state.political_climate.tension,
        events_fired_this_tick: events_this_tick,
    }
}
