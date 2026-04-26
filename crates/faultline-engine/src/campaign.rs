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
            let dp = phase.detection_probability_per_tick;
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
                let effective_dp = (dp * saturated_factor).clamp(0.0, 1.0);
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
                    // Cost accounting.
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
// Defender capacity / queue dynamics (Epic K)
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
        let policy = scenario
            .factions
            .get(&noise.defender)
            .and_then(|f| f.defender_capacities.get(&noise.role))
            .map(|cap| cap.overflow)
            .unwrap_or(OverflowPolicy::DropNew);
        if let Some(q) = queue_mut(state, &noise.defender, &noise.role) {
            enqueue_with_policy(q, count, policy);
        }
    }
}

/// Apply `count` enqueues to `q` under `policy`. Updates lifetime
/// counters for both successful enqueues and dropped items.
fn enqueue_with_policy(
    q: &mut crate::state::DefenderQueueState,
    count: u32,
    policy: OverflowPolicy,
) {
    q.total_enqueued += u64::from(count);
    if q.capacity == 0 {
        // Degenerate: a role with capacity 0 drops everything regardless.
        q.total_dropped += u64::from(count);
        return;
    }
    match policy {
        OverflowPolicy::DropNew => {
            let headroom = q.capacity.saturating_sub(q.depth);
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
            let new_depth = q.capacity.min(q.depth.saturating_add(count));
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
// Tests for EscalationThreshold (Epic C)
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
// Tests for defender capacity / queue dynamics (Epic K)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod capacity_tests {
    use super::*;
    use crate::state::DefenderQueueState;

    fn q(capacity: u32, service_rate: f64) -> DefenderQueueState {
        DefenderQueueState::new(capacity, service_rate)
    }

    #[test]
    fn enqueue_drop_new_caps_at_depth_and_counts_dropped() {
        // DropNew: queue saturates at capacity, excess counted as dropped.
        let mut s = q(10, 1.0);
        enqueue_with_policy(&mut s, 7, OverflowPolicy::DropNew);
        assert_eq!(s.depth, 7);
        assert_eq!(s.total_dropped, 0);
        // Pushing 5 more — only 3 fit, 2 are dropped.
        enqueue_with_policy(&mut s, 5, OverflowPolicy::DropNew);
        assert_eq!(s.depth, 10);
        assert_eq!(s.total_dropped, 2);
        assert_eq!(s.total_enqueued, 12);
    }

    #[test]
    fn enqueue_backlog_grows_unbounded_no_drops() {
        // Backlog: depth grows past capacity, total_dropped stays 0.
        let mut s = q(10, 1.0);
        enqueue_with_policy(&mut s, 50, OverflowPolicy::Backlog);
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
        enqueue_with_policy(&mut s, 8, OverflowPolicy::DropOldest);
        assert_eq!(s.depth, 8);
        // Push 5 more — depth caps at 10, 3 evicted.
        enqueue_with_policy(&mut s, 5, OverflowPolicy::DropOldest);
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
