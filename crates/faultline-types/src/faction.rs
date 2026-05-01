use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{
    DefenderRoleId, EventId, FactionId, ForceId, InstitutionId, RegionId, TechCardId,
};
use crate::strategy::Doctrine;

/// A participant in the simulation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Faction {
    pub id: FactionId,
    pub name: String,
    pub faction_type: FactionType,
    pub description: String,
    pub color: String,
    pub forces: BTreeMap<ForceId, ForceUnit>,
    pub tech_access: Vec<TechCardId>,
    pub initial_morale: f64,
    pub logistics_capacity: f64,
    pub initial_resources: f64,
    pub resource_rate: f64,
    pub recruitment: Option<RecruitmentConfig>,
    pub command_resilience: f64,
    pub intelligence: f64,
    pub diplomacy: Vec<DiplomaticStance>,
    #[serde(default)]
    pub doctrine: Doctrine,
    /// Declarative doctrine / rules-of-engagement contract describing
    /// how this faction is *permitted* to escalate. Reports surface the
    /// ladder in Policy Implications; the engine itself does not
    /// currently enforce these — they document the decision-maker's
    /// standing orders so analysts can see when a counterfactual
    /// assumes the faction would violate its own doctrine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_rules: Option<EscalationRules>,
    /// Defender-side capacity model: per-role investigative queues
    /// constraining how fast this faction can process incoming alerts /
    /// tips / forensic work. Empty = legacy infinite-capacity assumption
    /// (every detection roll is independent and unaffected by other
    /// in-flight work). When kill-chain phases reference roles via
    /// `gated_by_defender` or `defender_noise`, the engine maintains
    /// per-role queues with deterministic FIFO service and applies the
    /// `saturated_detection_factor` penalty when a queue is at depth.
    /// Enables alert-fatigue, FOIA-flood, and forensic-backlog
    /// scenarios; see `docs/scenario_schema.md` for the full model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defender_capacities: BTreeMap<DefenderRoleId, DefenderCapacity>,
    /// Optional leadership cadre — named ranks (top of chain first)
    /// plus succession parameters. Drives the
    /// `PhaseOutput::LeadershipDecapitation` mechanic: a successful
    /// decapitation phase advances the rank index, applies a morale
    /// shock, and caps the faction's morale at the new rank's
    /// effectiveness during the recovery ramp. `None` = legacy
    /// behavior (faction has no decapitation surface to expose).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leadership: Option<LeadershipCadre>,
    /// Declarative alliance-fracture rules (Epic D round two). Names
    /// conditions under which this faction's diplomatic stance toward
    /// a counterparty flips — typically `Cooperative` / `Allied` ->
    /// `Hostile` when the counterparty is publicly attributed for an
    /// attack, takes unsustainable casualties, etc. Each rule fires
    /// at most once per run (latched via `fired_fractures` on the
    /// runtime state). Empty / absent = legacy behavior (alliances
    /// never break mid-run). Engine validation rejects unknown
    /// counterparty / attacker / event ids and out-of-range
    /// thresholds at scenario load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alliance_fracture: Option<AllianceFracture>,
}

/// A faction's leadership cadre — ranks plus succession dynamics.
///
/// Models discontinuous capability drops when a top leader is killed
/// or removed. The current top rank is index 0 at simulation start; a
/// `LeadershipDecapitation` phase advances the index by one and
/// triggers a recovery ramp during which the new rank's nominal
/// effectiveness is multiplied by an interpolated factor rising from
/// `succession_floor` to 1.0 over `succession_recovery_ticks`.
///
/// When the rank index passes the end of the cadre the faction is
/// "leaderless": effectiveness collapses to 0.0 and no further
/// decapitation can degrade it. Reports surface this as a terminal
/// state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeadershipCadre {
    /// Ranks ordered top-of-chain first. Must contain at least one
    /// entry (validated at scenario load).
    pub ranks: Vec<LeadershipRank>,
    /// Number of ticks the recovery ramp lasts after a decapitation.
    /// `0` means a successor reaches full effectiveness immediately
    /// (no transition penalty); `succession_floor` is then ignored.
    pub succession_recovery_ticks: u32,
    /// Multiplier applied on the first tick after a decapitation.
    /// Linearly interpolates to 1.0 over `succession_recovery_ticks`.
    /// Defaults to 0.5 (a successor is half-effective day one) which
    /// matches the public published case-study spread on contested
    /// successions.
    #[serde(default = "default_succession_floor")]
    pub succession_floor: f64,
}

/// One rank in the leadership cadre.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeadershipRank {
    /// Stable identifier (e.g. "principal", "deputy", "field_lt").
    pub id: String,
    pub name: String,
    /// Multiplicative scalar describing this rank's relative command
    /// effectiveness. Top of chain is conventionally `1.0`; later
    /// successors typically have lower values reflecting reduced
    /// authority and experience. The engine reads this to cap the
    /// faction's runtime morale during the recovery period.
    pub effectiveness: f64,
    #[serde(default)]
    pub description: String,
}

fn default_succession_floor() -> f64 {
    0.5
}

/// One defender role with bounded investigative throughput.
///
/// The model is a single-server queue with discrete capacity and a
/// fractional-rate accumulator: `service_rate = 0.5` services one item
/// every two ticks. Items past `queue_depth` are handled per
/// [`OverflowPolicy`]. When the queue is at full saturation, any
/// kill-chain phase whose `gated_by_defender` names this role suffers a
/// detection-probability multiplier of `saturated_detection_factor` —
/// modelling alert fatigue, where a swamped SOC misses real signal even
/// if it would have caught it idle.
///
/// All fields are serde-default-aware so a partial scenario edit (e.g.
/// adding only `queue_depth` and `service_rate`) loads cleanly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefenderCapacity {
    pub id: DefenderRoleId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Maximum queue depth before [`OverflowPolicy`] applies.
    pub queue_depth: u32,
    /// Mean items serviced per tick. Fractional rates accumulate
    /// (`0.5` = one item every two ticks). Must be `>= 0`.
    pub service_rate: f64,
    /// Behavior when an enqueue would exceed `queue_depth`.
    #[serde(default)]
    pub overflow: OverflowPolicy,
    /// Detection-probability multiplier applied to phases gated by this
    /// role when the queue is at full capacity. `1.0` = no penalty
    /// (legacy behavior). Realistic alert-fatigue values are `0.2`–
    /// `0.5`; the published SOC-effectiveness literature consistently
    /// reports a 50–80% drop in true-positive rates under sustained
    /// queue saturation.
    #[serde(default = "default_saturated_factor")]
    pub saturated_detection_factor: f64,
}

/// What the queue does when an enqueue would overflow `queue_depth`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverflowPolicy {
    /// Refuse the new item — most realistic for SOC alert pipes where
    /// a full ticket queue silently drops the next page. Default.
    #[default]
    DropNew,
    /// Evict the oldest queued item to make room. Models cache-style
    /// alert systems and cookie-jar-bounded forensic pipelines.
    DropOldest,
    /// No drops — the queue grows unbounded past `queue_depth`. Use
    /// only when the work is genuinely unbounded (FOIA backlog, court
    /// calendar) and you want to track how far it gets behind.
    Backlog,
}

fn default_saturated_factor() -> f64 {
    1.0
}

/// A scenario-author-asserted escalation ladder for a faction.
///
/// Purely declarative in this iteration — the engine does not consult
/// it when selecting actions. Surfaced in reports so analysts can see
/// which counterfactuals implicitly require the faction to cross a
/// doctrinal threshold. A later engine iteration may enforce the
/// ladder, at which point this type becomes load-bearing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRules {
    /// One-line summary of the faction's doctrine / ROE stance.
    #[serde(default)]
    pub posture: String,
    /// Ordered rungs the faction is permitted to climb. Earlier rungs
    /// are lower escalation; later rungs are higher. `None` on
    /// `trigger_tension` = rung is a permanent standing posture.
    #[serde(default)]
    pub ladder: Vec<EscalationRung>,
    /// Tension level above which the faction will *not* voluntarily
    /// de-escalate without an external event. Declarative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub de_escalation_floor: Option<f64>,
}

/// A single rung on an escalation ladder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationRung {
    /// Stable identifier (e.g. "grey_zone", "kinetic", "strategic").
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Political tension at or above which the faction is *authorized*
    /// to operate at this rung. `None` = always authorized (e.g. a
    /// peacetime-permitted information-ops posture).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_tension: Option<f64>,
    /// Actions the faction is permitted to take at this rung. Free
    /// text — authors describe capabilities (e.g. "kinetic strikes
    /// against military targets outside own territory").
    #[serde(default)]
    pub permitted_actions: Vec<String>,
    /// Actions explicitly prohibited at this rung. Useful for
    /// documenting red lines ("no strikes against nuclear
    /// infrastructure").
    #[serde(default)]
    pub prohibited_actions: Vec<String>,
}

/// What kind of faction this is.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FactionType {
    Government {
        institutions: BTreeMap<InstitutionId, Institution>,
    },
    Military {
        branch: MilitaryBranch,
    },
    Insurgent,
    #[default]
    Civilian,
    PrivateMilitary,
    Foreign {
        is_proxy: bool,
    },
}

/// A government institution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Institution {
    pub id: InstitutionId,
    pub name: String,
    pub institution_type: InstitutionType,
    pub loyalty: f64,
    pub effectiveness: f64,
    pub personnel: u64,
    pub fracture_threshold: Option<f64>,
}

/// Categories of government institutions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstitutionType {
    LawEnforcement,
    Intelligence,
    Judiciary,
    Legislature,
    Executive,
    NationalGuard,
    FederalAgency,
    FinancialRegulator,
    MediaRegulator,
    Custom(String),
}

/// Branches of military service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MilitaryBranch {
    Army,
    Navy,
    AirForce,
    Marines,
    SpaceForce,
    CoastGuard,
    Combined,
    Custom(String),
}

/// A deployable military or paramilitary unit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForceUnit {
    pub id: ForceId,
    pub name: String,
    pub unit_type: UnitType,
    pub region: RegionId,
    pub strength: f64,
    pub mobility: f64,
    pub force_projection: Option<ForceProjection>,
    pub upkeep: f64,
    pub morale_modifier: f64,
    pub capabilities: Vec<UnitCapability>,
}

/// Categories of military/paramilitary units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitType {
    Infantry,
    Mechanized,
    Armor,
    Artillery,
    AirSupport,
    Naval,
    SpecialOperations,
    CyberUnit,
    DroneSwarm,
    LawEnforcement,
    Militia,
    Logistics,
    AirDefense,
    ElectronicWarfare,
    Custom(String),
}

/// How a unit can project force beyond its region.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum ForceProjection {
    Airlift { capacity: f64 },
    Naval { range: f64 },
    StandoffStrike { range: f64, damage: f64 },
}

/// Special capabilities a unit may possess.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnitCapability {
    Garrison,
    Raid,
    Sabotage {
        effectiveness: f64,
    },
    Recon {
        range: f64,
        detection: f64,
    },
    Interdiction {
        range: f64,
    },
    AreaDenial {
        radius: f64,
    },
    CounterUAS {
        effectiveness: f64,
    },
    EW {
        jamming_range: f64,
        effectiveness: f64,
    },
    Cyber {
        attack: f64,
        defense: f64,
    },
    InfoOps {
        reach: f64,
        persuasion: f64,
    },
    Humanitarian {
        capacity: f64,
    },
}

/// Configuration for recruiting new units over time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecruitmentConfig {
    pub rate: f64,
    pub population_threshold: f64,
    pub unit_type: UnitType,
    pub base_strength: f64,
    pub cost: f64,
}

/// A faction's diplomatic posture toward another faction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiplomaticStance {
    pub target_faction: FactionId,
    pub stance: Diplomacy,
}

/// Levels of diplomatic relations.
///
/// `Default` = `Neutral` so `..Default::default()` spread in test
/// fixtures lands on the most innocuous stance; switching this default
/// would silently flip baseline scenario behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Diplomacy {
    War,
    Hostile,
    #[default]
    Neutral,
    Cooperative,
    Allied,
}

/// Declarative alliance-fracture configuration (Epic D round two).
///
/// Authors describe coalitions as static at scenario start (via
/// `Faction.diplomacy`) and then list the conditions under which each
/// alliance can break. The engine evaluates rules at end-of-tick after
/// the campaign phase, so a fracture triggered by attribution from a
/// chain that succeeded *this* tick is observable on the next tick's
/// downstream effects (e.g. an Allied faction switching to Hostile
/// against the attacker).
///
/// Each rule fires at most once per run; the runtime tracks fired rule
/// ids on `SimulationState.fired_fractures`. Per-run fracture events
/// are recorded on `RunResult.fracture_events` and aggregated across
/// runs by `MonteCarloSummary.alliance_dynamics`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AllianceFracture {
    /// One or more fracture rules. Empty `rules` is rejected at
    /// scenario load (an opted-in empty `alliance_fracture` is almost
    /// certainly an unfilled author template).
    pub rules: Vec<FractureRule>,
}

/// One alliance-fracture rule.
///
/// `id` is a stable identifier used by the report and the runtime
/// `fired_fractures` set; it must be unique within a faction's
/// `alliance_fracture.rules`. `counterparty` is the faction whose
/// stance changes — the relationship being fractured runs from this
/// faction *to* `counterparty`. `condition` evaluates against
/// `SimulationState` at end of tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FractureRule {
    pub id: String,
    pub counterparty: FactionId,
    /// Stance to flip to when the rule fires. Defaults to `Hostile`
    /// — the most common analyst use case is "Cooperative -> Hostile
    /// when attribution lands."
    #[serde(default = "default_fracture_stance")]
    pub new_stance: Diplomacy,
    pub condition: FractureCondition,
    /// Optional human-readable label surfaced by the report. Free
    /// text; not consumed by the engine.
    #[serde(default)]
    pub description: String,
}

fn default_fracture_stance() -> Diplomacy {
    Diplomacy::Hostile
}

/// Conditions under which an alliance fractures.
///
/// All thresholds are evaluated at end-of-tick after the campaign
/// phase. Pure functions of `SimulationState` plus the in-flight
/// campaign reports — no RNG, so determinism follows from the
/// existing engine contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FractureCondition {
    /// Mean attribution confidence across all in-flight kill chains
    /// owned by `attacker` is at or above `threshold`. Models a
    /// covert operation losing its protective ambiguity — once the
    /// attribution lands publicly, an Allied counterparty can no
    /// longer politically sustain cooperation with the attacker.
    AttributionThreshold {
        attacker: FactionId,
        /// Threshold in `[0, 1]`. Fires when measured >= threshold.
        threshold: f64,
    },
    /// This faction's runtime morale is at or below `floor`. Models
    /// a coalition partner losing the political will to remain
    /// engaged.
    MoraleFloor { floor: f64 },
    /// Political tension is at or above `threshold`. Models
    /// environmental pressure breaking a fragile coalition without
    /// any single attribution event.
    TensionThreshold { threshold: f64 },
    /// A specific event has fired in the run. Most permissive form
    /// — gives authors full control over fracture triggers via the
    /// existing event scaffolding (e.g. an event with custom prose
    /// that fires on a tech-driven trigger then fractures an
    /// alliance).
    EventFired { event: EventId },
    /// This faction's strength has dropped by at least
    /// `delta_fraction` of its starting value. Models a coalition
    /// partner taking unsustainable casualties.
    StrengthLossFraction {
        /// Threshold in `[0, 1]`. Fires when
        /// `(initial - current) / initial >= delta_fraction`. A
        /// faction that started at zero strength never fires this
        /// condition (the divisor would be zero).
        delta_fraction: f64,
    },
}
