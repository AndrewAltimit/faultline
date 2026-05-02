//! Runtime kill chain / campaign state and per-tick phase progression.
//!
//! The configuration types live in `faultline_types::campaign`. This
//! module holds the mutable runtime state (which phases are active /
//! complete / detected) and the per-tick advancement logic.

use std::collections::BTreeMap;

use rand::Rng;
use serde::{Deserialize, Serialize};

use faultline_types::campaign::{
    BranchCondition, CampaignPhase, DefenderRoleRef, EscalationMetric, KillChain, PhaseOutput,
    ThresholdDirection,
};
use faultline_types::faction::OverflowPolicy;
use faultline_types::ids::{DefenderRoleId, FactionId, KillChainId, PhaseId, RegionId};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{CampaignReport, PhaseOutcome};

use crate::state::{MetricSnapshot, SimulationState};

/// Multiplier applied to per-tick effective detection probability once
/// the scenario's `defender_budget` has been exhausted. Mirrors the
/// shape of `DefenderCapacity::saturated_detection_factor` (the queue-
/// saturation analogue): a value in `[0.0, 1.0]` that scales how much
/// signal an overstretched defender catches. The 0.5× constant chosen
/// here represents a plausible 50% drop in true-positive throughput
/// when the defender's wallet is empty and gap-closing programs go
/// unfunded — concrete enough to make the parameter meaningful, not so
/// aggressive that it dominates other defender-side variables.
const DEFENDER_OVER_BUDGET_DETECTION_FACTOR: f64 = 0.5;

// ---------------------------------------------------------------------------
// Phase status
// ---------------------------------------------------------------------------

/// Runtime status of a single phase within a kill chain.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state")]
pub enum PhaseStatus {
    /// Not yet eligible to start (prerequisites unmet or not reached).
    Pending,
    /// Currently executing. `started_at` and `duration` are in ticks.
    Active { started_at: u32, duration: u32 },
    /// Completed successfully at the given tick.
    Succeeded { tick: u32 },
    /// Completed and failed the success roll at the given tick.
    Failed { tick: u32 },
    /// Detected by the defender while active. Detection is terminal
    /// for the phase — subsequent branches should use `OnDetection`.
    Detected { tick: u32 },
}

impl PhaseStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            PhaseStatus::Succeeded { .. }
                | PhaseStatus::Failed { .. }
                | PhaseStatus::Detected { .. }
        )
    }

    pub fn succeeded(&self) -> bool {
        matches!(self, PhaseStatus::Succeeded { .. })
    }

    pub fn detected(&self) -> bool {
        matches!(self, PhaseStatus::Detected { .. })
    }
}

// ---------------------------------------------------------------------------
// Campaign runtime state
// ---------------------------------------------------------------------------

/// Runtime state for one in-flight kill chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignState {
    pub chain_id: KillChainId,
    pub phase_status: BTreeMap<PhaseId, PhaseStatus>,
    /// Accumulated detection probability per phase (`1 - product(1 - p_i)`).
    pub detection_accumulation: BTreeMap<PhaseId, f64>,
    /// Whether the defender has been alerted on any phase in this chain.
    pub defender_alerted: bool,
    /// Dollar outlays.
    pub attacker_spend: f64,
    pub defender_spend: f64,
    /// Accumulated attribution confidence `[0, 1]` that the defender
    /// has developed about the attacker.
    pub attribution_confidence: f64,
    /// Non-kinetic metric accumulators.
    pub information_dominance: f64,
    pub institutional_erosion: f64,
    pub coercion_pressure: f64,
    pub political_cost: f64,
}

impl CampaignState {
    pub fn new(chain: &KillChain) -> Self {
        let phase_status = chain
            .phases
            .keys()
            .map(|pid| (pid.clone(), PhaseStatus::Pending))
            .collect();
        let detection_accumulation = chain.phases.keys().map(|pid| (pid.clone(), 0.0)).collect();
        Self {
            chain_id: chain.id.clone(),
            phase_status,
            detection_accumulation,
            defender_alerted: false,
            attacker_spend: 0.0,
            defender_spend: 0.0,
            attribution_confidence: 0.0,
            information_dominance: 0.0,
            institutional_erosion: 0.0,
            coercion_pressure: 0.0,
            political_cost: 0.0,
        }
    }

    /// Count of phases that have reached `Succeeded`.
    pub fn succeeded_phases(&self) -> usize {
        self.phase_status.values().filter(|s| s.succeeded()).count()
    }

    /// Produce a terminal report snapshot for inclusion in `RunResult`.
    pub fn to_report(&self) -> CampaignReport {
        let phase_outcomes = self
            .phase_status
            .iter()
            .map(|(pid, status)| {
                let outcome = match status {
                    PhaseStatus::Pending => PhaseOutcome::Pending,
                    PhaseStatus::Active { .. } => PhaseOutcome::Active,
                    PhaseStatus::Succeeded { tick } => PhaseOutcome::Succeeded { tick: *tick },
                    PhaseStatus::Failed { tick } => PhaseOutcome::Failed { tick: *tick },
                    PhaseStatus::Detected { tick } => PhaseOutcome::Detected { tick: *tick },
                };
                (pid.clone(), outcome)
            })
            .collect();
        CampaignReport {
            chain_id: self.chain_id.clone(),
            phase_outcomes,
            detection_accumulation: self.detection_accumulation.clone(),
            defender_alerted: self.defender_alerted,
            attacker_spend: self.attacker_spend,
            defender_spend: self.defender_spend,
            attribution_confidence: self.attribution_confidence,
            information_dominance: self.information_dominance,
            institutional_erosion: self.institutional_erosion,
            coercion_pressure: self.coercion_pressure,
            political_cost: self.political_cost,
        }
    }
}

/// Convert all in-flight campaigns to terminal reports.
pub fn reports(
    campaigns: &BTreeMap<KillChainId, CampaignState>,
) -> BTreeMap<KillChainId, CampaignReport> {
    campaigns
        .iter()
        .map(|(id, s)| (id.clone(), s.to_report()))
        .collect()
}

// ---------------------------------------------------------------------------
// Initialization / tick
// ---------------------------------------------------------------------------

/// Build initial campaign states from a scenario. All phases start
/// `Pending`; entry phases are activated on the first tick of campaign
/// phase progression, not here.
pub fn initialize_campaigns(scenario: &Scenario) -> BTreeMap<KillChainId, CampaignState> {
    scenario
        .kill_chains
        .iter()
        .map(|(id, chain)| (id.clone(), CampaignState::new(chain)))
        .collect()
}

/// Advance all in-flight kill chains by one tick.
///
/// Per-tick order is **arrive → assess → service**, which mirrors
/// how a real SOC shift actually unfolds: alerts pile up first, the
/// analyst sees the current backlog, then they spend the shift
/// working through it.
///
/// 1. For each chain, activate eligible pending phases.
/// 2. For each active phase, in this order: enqueue the phase's
///    `defender_noise` so the saturation check below reads post-
///    arrival depth; then roll detection against that depth (when a
///    phase declares `gated_by_defender` and the queue is at capacity,
///    detection probability is multiplied by the role's
///    `saturated_detection_factor`, with a single uniform draw
///    covering both the actual roll and the "shadow detection"
///    bookkeeping — a draw below the unattenuated `dp` but above the
///    saturated `dp` counts as a shadow detection); then run
///    completion / branching as usual.
/// 3. **Service** every defender queue once at its declared per-tick
///    rate. Done at end-of-tick so a noise-flooded queue stays at
///    saturation through *this tick's* detection rolls — that
///    persistence is what reproduces the alert-fatigue effect when
///    a sequential phase 2 inherits the backlog phase 1 created.
/// 4. **Sample** per-queue stats (max-depth, first-saturation tick,
///    rolling depth-sum for mean utilization). Sampled after service
///    so the depth-sum reflects the post-service residual the
///    defender carries into the next shift.
pub fn campaign_phase(
    state: &mut SimulationState,
    scenario: &Scenario,
    campaigns: &mut BTreeMap<KillChainId, CampaignState>,
    rng: &mut impl Rng,
) {
    // Defender-budget gate. Sum cumulative defender spend across every
    // chain's CampaignState; when the scenario set a budget and the
    // total has grown past it, latch the first-overrun tick on
    // `SimulationState` (sticky for the rest of the run) and apply a
    // 0.5× detection-probability multiplier to all subsequent
    // detection rolls. Snapshot the status at tick-start rather than
    // re-checking after each phase so chain-processing order can never
    // affect which phase first incurs the penalty within a tick.
    if state.defender_over_budget_tick.is_none()
        && let Some(cap) = scenario.defender_budget
    {
        let total_spend: f64 = campaigns.values().map(|c| c.defender_spend).sum();
        if total_spend > cap {
            state.defender_over_budget_tick = Some(state.tick);
        }
    }
    let defender_over_budget_factor = if state.defender_over_budget_tick.is_some() {
        DEFENDER_OVER_BUDGET_DETECTION_FACTOR
    } else {
        1.0
    };

    for (chain_id, chain) in &scenario.kill_chains {
        let Some(campaign) = campaigns.get_mut(chain_id) else {
            continue;
        };

        // Step 1: activate eligible pending phases.
        activate_ready_phases(
            campaign,
            chain,
            state.tick,
            rng,
            scenario.attacker_budget,
            &state.metric_history,
        );

        // Step 2: process active phases — detection + completion.
        let active_phase_ids: Vec<PhaseId> = campaign
            .phase_status
            .iter()
            .filter(|(_, st)| matches!(st, PhaseStatus::Active { .. }))
            .map(|(pid, _)| pid.clone())
            .collect();

        for pid in active_phase_ids {
            let phase = match chain.phases.get(&pid) {
                Some(p) => p,
                None => continue,
            };

            // Enqueue this phase's noise *before* the detection roll
            // so the saturation check reads the post-arrival depth.
            // See `campaign_phase`'s docstring for why this ordering
            // matters for the alert-fatigue archetype.
            enqueue_phase_noise(state, scenario, phase, rng);

            // Detection roll. The unattenuated `dp` is used for
            // accumulating exposure (the attribution / detection-rate
            // analytics still report the operation's intrinsic
            // visibility, independent of load), while the load-
            // adjusted draw decides whether the defender actually
            // catches it this tick.
            //
            // Environment factor (weather / time-of-day) is
            // applied multiplicatively *into* `dp` itself, before the
            // saturation gate, so a Night window simultaneously
            // shrinks the unattenuated and the saturated rolls (the
            // attacker is harder to see at night regardless of
            // defender load) and naturally narrows the
            // shadow-detection window between them.
            let env_factor = crate::tick::environment_detection_factor(scenario, state.tick);
            let dp = (phase.detection_probability_per_tick * env_factor).clamp(0.0, 1.0);
            if dp > 0.0 {
                let prev = campaign
                    .detection_accumulation
                    .get(&pid)
                    .copied()
                    .unwrap_or(0.0);
                let new_accum = 1.0 - (1.0 - prev) * (1.0 - dp);
                campaign
                    .detection_accumulation
                    .insert(pid.clone(), new_accum);

                let saturated_factor =
                    saturated_factor_for(state, scenario, phase.gated_by_defender.as_ref());
                let effective_dp =
                    (dp * saturated_factor * defender_over_budget_factor).clamp(0.0, 1.0);
                let draw: f64 = rng.r#gen();
                let detected = draw < effective_dp;
                let shadow = !detected && draw < dp;

                if let Some(role_ref) = phase.gated_by_defender.as_ref()
                    && shadow
                    && let Some(q) = queue_mut(state, &role_ref.faction, &role_ref.role)
                {
                    q.shadow_detections += 1;
                }

                if detected {
                    campaign
                        .phase_status
                        .insert(pid.clone(), PhaseStatus::Detected { tick: state.tick });
                    campaign.defender_alerted = true;
                    campaign.attribution_confidence =
                        (1.0 - phase.attribution_difficulty).clamp(0.0, 1.0);
                    apply_detection_penalty(state, phase);
                    resolve_branches(
                        campaign,
                        chain,
                        &pid,
                        state.tick,
                        rng,
                        scenario.attacker_budget,
                        &state.metric_history,
                    );
                    continue;
                }
            }

            // Completion check.
            let (started_at, duration) = if let PhaseStatus::Active {
                started_at,
                duration,
            } = campaign.phase_status[&pid]
            {
                (started_at, duration)
            } else {
                continue;
            };
            if state.tick.saturating_sub(started_at) >= duration {
                // Roll for success.
                let succeeded_prereqs = phase
                    .prerequisites
                    .iter()
                    .filter(|p| campaign.phase_status.get(*p).is_some_and(|s| s.succeeded()))
                    .count() as f64;
                let boost = phase.prerequisite_success_boost * succeeded_prereqs;
                let p_success = (phase.base_success_probability + boost).clamp(0.0, 1.0);

                if rng.r#gen::<f64>() < p_success {
                    campaign
                        .phase_status
                        .insert(pid.clone(), PhaseStatus::Succeeded { tick: state.tick });
                    // Apply outputs.
                    for output in &phase.outputs {
                        apply_phase_output(state, scenario, campaign, output);
                    }
                    // Cost accounting. Defender spend accrues only on
                    // success — see `PhaseCost::defender_dollars` — so a
                    // defender that repels every attempt accrues zero
                    // spend and never trips `defender_budget`. Attacker
                    // spend accrues either way (mirrored in the failure
                    // branch below).
                    campaign.attacker_spend += phase.cost.attacker_dollars;
                    campaign.defender_spend += phase.cost.defender_dollars;
                } else {
                    campaign
                        .phase_status
                        .insert(pid.clone(), PhaseStatus::Failed { tick: state.tick });
                    campaign.attacker_spend += phase.cost.attacker_dollars;
                }
                resolve_branches(
                    campaign,
                    chain,
                    &pid,
                    state.tick,
                    rng,
                    scenario.attacker_budget,
                    &state.metric_history,
                );
            }
        }
    }

    // Step 3: end-of-tick service. Drain whatever the analysts could
    // get to this shift; whatever's left is tomorrow's backlog.
    service_all_queues(state);

    // Step 4: sample queue depth and update saturation timestamps.
    // Done after service so the depth-sample reflects post-service
    // residual rather than the briefly-elevated post-enqueue peak.
    sample_queue_stats(state);
}

fn activate_ready_phases(
    campaign: &mut CampaignState,
    chain: &KillChain,
    tick: u32,
    rng: &mut impl Rng,
    attacker_budget: Option<f64>,
    metric_history: &[MetricSnapshot],
) {
    let mut to_activate: Vec<PhaseId> = Vec::new();

    // Entry phase: activate on first tick if still pending and no other
    // phase has been activated.
    let any_started = campaign
        .phase_status
        .values()
        .any(|s| !matches!(s, PhaseStatus::Pending));
    if !any_started
        && let Some(PhaseStatus::Pending) = campaign.phase_status.get(&chain.entry_phase)
    {
        to_activate.push(chain.entry_phase.clone());
    }

    for pid in to_activate {
        if let Some(phase) = chain.phases.get(&pid) {
            // Budget check: if attacker would overspend,
            // the phase cannot begin and is marked Failed. We still
            // resolve branches so that any `OnFailure` branch defined
            // on this phase (typically the entry phase) can activate
            // a cheaper fallback path instead of leaving the chain
            // permanently stuck.
            if let Some(cap) = attacker_budget
                && campaign.attacker_spend + phase.cost.attacker_dollars > cap
            {
                campaign
                    .phase_status
                    .insert(pid.clone(), PhaseStatus::Failed { tick });
                resolve_branches(
                    campaign,
                    chain,
                    &pid,
                    tick,
                    rng,
                    attacker_budget,
                    metric_history,
                );
                continue;
            }
            let duration = sample_duration(phase, rng);
            campaign.phase_status.insert(
                pid,
                PhaseStatus::Active {
                    started_at: tick,
                    duration,
                },
            );
        }
    }
}

fn sample_duration(phase: &CampaignPhase, rng: &mut impl Rng) -> u32 {
    if phase.max_duration <= phase.min_duration {
        return phase.min_duration.max(1);
    }
    rng.gen_range(phase.min_duration..=phase.max_duration)
}

fn resolve_branches(
    campaign: &mut CampaignState,
    chain: &KillChain,
    completed_pid: &PhaseId,
    tick: u32,
    rng: &mut impl Rng,
    attacker_budget: Option<f64>,
    metric_history: &[MetricSnapshot],
) {
    let status = campaign.phase_status[completed_pid].clone();
    let phase = match chain.phases.get(completed_pid) {
        Some(p) => p,
        None => return,
    };
    for branch in &phase.branches {
        if branch_matches(&branch.condition, &status, rng, metric_history)
            && let Some(PhaseStatus::Pending) = campaign.phase_status.get(&branch.next_phase)
            && let Some(next_phase) = chain.phases.get(&branch.next_phase)
        {
            if let Some(cap) = attacker_budget
                && campaign.attacker_spend + next_phase.cost.attacker_dollars > cap
            {
                // Mark this branch unaffordable and keep scanning —
                // another matching branch may point to a cheaper phase
                // the attacker can still execute.
                campaign
                    .phase_status
                    .insert(branch.next_phase.clone(), PhaseStatus::Failed { tick });
                continue;
            }
            let duration = sample_duration(next_phase, rng);
            campaign.phase_status.insert(
                branch.next_phase.clone(),
                PhaseStatus::Active {
                    started_at: tick,
                    duration,
                },
            );
            return; // first affordable matching branch wins
        }
    }
}

fn branch_matches(
    cond: &BranchCondition,
    status: &PhaseStatus,
    rng: &mut impl Rng,
    metric_history: &[MetricSnapshot],
) -> bool {
    match cond {
        BranchCondition::OnSuccess => status.succeeded(),
        BranchCondition::OnFailure => matches!(status, PhaseStatus::Failed { .. }),
        BranchCondition::OnDetection => status.detected(),
        BranchCondition::Probability { p } => rng.r#gen::<f64>() < *p,
        BranchCondition::Always => true,
        BranchCondition::EscalationThreshold {
            metric,
            threshold,
            direction,
            sustained_ticks,
        } => escalation_threshold_satisfied(
            metric_history,
            metric,
            *threshold,
            *direction,
            *sustained_ticks,
        ),
        // Short-circuit OR: stops at the first matching inner condition.
        // Probability inner conditions still consume their RNG draw
        // when reached, so the run remains deterministic given the
        // declared inner-condition order.
        BranchCondition::OrAny { conditions } => conditions
            .iter()
            .any(|inner| branch_matches(inner, status, rng, metric_history)),
    }
}

/// Has `metric` been on `direction` of `threshold` for at least
/// `sustained_ticks` consecutive end-of-tick snapshots?
///
/// `sustained_ticks == 0` is treated as "must currently be on the right
/// side" — a single tick of crossing is enough. If the history is
/// shorter than `sustained_ticks` (early in the run), the condition is
/// false: we can't yet say it has "stayed" on the right side because
/// we haven't observed long enough.
fn escalation_threshold_satisfied(
    history: &[MetricSnapshot],
    metric: &EscalationMetric,
    threshold: f64,
    direction: ThresholdDirection,
    sustained_ticks: u32,
) -> bool {
    let need = (sustained_ticks as usize).max(1);
    if history.len() < need {
        return false;
    }
    let window = &history[history.len() - need..];
    window.iter().all(|snap| {
        let value = read_metric(snap, metric);
        match direction {
            ThresholdDirection::Above => value >= threshold,
            ThresholdDirection::Below => value <= threshold,
        }
    })
}

fn read_metric(snap: &MetricSnapshot, metric: &EscalationMetric) -> f64 {
    match metric {
        EscalationMetric::Tension => snap.tension,
        EscalationMetric::InformationDominance => snap.information_dominance,
        EscalationMetric::InstitutionalErosion => snap.institutional_erosion,
        EscalationMetric::CoercionPressure => snap.coercion_pressure,
        EscalationMetric::PoliticalCost => snap.political_cost,
    }
}

fn apply_phase_output(
    state: &mut SimulationState,
    scenario: &Scenario,
    campaign: &mut CampaignState,
    output: &PhaseOutput,
) {
    match output {
        PhaseOutput::IntelligenceGain { .. } => {
            // Intelligence is modeled implicitly through
            // `prerequisite_success_boost` — no direct state mutation.
        },
        PhaseOutput::InfraDamage { region, factor } => {
            damage_infra_in_region(state, scenario, region, *factor);
        },
        PhaseOutput::TensionDelta { delta } => {
            state.political_climate.tension =
                (state.political_climate.tension + delta).clamp(0.0, 1.0);
        },
        PhaseOutput::MoraleDelta { faction, delta } => {
            if let Some(fs) = state.faction_states.get_mut(faction) {
                fs.morale = (fs.morale + delta).clamp(0.0, 1.0);
            }
        },
        PhaseOutput::InformationDominance { delta } => {
            campaign.information_dominance =
                (campaign.information_dominance + delta).clamp(-1.0, 1.0);
            state.non_kinetic.information_dominance =
                (state.non_kinetic.information_dominance + delta).clamp(-1.0, 1.0);
        },
        PhaseOutput::InstitutionalErosion { delta } => {
            campaign.institutional_erosion =
                (campaign.institutional_erosion + delta).clamp(0.0, 1.0);
            state.non_kinetic.institutional_erosion =
                (state.non_kinetic.institutional_erosion + delta).clamp(0.0, 1.0);
            // Erode institution loyalty proportionally.
            let loyalty_drop = delta * 0.5;
            for loyalty in state.institution_loyalty.values_mut() {
                *loyalty = (*loyalty - loyalty_drop).clamp(0.0, 1.0);
            }
        },
        PhaseOutput::CoercionPressure { delta } => {
            campaign.coercion_pressure = (campaign.coercion_pressure + delta).clamp(0.0, 1.0);
            state.non_kinetic.coercion_pressure =
                (state.non_kinetic.coercion_pressure + delta).clamp(0.0, 1.0);
        },
        PhaseOutput::PoliticalCost { delta } => {
            campaign.political_cost = (campaign.political_cost + delta).clamp(0.0, 1.0);
            state.non_kinetic.political_cost =
                (state.non_kinetic.political_cost + delta).clamp(0.0, 1.0);
        },
        PhaseOutput::Custom { .. } => {},
        PhaseOutput::LeadershipDecapitation {
            target_faction,
            morale_shock,
        } => {
            apply_leadership_decapitation(state, scenario, target_faction, *morale_shock);
        },
    }
}

/// Apply a leadership decapitation to `target_faction`.
///
/// Mutates the runtime faction state — advances the rank index,
/// records the strike tick, increments the cumulative count, and
/// applies a one-shot morale drop.
///
/// Scenario validation rejects `LeadershipDecapitation` against any
/// faction that does not declare a `LeadershipCadre`, so the runtime
/// path here can assume the cadre exists when the target's faction
/// state does. The defensive `cadre_len` lookup remains so a
/// counterfactual override that mutates `leadership` post-validation
/// (or a hand-built `Scenario` that bypasses `validate_scenario`) does
/// not advance into a non-existent rank list.
fn apply_leadership_decapitation(
    state: &mut SimulationState,
    scenario: &Scenario,
    target: &FactionId,
    morale_shock: f64,
) {
    // Look up the cadre on the scenario side to decide whether to
    // advance the rank counter at all.
    let cadre_len = scenario
        .factions
        .get(target)
        .and_then(|f| f.leadership.as_ref())
        .map(|c| c.ranks.len() as u32);

    // `command_resilience` ∈ [0.0, 1.0] attenuates the one-shot morale
    // drop from a successful decapitation strike: 0.0 = full shock
    // (legacy default behavior), 1.0 = strike still advances the rank
    // index but does not depress morale at all (a faction with deeply-
    // rehearsed succession protocols absorbs the loss without panic).
    // Values outside the range are clamped rather than rejected so a
    // counterfactual override that pushes the parameter out of bounds
    // still produces a sensible attenuation factor. NaN is treated as
    // 0.0 (full shock) rather than passed through `clamp` — `f64::clamp`
    // returns NaN when `self` is NaN, which would propagate into
    // `effective_shock` and silently corrupt morale. The explicit guard
    // matches the graceful-degradation pattern used for `morale_modifier`
    // in `tick::find_contested_regions`.
    let resilience = scenario
        .factions
        .get(target)
        .map(|f| {
            if f.command_resilience.is_nan() {
                0.0
            } else {
                f.command_resilience.clamp(0.0, 1.0)
            }
        })
        .unwrap_or(0.0);

    let Some(fs) = state.faction_states.get_mut(target) else {
        return;
    };

    fs.leadership_decapitations = fs.leadership_decapitations.saturating_add(1);
    fs.last_decapitation_tick = Some(state.tick);

    if let Some(len) = cadre_len {
        // Saturate at len — once past the end the faction is
        // leaderless. `saturating_add` plus `.min(len)` prevents both
        // u32 overflow on pathological repeat-strike scenarios and
        // nonsensical large indices in the report.
        fs.current_leadership_rank = fs.current_leadership_rank.saturating_add(1).min(len);
    }

    // morale_shock NaN is rejected at validation, but we keep the
    // `> 0.0` guard so a hand-built scenario that bypasses validation
    // produces a no-op rather than a NaN-poisoned morale value.
    if morale_shock > 0.0 {
        let effective_shock = morale_shock * (1.0 - resilience);
        fs.morale = (fs.morale - effective_shock).clamp(0.0, 1.0);
    }
}

fn apply_detection_penalty(state: &mut SimulationState, _phase: &CampaignPhase) {
    // Detection raises political tension as public awareness grows.
    state.political_climate.tension = (state.political_climate.tension + 0.05).clamp(0.0, 1.0);
}

fn damage_infra_in_region(
    state: &mut SimulationState,
    scenario: &Scenario,
    region: &RegionId,
    factor: f64,
) {
    for (iid, node) in &scenario.map.infrastructure {
        if node.region == *region
            && let Some(status) = state.infra_status.get_mut(iid)
        {
            *status = (*status - factor).max(0.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Defender capacity / queue dynamics
// ---------------------------------------------------------------------------

/// Drain every defender queue once per tick at its declared service
/// rate. No-op when the scenario declares no `defender_capacities`.
fn service_all_queues(state: &mut SimulationState) {
    for roles in state.defender_queues.values_mut() {
        for q in roles.values_mut() {
            q.service();
        }
    }
}

/// Look up a queue by `(faction, role)`. Returns `None` when the
/// faction has no defender capacities or the role is unknown — the
/// engine treats those as "no gating" rather than erroring, since
/// scenario validation already rejected references to undeclared
/// roles at load time.
fn queue_mut<'a>(
    state: &'a mut SimulationState,
    faction: &FactionId,
    role: &DefenderRoleId,
) -> Option<&'a mut crate::state::DefenderQueueState> {
    state
        .defender_queues
        .get_mut(faction)
        .and_then(|roles| roles.get_mut(role))
}

/// Resolve the saturated-detection multiplier for a phase that names a
/// defender role.
///
/// Returns `1.0` (no penalty) when the phase doesn't declare
/// `gated_by_defender`, the named faction or role is missing from the
/// runtime state, or the queue is below capacity. Otherwise returns
/// the role's `saturated_detection_factor` from the scenario.
fn saturated_factor_for(
    state: &SimulationState,
    scenario: &Scenario,
    role_ref: Option<&DefenderRoleRef>,
) -> f64 {
    let Some(rr) = role_ref else {
        return 1.0;
    };
    let Some(roles) = state.defender_queues.get(&rr.faction) else {
        return 1.0;
    };
    let Some(q) = roles.get(&rr.role) else {
        return 1.0;
    };
    if !q.is_saturated() {
        return 1.0;
    }
    scenario
        .factions
        .get(&rr.faction)
        .and_then(|f| f.defender_capacities.get(&rr.role))
        .map_or(1.0, |cap| cap.saturated_detection_factor)
}

/// Push synthetic work items into defender queues for one active
/// phase. Items are sampled from a Poisson distribution with mean
/// `items_per_tick`; the engine RNG is the source so determinism
/// holds bit-for-bit. Called per-phase from the campaign tick *before*
/// that phase's detection roll, so the saturation check reads the
/// post-arrival depth.
fn enqueue_phase_noise(
    state: &mut SimulationState,
    scenario: &Scenario,
    phase: &CampaignPhase,
    rng: &mut impl Rng,
) {
    for noise in &phase.defender_noise {
        let count = sample_poisson(rng, noise.items_per_tick.max(0.0));
        if count == 0 {
            continue;
        }
        enqueue_with_overflow(state, scenario, &noise.defender, &noise.role, count, false);
    }
}

/// Hard cap on overflow-chain depth.
///
/// Validation rejects authored cycles at scenario load, so this is
/// defense in depth against (a) hand-built `SimulationState` fixtures
/// that bypass the loader, and (b) any future schema mutation that
/// might let a chain grow past a sane operational depth. A real SOC
/// escalation ladder is at most 3–4 deep; 32 is a generous safety
/// margin without any meaningful runtime cost.
const MAX_OVERFLOW_CHAIN_DEPTH: u32 = 32;

/// Enqueue `count` items into `(faction, role)` with optional
/// cross-role spillover.
///
/// Per-role behavior:
/// - Resolve the role's `DefenderCapacity` from the scenario.
/// - If the role declares `overflow_to`, split `count` into
///   `(direct, spillover)` where `direct` is whatever fits below
///   `overflow_threshold * queue_depth` (default `1.0` = full
///   capacity), and `spillover` is the rest. The split is computed
///   *before* applying the overflow policy — overflow takes
///   precedence over `OverflowPolicy::DropNew` because the analyst
///   intent of declaring `overflow_to` is "escalate, don't drop".
/// - Apply the existing `OverflowPolicy` to `direct` only — items
///   in the direct portion that still don't fit (e.g. a Backlog
///   queue whose depth is already past the threshold) accumulate
///   per the policy.
/// - Recursively enqueue `spillover` to the named overflow target.
///
/// Roles without `overflow_to` reproduce the legacy Epic K behavior
/// exactly: every item is "direct", no spillover, no recursion.
///
/// `is_spillover` flags whether *this* enqueue arrived via escalation
/// from another role (so we can break out the
/// `spillover_in` counter). The initial call from
/// `enqueue_phase_noise` always passes `false`; recursive calls pass
/// `true`.
///
/// Determinism: every step is a pure function of the scenario, the
/// state, and `count`. No RNG; the Poisson draw happened once at the
/// top of the phase-noise loop.
fn enqueue_with_overflow(
    state: &mut SimulationState,
    scenario: &Scenario,
    faction: &FactionId,
    role: &DefenderRoleId,
    count: u32,
    is_spillover: bool,
) {
    enqueue_with_overflow_inner(state, scenario, faction, role, count, is_spillover, 0);
}

fn enqueue_with_overflow_inner(
    state: &mut SimulationState,
    scenario: &Scenario,
    faction: &FactionId,
    role: &DefenderRoleId,
    count: u32,
    is_spillover: bool,
    chain_depth: u32,
) {
    if count == 0 {
        return;
    }
    if chain_depth >= MAX_OVERFLOW_CHAIN_DEPTH {
        // Defense in depth against hand-built `SimulationState`
        // fixtures that bypass scenario validation (which already
        // rejects authored cycles, so a real TOML can never reach
        // this branch). The upstream caller has already incremented
        // its `spillover_out` counter for these items, so silently
        // returning would break the chain-conservation invariant the
        // report relies on. Surface the loss as a drop on the
        // would-be-target queue instead. If the target queue itself
        // is missing (a deeper malformation — validation rejects
        // unknown roles too), log a warning so the broken invariant
        // is visible rather than silent.
        if let Some(q) = queue_mut(state, faction, role) {
            q.total_dropped += u64::from(count);
        } else {
            tracing::warn!(
                faction = %faction,
                role = %role,
                count,
                "MAX_OVERFLOW_CHAIN_DEPTH guard fired but target queue not in state — \
                 malformed fixture bypassed scenario validation; chain-conservation invariant broken"
            );
        }
        return;
    }

    let Some(cap) = scenario
        .factions
        .get(faction)
        .and_then(|f| f.defender_capacities.get(role))
    else {
        // Defense in depth: validation rejects unknown roles at
        // scenario load, so this branch is reachable only via
        // hand-built fixtures that bypass the loader. When this is a
        // spillover call, the upstream caller already incremented its
        // `spillover_out`; silently returning would leave the
        // chain-conservation invariant `parent.spillover_out ==
        // child.spillover_in` broken with no diagnostic. Best-effort:
        // if the queue exists, charge `spillover_in` (closing the
        // chain link) and `total_dropped` (we have no cap to route
        // under). Otherwise, log so the loss is visible.
        if is_spillover {
            if let Some(q) = queue_mut(state, faction, role) {
                q.spillover_in += u64::from(count);
                q.total_dropped += u64::from(count);
            } else {
                tracing::warn!(
                    faction = %faction,
                    role = %role,
                    count,
                    "spillover target has no defender_capacity entry and no queue in state — \
                     malformed fixture bypassed scenario validation; chain-conservation invariant broken"
                );
            }
        }
        return;
    };
    let policy = cap.overflow;
    let queue_depth_cap = cap.queue_depth;
    let overflow_target = cap.overflow_to.clone();
    // Validation rejects out-of-range and non-finite thresholds at
    // scenario load (see `validate_defender_capacities`), so the
    // engine trusts the value here and the previous `.clamp(0.0, 1.0)`
    // was redundant defensive code on the hot path.
    let threshold = cap.overflow_threshold.unwrap_or(1.0);

    let spillover = {
        let Some(q) = queue_mut(state, faction, role) else {
            // Defense in depth: cap exists but queue is missing —
            // only reachable via hand-built fixtures. Mirrors the
            // depth-guard fix: warn on the spillover path so the
            // broken `parent.spillover_out == child.spillover_in`
            // invariant is visible rather than silent.
            if is_spillover {
                tracing::warn!(
                    faction = %faction,
                    role = %role,
                    count,
                    "spillover target queue missing despite defender_capacity declaration — \
                     malformed fixture bypassed scenario validation; chain-conservation invariant broken"
                );
            }
            return;
        };
        // `spillover_in` tracks the chain link from upstream — it's
        // the count that arrived here from another saturated role,
        // independent of how much of it then further spills. Pairs
        // with the upstream role's `spillover_out` for the
        // conservation invariant `A.spillover_out == B.spillover_in`.
        if is_spillover {
            q.spillover_in += u64::from(count);
        }

        let (direct, spillover) = if overflow_target.is_some() {
            // Threshold rounding: use ceil so a fractional threshold of
            // e.g. 0.5 against capacity 3 yields a threshold of 2 (not
            // 1) — round-up reads more naturally as "spill once you've
            // crossed half-capacity" than the floor rounding which
            // would spill *at* half. The clamp ensures we never read
            // below the current depth (saturation is sticky once
            // reached, even if the queue has drained to under the
            // threshold this same tick). `threshold = 0.0` is a
            // legitimate authoring choice meaning "spill 100% of
            // arrivals from the start"; ceil(0.0) = 0 yields zero
            // headroom, so every item routes to spillover immediately.
            let threshold_depth =
                ((f64::from(queue_depth_cap) * threshold).ceil() as u32).min(queue_depth_cap);
            let headroom = threshold_depth.saturating_sub(q.depth);
            let direct = count.min(headroom);
            (direct, count - direct)
        } else {
            (count, 0)
        };

        // `total_enqueued` charges only the direct portion: items
        // that further spill to another role never enter this queue's
        // policy and must not inflate this row's throughput counter
        // (the docs on `DefenderQueueState.spillover_out` /
        // `DefenderQueueReport.spillover_out` pin this contract).
        q.total_enqueued += u64::from(direct);

        if direct > 0 {
            apply_policy_to_direct(q, direct, queue_depth_cap, policy);
        }
        if spillover > 0 {
            q.spillover_out += u64::from(spillover);
        }
        spillover
    };

    if spillover > 0
        && let Some(target) = overflow_target
    {
        enqueue_with_overflow_inner(
            state,
            scenario,
            faction,
            &target,
            spillover,
            true,
            chain_depth + 1,
        );
    }
}

/// Apply `count` items to `q` under `policy`, updating depth and
/// drop counters. Caller is responsible for incrementing
/// `total_enqueued`.
fn apply_policy_to_direct(
    q: &mut crate::state::DefenderQueueState,
    count: u32,
    queue_depth_cap: u32,
    policy: OverflowPolicy,
) {
    if queue_depth_cap == 0 {
        // Degenerate: a role with capacity 0 drops everything regardless.
        q.total_dropped += u64::from(count);
        return;
    }
    match policy {
        OverflowPolicy::DropNew => {
            let headroom = queue_depth_cap.saturating_sub(q.depth);
            let accepted = count.min(headroom);
            let dropped = count - accepted;
            q.depth += accepted;
            q.total_dropped += u64::from(dropped);
        },
        OverflowPolicy::DropOldest => {
            // Oldest-eviction in a depth-only queue (no per-item
            // identity) collapses to: accept up to capacity, count
            // the rest as effective drops of older items. Net depth
            // never exceeds capacity.
            let new_depth = queue_depth_cap.min(q.depth.saturating_add(count));
            let dropped_count = q.depth.saturating_add(count).saturating_sub(new_depth);
            q.total_dropped += u64::from(dropped_count);
            q.depth = new_depth;
        },
        OverflowPolicy::Backlog => {
            q.depth = q.depth.saturating_add(count);
        },
    }
}

/// Inverse-transform Poisson sampling. Used over the `rand_distr`
/// crate so the engine doesn't pick up a new dependency just for
/// this one variate. For the small means we sample (typically `< 50`)
/// the simple Knuth method is both correct and faster than the
/// rejection-based variants in `rand_distr`.
fn sample_poisson(rng: &mut impl Rng, mean: f64) -> u32 {
    if mean <= 0.0 || !mean.is_finite() {
        return 0;
    }
    let l = (-mean).exp();
    let mut k: u32 = 0;
    let mut p: f64 = 1.0;
    loop {
        k += 1;
        p *= rng.r#gen::<f64>();
        if p <= l {
            return k - 1;
        }
        // Defensive cap: at extreme means, the loop is bounded
        // anyway (E[k] = mean), but a stuck draw under arithmetic
        // pathologies returns the mean rather than spinning.
        if k > 100_000 {
            return mean as u32;
        }
    }
}

/// At the end of the tick: update `max_depth`, `first_saturated_at`,
/// and the running depth-sum used for mean-utilization reporting.
fn sample_queue_stats(state: &mut SimulationState) {
    let tick = state.tick;
    for roles in state.defender_queues.values_mut() {
        for q in roles.values_mut() {
            q.ticks_observed = q.ticks_observed.saturating_add(1);
            q.total_depth_sum = q.total_depth_sum.saturating_add(u64::from(q.depth));
            if q.depth > q.max_depth {
                q.max_depth = q.depth;
            }
            if q.is_saturated() && q.first_saturated_at.is_none() {
                q.first_saturated_at = Some(tick);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests for EscalationThreshold
// ---------------------------------------------------------------------------

#[cfg(test)]
mod escalation_tests {
    use super::*;

    fn snap(tick: u32, tension: f64) -> MetricSnapshot {
        MetricSnapshot {
            tick,
            tension,
            information_dominance: 0.0,
            institutional_erosion: 0.0,
            coercion_pressure: 0.0,
            political_cost: 0.0,
        }
    }

    #[test]
    fn threshold_above_satisfied_after_sustained_window() {
        // Three consecutive ticks ≥ 0.7 satisfies a `sustained_ticks=3`
        // window — but not a `sustained_ticks=4` window because the
        // history is only 3 entries deep.
        let history = vec![snap(0, 0.8), snap(1, 0.85), snap(2, 0.9)];
        assert!(escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.7,
            ThresholdDirection::Above,
            3,
        ));
        assert!(!escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.7,
            ThresholdDirection::Above,
            4,
        ));
    }

    #[test]
    fn threshold_above_short_dip_breaks_window() {
        // Tick 1 dipped below 0.7. The most recent 3 ticks are not all
        // above threshold, so the window is unsatisfied.
        let history = vec![snap(0, 0.85), snap(1, 0.6), snap(2, 0.85)];
        assert!(!escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.7,
            ThresholdDirection::Above,
            3,
        ));
        // …but a 1-tick window only looks at the latest snapshot,
        // which is back above threshold.
        assert!(escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.7,
            ThresholdDirection::Above,
            1,
        ));
    }

    #[test]
    fn threshold_below_works_symmetrically() {
        let history = vec![snap(0, 0.05), snap(1, 0.10), snap(2, 0.08)];
        assert!(escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.15,
            ThresholdDirection::Below,
            3,
        ));
        // Tick 1 hit 0.10, which is still ≤ 0.10, so satisfied.
        assert!(escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.10,
            ThresholdDirection::Below,
            3,
        ));
        // Tighten to 0.09 and tick 1's value of 0.10 breaks it.
        assert!(!escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.09,
            ThresholdDirection::Below,
            3,
        ));
    }

    #[test]
    fn threshold_with_zero_sustained_ticks_checks_latest_only() {
        // sustained_ticks == 0 is interpreted as "must currently be on
        // the right side" — equivalent to a 1-tick window.
        let history = vec![snap(0, 0.1), snap(1, 0.85)];
        assert!(escalation_threshold_satisfied(
            &history,
            &EscalationMetric::Tension,
            0.7,
            ThresholdDirection::Above,
            0,
        ));
    }

    #[test]
    fn threshold_unsatisfied_with_empty_history() {
        assert!(!escalation_threshold_satisfied(
            &[],
            &EscalationMetric::Tension,
            0.5,
            ThresholdDirection::Above,
            1,
        ));
    }
}

// ---------------------------------------------------------------------------
// Tests for defender capacity / queue dynamics
// ---------------------------------------------------------------------------

#[cfg(test)]
mod capacity_tests {
    use super::*;
    use crate::state::DefenderQueueState;

    fn q(capacity: u32, service_rate: f64) -> DefenderQueueState {
        DefenderQueueState::new(capacity, service_rate)
    }

    /// Bump `total_enqueued` then defer to `apply_policy_to_direct` —
    /// for a role without `overflow_to` the production path
    /// (`enqueue_with_overflow_inner`) treats every arrival as
    /// `direct` and charges it to `total_enqueued`, so the legacy
    /// single-queue policy tests below preserve that invariant
    /// exactly.
    fn legacy_enqueue(s: &mut DefenderQueueState, count: u32, policy: OverflowPolicy) {
        s.total_enqueued += u64::from(count);
        apply_policy_to_direct(s, count, s.capacity, policy);
    }

    #[test]
    fn enqueue_drop_new_caps_at_depth_and_counts_dropped() {
        // DropNew: queue saturates at capacity, excess counted as dropped.
        let mut s = q(10, 1.0);
        legacy_enqueue(&mut s, 7, OverflowPolicy::DropNew);
        assert_eq!(s.depth, 7);
        assert_eq!(s.total_dropped, 0);
        // Pushing 5 more — only 3 fit, 2 are dropped.
        legacy_enqueue(&mut s, 5, OverflowPolicy::DropNew);
        assert_eq!(s.depth, 10);
        assert_eq!(s.total_dropped, 2);
        assert_eq!(s.total_enqueued, 12);
    }

    #[test]
    fn enqueue_backlog_grows_unbounded_no_drops() {
        // Backlog: depth grows past capacity, total_dropped stays 0.
        let mut s = q(10, 1.0);
        legacy_enqueue(&mut s, 50, OverflowPolicy::Backlog);
        assert_eq!(s.depth, 50);
        assert_eq!(s.total_dropped, 0);
        assert!(
            s.is_saturated(),
            "backlog depth above capacity is saturated"
        );
    }

    #[test]
    fn enqueue_drop_oldest_caps_at_depth_treats_evictions_as_drops() {
        // DropOldest in a depth-only queue: depth caps at capacity,
        // any enqueues past capacity count as evictions of older
        // items.
        let mut s = q(10, 1.0);
        legacy_enqueue(&mut s, 8, OverflowPolicy::DropOldest);
        assert_eq!(s.depth, 8);
        // Push 5 more — depth caps at 10, 3 evicted.
        legacy_enqueue(&mut s, 5, OverflowPolicy::DropOldest);
        assert_eq!(s.depth, 10);
        assert_eq!(s.total_dropped, 3);
    }

    #[test]
    fn service_drains_at_declared_rate() {
        let mut s = q(100, 5.0);
        s.depth = 50;
        let drained = s.service();
        assert_eq!(drained, 5);
        assert_eq!(s.depth, 45);
        assert_eq!(s.total_serviced, 5);
    }

    #[test]
    fn service_accumulator_carries_fractional_rate_across_ticks() {
        // service_rate = 0.5 means one item every two ticks. Without
        // accumulator carry the queue would never drain.
        let mut s = q(100, 0.5);
        s.depth = 10;
        assert_eq!(s.service(), 0, "tick 1: accumulator at 0.5, no whole");
        assert_eq!(s.depth, 10);
        assert_eq!(s.service(), 1, "tick 2: accumulator at 1.0, drains 1");
        assert_eq!(s.depth, 9);
        assert_eq!(s.service(), 0, "tick 3: accumulator at 0.5 again");
        assert_eq!(s.depth, 9);
    }

    #[test]
    fn service_clamps_to_remaining_depth() {
        // service rate 100 against depth 3 only drains 3, not 100.
        let mut s = q(50, 100.0);
        s.depth = 3;
        assert_eq!(s.service(), 3);
        assert_eq!(s.depth, 0);
    }

    #[test]
    fn poisson_sampler_returns_zero_for_zero_mean() {
        let mut rng = seeded_rng(42);
        for _ in 0..50 {
            assert_eq!(sample_poisson(&mut rng, 0.0), 0);
        }
    }

    #[test]
    fn poisson_sampler_mean_matches_input_in_aggregate() {
        // 5000 Poisson(10) draws should average ≈ 10. Loose bound
        // because variance of Poisson(10) is 10 — a few percent
        // deviation is expected even at large N.
        let mut rng = seeded_rng(123);
        let mut total = 0u64;
        for _ in 0..5000 {
            total += u64::from(sample_poisson(&mut rng, 10.0));
        }
        let mean = total as f64 / 5000.0;
        assert!(
            (mean - 10.0).abs() < 0.5,
            "Poisson(10) mean over 5000 draws should be near 10, got {mean}"
        );
    }

    fn seeded_rng(seed: u64) -> rand_chacha::ChaCha8Rng {
        use rand::SeedableRng;
        rand_chacha::ChaCha8Rng::seed_from_u64(seed)
    }
}
