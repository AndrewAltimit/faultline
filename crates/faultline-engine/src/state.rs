use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use faultline_types::faction::{Diplomacy, ForceUnit, UnitType};
use faultline_types::ids::{
    DefenderRoleId, EdgeId, EventId, FactionId, ForceId, InfraId, InstitutionId, NetworkId, NodeId,
    RegionId, TechCardId,
};
use faultline_types::politics::PoliticalClimate;
use faultline_types::stats::{NetworkSample, StateSnapshot};

/// All mutable runtime state for a running simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimulationState {
    /// Current tick number (starts at 0).
    pub tick: u32,
    /// Per-faction runtime state.
    pub faction_states: BTreeMap<FactionId, RuntimeFactionState>,
    /// Which faction (if any) controls each region.
    pub region_control: BTreeMap<RegionId, Option<FactionId>>,
    /// Infrastructure health in `[0.0, 1.0]`.
    pub infra_status: BTreeMap<InfraId, f64>,
    /// Institution loyalty in `[0.0, 1.0]`.
    pub institution_loyalty: BTreeMap<InstitutionId, f64>,
    /// Political climate (cloned from scenario, mutated at runtime).
    pub political_climate: PoliticalClimate,
    /// Set of event IDs that have already fired (cumulative, for one-shot guard).
    pub events_fired: BTreeSet<EventId>,
    /// Events that fired during the current tick (cleared each tick).
    pub events_fired_this_tick: Vec<EventId>,
    /// Periodic state snapshots for analysis.
    pub snapshots: Vec<StateSnapshot>,
    /// Aggregated non-kinetic metrics. Summed across all
    /// in-flight kill chains so scenario victory conditions can check
    /// them uniformly.
    #[serde(default)]
    pub non_kinetic: NonKineticMetrics,
    /// Rolling history of escalation-relevant metrics, used to evaluate
    /// `BranchCondition::EscalationThreshold` with hysteresis. Bounded
    /// to the longest `sustained_ticks` window any branch in the
    /// scenario asks for, plus a small safety margin — see
    /// [`Self::push_metric_snapshot`].
    #[serde(default)]
    pub metric_history: Vec<MetricSnapshot>,
    /// Per-(faction, role) defender investigative queue state (Epic K).
    /// Empty when no scenario faction declares `defender_capacities`;
    /// the campaign phase skips its queue-service step entirely in
    /// that case so legacy scenarios pay zero overhead.
    #[serde(default)]
    pub defender_queues: BTreeMap<FactionId, BTreeMap<DefenderRoleId, DefenderQueueState>>,
    /// Per-network runtime mutation state (Epic L). Empty when the
    /// scenario declares no networks; the network phase short-circuits
    /// in that case so legacy scenarios pay zero overhead.
    #[serde(default)]
    pub network_states: BTreeMap<NetworkId, NetworkRuntimeState>,
    /// First tick at which cumulative defender spend (summed across all
    /// in-flight kill chains) exceeded the scenario's `defender_budget`.
    /// `None` when the scenario set no budget, when the scenario set a
    /// budget the attacker never forced past, or before the threshold
    /// is crossed. Once set the value is sticky for the remainder of
    /// the run and gates a 0.5× detection-probability multiplier on all
    /// subsequent kill-chain phases — modelling a defender who has
    /// exhausted the funds available to close gaps and is now operating
    /// understaffed against in-flight attacks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender_over_budget_tick: Option<u32>,
    /// Runtime overrides to the scenario's authored
    /// [`Faction.diplomacy`] table. Keyed by `(source_faction,
    /// target_faction)`; `None` in inner result means the relationship
    /// has not been overridden and the scenario baseline applies.
    /// Populated by `EventEffect::DiplomacyChange` and by the
    /// alliance-fracture phase (Epic D round two). Empty when no
    /// scenario faction declares an `alliance_fracture` rule and no
    /// event ever fires `DiplomacyChange`, so legacy scenarios pay
    /// zero overhead.
    #[serde(default)]
    pub diplomacy_overrides: BTreeMap<FactionId, BTreeMap<FactionId, Diplomacy>>,
    /// Set of `(faction, rule_id)` pairs whose alliance-fracture rule
    /// has fired in the current run. Used to enforce one-shot
    /// semantics — a rule that fired once will not be re-evaluated.
    /// Empty when no faction declares `alliance_fracture`.
    #[serde(default)]
    pub fired_fractures: BTreeSet<(FactionId, String)>,
    /// Per-faction starting strength snapshot, captured once at
    /// initialization. Used by `FractureCondition::StrengthLossFraction`
    /// to compute the loss ratio without keeping a running history.
    /// Always populated at startup with one entry per faction (the
    /// snapshot is cheap and unconditional); the fracture phase only
    /// consults it when a rule references the strength condition.
    #[serde(default)]
    pub initial_faction_strengths: BTreeMap<FactionId, f64>,
    /// Log of every alliance-fracture firing in the current run, in
    /// emission order. Surfaced post-run on
    /// [`faultline_types::stats::RunResult::fracture_events`] and
    /// aggregated across runs by
    /// `MonteCarloSummary.alliance_dynamics`. Empty when no
    /// alliance-fracture rule fired.
    #[serde(default)]
    pub fracture_events: Vec<faultline_types::stats::FractureEvent>,
    /// Log of every civilian-segment activation in the current run,
    /// in emission order (R3-2 round-two — population-segment
    /// activation). Surfaced post-run on
    /// [`faultline_types::stats::RunResult::civilian_activations`] and
    /// aggregated across runs by
    /// `MonteCarloSummary.civilian_activation_summaries`. Empty when
    /// no `population_segments` are declared or none crossed their
    /// activation threshold during the run.
    #[serde(default)]
    pub civilian_activations: Vec<faultline_types::stats::CivilianActivationEvent>,
}

/// Per-run runtime state for one declared [`Network`](
/// faultline_types::network::Network).
///
/// Static topology (nodes / edges / metadata) lives on the scenario
/// and never changes mid-run. This struct holds *only* the mutations
/// produced by event effects: per-edge capacity factors, per-node
/// disruption flags, and per-faction infiltration sets. Per-tick
/// [`NetworkSample`]s are appended to `samples` after the network
/// phase so the resilience curve can be rendered without re-deriving
/// it from the event log.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkRuntimeState {
    /// Multiplicative factor on each edge's static capacity. Edges
    /// absent from the map run at factor 1.0 (pristine baseline). The
    /// factor is clamped to `[0.0, 4.0]` on every update — this keeps
    /// runaway authoring errors (a `factor = 1e9` chained event) from
    /// poisoning the residual-capacity series.
    pub edge_factors: BTreeMap<EdgeId, f64>,
    /// Set of currently-disrupted nodes. Disrupted nodes have all
    /// their incident edges treated as severed for resilience metrics
    /// even if the static edge capacity is non-zero.
    pub disrupted_nodes: BTreeSet<NodeId>,
    /// Per-faction set of infiltrated nodes — factions with attacker-
    /// style visibility into a network node.
    pub infiltrated: BTreeMap<FactionId, BTreeSet<NodeId>>,
    /// Per-tick resilience samples in capture order.
    pub samples: Vec<NetworkSample>,
}

impl NetworkRuntimeState {
    /// Effective per-edge capacity factor — `edge_factors.get` with
    /// the default of `1.0`. Provided as a method so the network
    /// phase and metric-rendering call sites share a single source of
    /// truth.
    pub fn edge_factor(&self, edge: &EdgeId) -> f64 {
        self.edge_factors.get(edge).copied().unwrap_or(1.0)
    }
}

/// Per-tick mutable state for one defender role's investigative queue.
///
/// All counters are cumulative across the run; the post-run report
/// derives utilization / max-depth / time-to-saturation from these.
/// `service_accumulator` lets sub-unit service rates work without
/// rounding (rate = 0.5 means one item every two ticks).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderQueueState {
    /// Current items waiting in the queue.
    pub depth: u32,
    /// Capacity threshold copied from the scenario for cheap saturation
    /// checks — avoids resolving the role definition on the hot path.
    pub capacity: u32,
    pub service_rate: f64,
    /// Carries fractional service across ticks so sub-1.0 rates
    /// produce the right long-run throughput.
    pub service_accumulator: f64,
    /// Lifetime totals for analyst reporting.
    pub total_enqueued: u64,
    pub total_serviced: u64,
    pub total_dropped: u64,
    pub max_depth: u32,
    /// Sum of `depth` across observed ticks — divide by
    /// `ticks_observed` for mean utilization.
    pub total_depth_sum: u64,
    pub ticks_observed: u32,
    /// Tick at which `depth >= capacity` first became true. `None`
    /// when never saturated; the run is right-censored for the
    /// time-to-saturation analytic.
    pub first_saturated_at: Option<u32>,
    /// Detection rolls suppressed by saturation: original probability
    /// would have fired but the saturated multiplier did not. Pure
    /// post-hoc count, computed from a single uniform draw per roll
    /// (see `campaign::roll_detection_with_capacity`) so determinism
    /// is preserved.
    pub shadow_detections: u32,
}

impl DefenderQueueState {
    pub fn new(capacity: u32, service_rate: f64) -> Self {
        Self {
            depth: 0,
            capacity,
            service_rate,
            service_accumulator: 0.0,
            total_enqueued: 0,
            total_serviced: 0,
            total_dropped: 0,
            max_depth: 0,
            total_depth_sum: 0,
            ticks_observed: 0,
            first_saturated_at: None,
            shadow_detections: 0,
        }
    }

    /// Whether the queue is at or above its declared capacity.
    pub fn is_saturated(&self) -> bool {
        self.depth >= self.capacity
    }

    /// Drain up to `service_rate` items, carrying fractional capacity
    /// across ticks via `service_accumulator`. Returns the number of
    /// items actually serviced this tick.
    pub fn service(&mut self) -> u32 {
        if self.service_rate <= 0.0 || self.depth == 0 {
            return 0;
        }
        self.service_accumulator += self.service_rate;
        // Floor + take. Using floor (not round) avoids occasional
        // double-count rounding when rate fractional part > 0.5 — the
        // accumulator carries the leftover into the next tick.
        let mut to_serve = self.service_accumulator.floor() as u64;
        if to_serve == 0 {
            return 0;
        }
        let depth_u64 = u64::from(self.depth);
        if to_serve > depth_u64 {
            to_serve = depth_u64;
        }
        // Subtract whole-units serviced from the accumulator;
        // never negative because we floored it.
        self.service_accumulator -= to_serve as f64;
        let to_serve_u32 =
            u32::try_from(to_serve).expect("to_serve clamped to depth which fits in u32");
        self.depth -= to_serve_u32;
        self.total_serviced += u64::from(to_serve_u32);
        to_serve_u32
    }
}

/// One row of the rolling metric history. Captured at the end of each
/// tick (after the political and information phases have updated state)
/// so that an `EscalationThreshold` evaluated when a campaign phase
/// resolves on the *next* tick reads stable values.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub tick: u32,
    pub tension: f64,
    pub information_dominance: f64,
    pub institutional_erosion: f64,
    pub coercion_pressure: f64,
    pub political_cost: f64,
}

impl SimulationState {
    /// Capture the current escalation-metric values at the end of a tick.
    ///
    /// `max_history` is the longest `sustained_ticks` window any
    /// scenario branch needs to look back over; the buffer is kept just
    /// large enough to satisfy that window. Passing `0` disables
    /// retention entirely (the snapshot is dropped immediately) which
    /// is the no-op default for scenarios with no
    /// `EscalationThreshold` branches.
    pub fn push_metric_snapshot(&mut self, max_history: usize) {
        if max_history == 0 {
            return;
        }
        self.metric_history.push(MetricSnapshot {
            tick: self.tick,
            tension: self.political_climate.tension,
            information_dominance: self.non_kinetic.information_dominance,
            institutional_erosion: self.non_kinetic.institutional_erosion,
            coercion_pressure: self.non_kinetic.coercion_pressure,
            political_cost: self.non_kinetic.political_cost,
        });
        if self.metric_history.len() > max_history {
            let drop = self.metric_history.len() - max_history;
            self.metric_history.drain(0..drop);
        }
    }
}

/// Non-kinetic outcome metrics.
///
/// Each field lives in `[0, 1]` and represents cumulative pressure or
/// damage along a dimension that is not directly measured by combat
/// or territorial control.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NonKineticMetrics {
    pub information_dominance: f64,
    pub institutional_erosion: f64,
    pub coercion_pressure: f64,
    pub political_cost: f64,
}

/// Runtime state tracked per faction during the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeFactionState {
    pub faction_id: FactionId,
    /// Aggregate military strength across all forces.
    pub total_strength: f64,
    /// Current morale in `[0.0, 1.0]`.
    pub morale: f64,
    /// Accumulated resources.
    pub resources: f64,
    /// Resource income per tick.
    pub resource_rate: f64,
    /// Logistics capacity.
    pub logistics_capacity: f64,
    /// Regions currently controlled.
    pub controlled_regions: Vec<RegionId>,
    /// Force units owned by this faction.
    pub forces: BTreeMap<ForceId, ForceUnit>,
    /// Tech cards currently deployed.
    pub tech_deployed: Vec<TechCardId>,
    /// Tracks how many consecutive ticks this faction has held
    /// specific regions (for HoldRegions victory conditions).
    pub region_hold_ticks: BTreeMap<RegionId, u32>,
    /// Whether this faction has been eliminated (strength = 0
    /// and no recruitment possible).
    pub eliminated: bool,
    /// Index into the faction's `LeadershipCadre.ranks`. `0` means the
    /// top-of-chain leader is in command. Each
    /// `PhaseOutput::LeadershipDecapitation` strike advances this by
    /// one. When it passes the end of the cadre the faction is
    /// leaderless — effectiveness collapses to 0 and morale is capped
    /// there until the run ends. Zero-cost for legacy factions
    /// (the engine path skips the cadre lookup when no cadre is
    /// declared).
    #[serde(default)]
    pub current_leadership_rank: u32,
    /// Tick of the most recent decapitation, or `None` if the faction
    /// has never been struck. Drives the recovery-ramp interpolation
    /// in `effective_leadership_factor`.
    #[serde(default)]
    pub last_decapitation_tick: Option<u32>,
    /// Cumulative count of leadership decapitations against this
    /// faction over the run. Surfaced by the report.
    #[serde(default)]
    pub leadership_decapitations: u32,
    /// Most recently observed supply pressure in `[0, 1]` (Epic D
    /// round three, item 2). Updated at the top of
    /// [`crate::tick::attrition_phase`] for any faction that owns at
    /// least one `kind = "supply"` network. Defaults to `1.0` for
    /// legacy factions and the first tick before attrition has run,
    /// so combat / morale / report code can read it unconditionally.
    #[serde(default = "one_f64_default")]
    pub current_supply_pressure: f64,
    /// Cumulative sum of per-tick supply pressure samples. Divide by
    /// `supply_pressure_samples` to recover the run mean. `0.0` until
    /// the first attrition phase observes the faction.
    #[serde(default)]
    pub supply_pressure_sum: f64,
    /// Number of attrition ticks where supply pressure was sampled.
    /// Used as the denominator for the run-mean computation. Zero for
    /// legacy factions with no owned supply network — the report
    /// elides their row entirely in that case.
    #[serde(default)]
    pub supply_pressure_samples: u32,
    /// Minimum per-tick supply pressure observed over the run. `1.0`
    /// when never sampled (paired with `samples == 0` to mean "no
    /// supply network"); strictly less than `1.0` once any
    /// interdiction has bitten.
    #[serde(default = "one_f64_default")]
    pub supply_pressure_min: f64,
    /// Number of attrition ticks where pressure was strictly below
    /// [`crate::supply::PRESSURE_REPORTING_THRESHOLD`]. Surfaces as
    /// "ticks under meaningful pressure" in the report; complements
    /// the mean / min by capturing duration of stress rather than
    /// just severity.
    #[serde(default)]
    pub supply_pressure_pressured_ticks: u32,
}

/// Default-value helper for `#[serde(default = "...")]` on
/// `current_supply_pressure` / `supply_pressure_min`. Keeps the
/// legacy / no-supply-network baseline at `1.0` so consumers can read
/// the field unconditionally.
fn one_f64_default() -> f64 {
    1.0
}

impl RuntimeFactionState {
    /// Recompute `total_strength` from the force roster.
    pub fn recompute_strength(&mut self) {
        self.total_strength = self.forces.values().map(|f| f.strength).sum();
    }

    /// Return the list of regions where this faction has forces.
    pub fn force_regions(&self) -> BTreeSet<RegionId> {
        self.forces.values().map(|f| f.region.clone()).collect()
    }

    /// Check if any force is a guerrilla-style unit.
    pub fn has_guerrilla_units(&self) -> bool {
        self.forces
            .values()
            .any(|f| matches!(f.unit_type, UnitType::Militia | UnitType::SpecialOperations))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use faultline_types::faction::ForceUnit;

    fn make_test_faction_state() -> RuntimeFactionState {
        RuntimeFactionState {
            faction_id: FactionId::from("test"),
            total_strength: 0.0,
            morale: 0.8,
            resources: 100.0,
            resource_rate: 10.0,
            logistics_capacity: 50.0,
            controlled_regions: vec![],
            forces: BTreeMap::new(),
            tech_deployed: vec![],
            region_hold_ticks: BTreeMap::new(),
            eliminated: false,
            current_leadership_rank: 0,
            last_decapitation_tick: None,
            leadership_decapitations: 0,
            current_supply_pressure: 1.0,
            supply_pressure_sum: 0.0,
            supply_pressure_samples: 0,
            supply_pressure_min: 1.0,
            supply_pressure_pressured_ticks: 0,
        }
    }

    fn make_force(
        id: &str,
        region: &str,
        strength: f64,
        unit_type: UnitType,
    ) -> (ForceId, ForceUnit) {
        let fid = ForceId::from(id);
        let unit = ForceUnit {
            id: fid.clone(),
            name: id.into(),
            unit_type,
            region: RegionId::from(region),
            strength,
            mobility: 1.0,
            force_projection: None,
            upkeep: 1.0,
            morale_modifier: 0.0,
            capabilities: vec![],
            move_progress: 0.0,
        };
        (fid, unit)
    }

    #[test]
    fn recompute_strength_sums_forces() {
        let mut fs = make_test_faction_state();

        let (id1, u1) = make_force("f1", "r1", 50.0, UnitType::Infantry);
        let (id2, u2) = make_force("f2", "r2", 30.0, UnitType::Infantry);
        let (id3, u3) = make_force("f3", "r3", 20.0, UnitType::Armor);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);
        fs.forces.insert(id3, u3);

        fs.recompute_strength();

        assert!(
            (fs.total_strength - 100.0).abs() < f64::EPSILON,
            "total_strength should be 100.0, got {}",
            fs.total_strength,
        );
    }

    #[test]
    fn force_regions_lists_all() {
        let mut fs = make_test_faction_state();

        let (id1, u1) = make_force("f1", "r1", 50.0, UnitType::Infantry);
        let (id2, u2) = make_force("f2", "r2", 30.0, UnitType::Infantry);
        let (id3, u3) = make_force("f3", "r3", 20.0, UnitType::Infantry);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);
        fs.forces.insert(id3, u3);

        let regions = fs.force_regions();
        assert_eq!(regions.len(), 3, "should have 3 regions");
        assert!(regions.contains(&RegionId::from("r1")));
        assert!(regions.contains(&RegionId::from("r2")));
        assert!(regions.contains(&RegionId::from("r3")));
    }

    #[test]
    fn has_guerrilla_units_true() {
        let mut fs = make_test_faction_state();
        let (id, unit) = make_force("militia1", "r1", 50.0, UnitType::Militia);
        fs.forces.insert(id, unit);

        assert!(
            fs.has_guerrilla_units(),
            "should detect militia as guerrilla"
        );
    }

    #[test]
    fn has_guerrilla_units_false() {
        let mut fs = make_test_faction_state();
        let (id1, u1) = make_force("armor1", "r1", 50.0, UnitType::Armor);
        let (id2, u2) = make_force("inf1", "r2", 30.0, UnitType::Infantry);
        fs.forces.insert(id1, u1);
        fs.forces.insert(id2, u2);

        assert!(
            !fs.has_guerrilla_units(),
            "armor and infantry should not be guerrilla"
        );
    }
}
