# Engine Model

This document is the authoritative description of the per-tick simulation model and the engine behaviors layered on it. It covers phase ordering, branching, networks, defender queues, leadership, diplomacy, supply, resource contention, narrative/displacement, command effectiveness, utility-driven AI, and belief asymmetry.

Cross-references:
- Per-field parameter wiring (movement rate, morale modifier, tech costs, etc.) → `docs/parameter-audit.md`
- Cross-run analytics and report sections → `docs/analytics.md`
- Schema field reference → `docs/scenario_schema.md`

Source: `crates/faultline-engine/src/engine.rs` (`tick()`) and `crates/faultline-engine/src/tick.rs`.

---

## Per-tick phase order

The following 17-step sequence is executed once per simulation tick. Every phase is a deterministic pure function of `(state, scenario, rng)` unless noted otherwise. Phases are numbered for reference; the names match the function identifiers in `tick.rs` / `engine.rs`.

1. **`event_phase`** — Fires all scripted `[[events]]` scheduled for this tick, applying `EventEffect`s (including `DiplomacyChange`, `MediaEvent`, `Displacement`, `DeceptionOp`, `IntelligenceShare`, `AmbientIntel`, network mutations, etc.) to `SimulationState`.
2. **`decision_phase`** — Each faction's AI selects its top-3 actions via doctrine scoring optionally combined with multi-term utility scoring. Captures `utility_decisions` for post-run reporting.
3. **`movement_phase`** — Queued `MoveUnit` actions are resolved using the effective-mobility gate (`mobility × terrain_modifier × env_factor`), accumulating into `ForceUnit.move_progress`.
4. **`combat_phase`** — Lanchester-attrition combat between faction pairs in contested regions. Respects `combat_blocked` (Allied pairs skip) and tech `coverage_limit` counters.
5. **`attrition_phase`** — Resource income (`resource_rate × supply_pressure`) and outgo (force upkeep, tech `cost_per_tick`) are settled. Supply pressure from owned `kind = "supply"` networks is read here. Tech cards whose `cost_per_tick` exceeds available resources are decommissioned.
6. **`political_phase`** — Political climate, loyalty, and civilian-segment sympathy are updated via `faultline-politics`. Reads `MediaLandscape` fields (`fragmentation`, `social_media_penetration`, `internet_availability`).
7. **`information_phase`** — Disinformation and intel-dominance tension deltas are computed. Reads the same `MediaLandscape` fields as the political phase.
8. **`narrative_phase`** — Persistent narrative store decays, per-faction dominance is scored, segment sympathy is nudged toward the leading faction, and a tension delta is applied. Short-circuits when the narrative store is empty.
9. **`displacement_phase`** — Displaced populations propagate across `Region.borders` adjacencies (10%/tick outflow) and absorb back into the resident population (5%/tick). Short-circuits when the displacement map is empty.
10. **`campaign_phase`** (conditional) — Kill-chain campaign steps are evaluated; defender queues receive Poisson-sampled noise (`defender_noise`), detection rolls are gated by queue saturation (`gated_by_defender`), and per-phase outputs (including `LeadershipDecapitation`) are applied.
11. **`update_command_effectiveness`** — Writes the leadership-cadre degradation factor into `RuntimeFactionState.command_effectiveness`. Short-circuits when no faction declares a `[leadership]` cadre.
12. **`fracture_phase`** — Alliance-fracture rules are evaluated end-of-tick; fired rules write new stances to `SimulationState.diplomacy_overrides`. One-shot per rule (latched in `fired_fractures`).
13. **Network sample capture** — One `NetworkSample` per declared `[networks.*]` is appended: component count, largest-component size, residual capacity, disrupted-node count. Zero-overhead for scenarios without declared networks.
14. **`update_region_control`** — Region controller is recomputed from current force presence.
15. **`belief_phase`** — When `simulation.belief_model.enabled = true`, per-faction belief states are (1) decayed by per-axis rates, (2) refreshed from ground truth for visible entities, (3) pruned below `prune_threshold`. Updates per-faction accuracy counters. Short-circuits in O(1) when belief model is disabled.
16. **Snapshot** (conditional) — A `FactionState` snapshot is captured if `snapshot_interval` is set and this tick matches. Includes `command_effectiveness`.
17. **`victory_check`** — Victory conditions are evaluated; the run terminates if a faction meets its win criterion or `max_ticks` is reached.

> **Determinism contract:** Every phase iterates `BTreeMap`-ordered collections. No `HashMap` is used anywhere in the hot path. The RNG (`ChaCha8Rng`) is advanced by exactly the same sequence for any fixed seed regardless of which optional mechanics are active. The `--verify` CLI mode replays a saved manifest and asserts bit-identical `RunResult` JSON.

---

## BranchCondition and hysteresis branching (Epic C)

`BranchCondition::EscalationThreshold` adds hysteresis to phase branching: a branch fires only when a global metric has stayed on the requested side of a threshold for `sustained_ticks` consecutive end-of-tick snapshots. The engine sizes its rolling metric-history buffer to the longest window any branch in the scenario requests; legacy scenarios with no such branch pay zero overhead.

`BranchCondition::OrAny { conditions }` (Epic D round one) composes inner conditions with short-circuit OR semantics. `max_escalation_window` recurses through `OrAny` so an `EscalationThreshold` nested inside an OR still registers its history buffer requirement. An empty `conditions` list is rejected at validation.

Schema reference: `docs/scenario_schema.md` under `PhaseBranch`.

---

## Network primitives (Epic L)

Scenarios may declare any number of typed graphs via `[networks.<id>]`. Each network has nodes and directed weighted edges with per-edge metadata: `capacity`, `latency`, `bandwidth`, `trust`.

**Runtime mutation** — Three `EventEffect` variants drive per-tick state mutation stored in `SimulationState.network_states`:
- `NetworkEdgeCapacity` — multiplies the named edge's current capacity by the given factor. Composes multiplicatively with prior events; clamped to `[0, 4]` so a runaway author chain cannot poison the residual-capacity series.
- `NetworkNodeDisrupt` — marks a node as disrupted, removing it from path-finding.
- `NetworkInfiltrate` — records a faction infiltration event on the named network.

**Per-tick sampling** — At step 13 above, one `NetworkSample` is appended per declared network: component count, largest-component size, residual capacity, disrupted-node count. The engine path is zero-overhead for scenarios without `[networks.*]`.

**Validation** — Rejects edges with unknown endpoints, self-loops, and event effects targeting unknown networks, nodes, or factions.

**Cross-run analytics** — The Brandes betweenness centrality ranking, max-flow (Edmonds-Karp), fragmentation rate, and per-network mean/max disrupted-node and component counts are computed in `faultline_stats::network_metrics` and documented in `docs/analytics.md`.

Bundled archetype: `scenarios/network_resilience_demo.toml`.

---

## Defender capacity model (Epic K)

Factions may declare per-role investigative queues via `[factions.<id>.defender_capacities.<role>]`. Kill-chain phases hook into these queues via two fields:

- `defender_noise` — Poisson-sampled per-tick alert count pushed onto the named role's queue.
- `gated_by_defender` — When set, that phase's per-tick detection probability is multiplied by the role's `saturated_detection_factor` when the queue is at or above capacity. This reproduces alert-fatigue: a saturated tier-1 queue degrades detection for every phase that gates on it.

**Arrive → assess → service ordering** (within `campaign_phase` at step 10): a phase enqueues its Poisson noise first, the detection roll reads the post-arrival queue depth, then the queue is serviced at end-of-tick. A sequential phase 2 therefore inherits the backlog that phase 1 created in the same tick.

Per-run output: `RunResult.defender_queue_reports`. Cross-run aggregation to `MonteCarloSummary.defender_capacity`; both elide when no faction declares queues.

Bundled archetype: `scenarios/alert_fatigue_soc.toml`.

---

## Multi-front resource contention and escalation chains (Epic D round three, item 3)

Extends the defender capacity model with a declarative escalation chain so that saturated roles spill overflow work to a named downstream role.

**Schema** — Two new optional fields on `DefenderCapacity` (in `crates/faultline-types/src/faction.rs`):
- `overflow_to: Option<DefenderRoleId>` — names another role on the *same faction* whose queue receives spillover when this role saturates.
- `overflow_threshold: Option<f64>` — defaults to `1.0` in the engine; the queue-depth fraction at which spillover engages. Setting `0.8` against `queue_depth = 100` means "escalate once depth crosses 80", modelling proactive load-shed policy.

Both fields are `#[serde(default, skip_serializing_if = "Option::is_none")]` so legacy TOML loads byte-identically and roles without `overflow_to` cost zero overhead on the hot path.

**Engine spillover** (`campaign.rs::enqueue_with_overflow`) — Per Poisson noise draw: if `overflow_to.is_some()`, split the count into `(direct, spillover)` where `direct` fills headroom under `ceil(queue_depth × threshold)` and `spillover` is the remainder. The existing `OverflowPolicy` applies to `direct` only; `enqueue_with_overflow` is called recursively on the spillover portion against the named downstream role. Spillover takes precedence over `OverflowPolicy::DropNew` — declaring `overflow_to` is the analyst's signal that "escalate, don't drop" is the intended semantic.

**Queue accounting** on `state::DefenderQueueState`:
- `spillover_in` — cumulative count received via another role's overflow chain.
- `spillover_out` — cumulative count this role redirected downstream (not charged to `total_enqueued`).
- `total_enqueued` — items that entered this role's queue policy only; items that spilled onward are not counted.
- Conservation invariant: for any saturated role A with `overflow_to = B`, `A.spillover_out == B.spillover_in` (modulo the `MAX_OVERFLOW_CHAIN_DEPTH = 32` recursion guard, which is defense-in-depth against hand-built fixtures only — validated authoring can never produce a cycle).

**Validation** rejects six shapes: unknown `overflow_to` role; cross-faction overflow; self-loops (`tier1 -> tier1`); cycles (BFS from each role, reject on revisit); `overflow_threshold` outside `[0, 1]` or NaN; `overflow_threshold` set without `overflow_to`.

Per-run output: `RunResult.defender_queue_reports` rows gain `spillover_in` / `spillover_out`. Cross-run: `MonteCarloSummary.defender_capacity` rows gain `mean_spillover_in` / `mean_spillover_out`. Report section gains a "Cross-role escalation" sub-table gated on any non-zero spillover.

Bundled archetype: `scenarios/multifront_soc_escalation.toml` (3-tier SOC: tier-1 → tier-2 → tier-3 forensics).

---

## Engine model depth: environment windows and leadership cadres (Epic D round one)

All additions are `#[serde(default)]` so legacy scenarios load unchanged.

### Environment windows

`[[environment.windows]]` declares a global schedule with `Always` / `TickRange` / `Cycle` activation variants. Two per-tick read points:
- `environment_defense_factor` (in `tick.rs`) — per-terrain `defense_factor` multiplies into combat `terrain_defense`.
- `environment_detection_factor` (in `tick.rs`) — global `detection_factor` multiplies into every kill-chain phase's per-tick detection probability *before* saturation gating, naturally narrowing the shadow-detection window between unattenuated and saturated rolls.
- `environment_movement_factor` — globally-scoped weather/time-of-day attenuator that composes multiplicatively with the per-unit `mobility` and per-region `TerrainModifier.movement_modifier` to produce the effective mobility gate (see `docs/parameter-audit.md`).

### Leadership cadres

`[factions.<id>.leadership]` declares a named-rank cadre with `succession_recovery_ticks` and `succession_floor`. The `PhaseOutput::LeadershipDecapitation { target_faction, morale_shock }` variant:
1. Advances the faction's rank index.
2. Applies a one-shot morale drop (attenuated by `Faction.command_resilience` — see `docs/parameter-audit.md`).
3. Records the strike tick.

Past-end = leaderless: `command_effectiveness` floors at zero (see the Command Effectiveness section below).

**Validation** rejects `LeadershipDecapitation` targeting a faction without a `leadership` cadre.

---

## Coalition fracture and diplomatic stance (Epic D rounds two and three)

### Coalition fracture (Epic D round two)

`[factions.<id>.alliance_fracture]` declares one or more `FractureRule { id, counterparty, new_stance, condition }`. Five condition variants:
- `AttributionThreshold { attacker, threshold }` — mean attribution across attacker's chains crosses threshold.
- `MoraleFloor { floor }` — faction morale drops below floor.
- `TensionThreshold { threshold }` — global tension crosses threshold.
- `EventFired { event }` — a named event fires during the run.
- `StrengthLossFraction { delta_fraction }` — cumulative strength loss fraction crosses delta.

`fracture_phase` (step 12) evaluates all rules end-of-tick after `campaign_phase`. Each rule is one-shot (latched in `SimulationState.fired_fractures`). Fired rules write to the shared `SimulationState.diplomacy_overrides` map, readable via `fracture::current_stance` / `fracture::baseline_stance`.

`EventEffect::DiplomacyChange` is also wired in `tick.rs::apply_event_effects` and writes to the same override map.

**Validation** rejects: empty rules vector; unknown counterparty / attacker / event ids; self-targeting rules; duplicate rule ids within a faction; NaN / out-of-range thresholds; `AttributionThreshold` against a faction that owns no kill chain.

Per-run output: `RunResult.fracture_events`. Cross-run: `MonteCarloSummary.alliance_dynamics` (per-rule fire rate, mean fire tick, terminal-stance distribution). Report section: `## Alliance Dynamics` in `crates/faultline-stats/src/report/alliance_dynamics.rs`.

**Scope note:** The victory-check and political phases do not currently consult diplomacy.

Bundled archetype: `scenarios/coalition_fracture_demo.toml`.

### Diplomatic stance behavioral coupling (Epic D round three, item 1)

`crates/faultline-engine/src/diplomacy.rs` provides two pure helpers:

- `combat_blocked(state, scenario, a, b)` — returns `true` iff both A→B and B→A current stances are `Diplomacy::Allied`. Mutual alliance is required; one-sided declarations do not bind. Reads `fracture::current_stance` so post-fracture and `DiplomacyChange` overrides are live.
- `ai_threat_multiplier(state, scenario, self_id, other)` — scales `other`'s contribution to `self_id`'s perceived threat and attack-priority:
  - `Allied` → 0.0 (excluded from threat/attack scoring).
  - `Cooperative` → 0.3 (`COOPERATIVE_AI_FACTOR`).
  - Else → 1.0.
  Self-perspective only: a faction that mistakenly views a hostile party as Allied will fail to defend against them — this asymmetry is the intended diagnostic signal in miscalibrated-diplomacy scenarios.

**Combat hook** — `tick::combat_phase` (step 4) calls `combat_blocked` before resolving each faction pair. Cooperative pairs still fight; the relationship is "we cooperate but aren't sworn allies."

**AI hook** — `ai::compute_enemy_presence` and both `evaluate_attack_actions` variants (ground-truth + fog) consult `ai_threat_multiplier`. The fog-of-war path reads stance from ground truth because a faction always knows its own declared posture.

**RNG preservation** — The RNG draw in `evaluate_attack_actions` happens *before* the diplomacy multiplier check, so adding an `Allied` declaration to a legacy scenario does not desync the RNG sequence for unaffected pairs.

**Validation** rejects: self-stance declarations; unknown `target_faction`; duplicate target entries.

**Backward-compat** — Scenarios without authored diplomacy default every pair to `Neutral`, which preserves legacy combat semantics.

Coverage: `crates/faultline-engine/tests/diplomacy_behavior.rs`.

---

## Supply-network interdiction (Epic D round three, item 2)

`kind = "supply"` networks (declared via Epic L network primitives) now attenuate the owning faction's income each attrition tick.

**Implementation** in `crates/faultline-engine/src/supply.rs`:
- `is_active_supply_network(net)` — true iff `kind` matches `"supply"` case-insensitively *and* `owner` is `Some`.
- `supply_pressure_for_faction(scenario, state, faction)` — returns `(pressure ∈ [0, 1], sampled: bool)`. The `sampled` flag is `true` iff at least one non-degenerate owned supply network contributed; a faction whose only supply networks have zero baseline capacity does not produce phantom "supply intact" samples.

**Pressure formula** — For each owned supply network: `pressure_n = (residual_capacity / baseline_capacity).clamp(0, 1)`. Per-faction pressure is the product across all owned supply networks. Networks with `baseline = 0` (all edges zero capacity) are skipped rather than treated as fully broken.

**Hook point** — Top of `tick::attrition_phase` (step 5). Income is `resource_rate × pressure`; upkeep is *not* attenuated (units still consume regardless of resupply state).

**Pressure reporting threshold** — `PRESSURE_REPORTING_THRESHOLD = 0.9`: pressure values strictly below this count toward `pressured_ticks` in the per-faction running counters. This is cosmetic; income scaling reads the raw value.

**Validation** — `kind = "supply"` without `owner` is rejected at scenario load (silent no-op shape).

Per-run output: `RunResult.supply_pressure_reports` (`BTreeMap<FactionId, SupplyPressureReport>`). Cross-run: `MonteCarloSummary.supply_pressure_summaries`. Report section: `## Supply Pressure` in `crates/faultline-stats/src/report/supply_pressure.rs`.

Bundled archetype: `scenarios/supply_interdiction_demo.toml`.

---

## Narrative competition and displacement flows (Epic D round three, item 4)

### Narrative store and `narrative_phase` (step 8)

`EventEffect::MediaEvent { narrative, credibility, reach, favors }` (wired in `tick::apply_event_effects`) registers or reinforces a `NarrativeRuntimeState` keyed on the narrative string:
- **Reinforcement strength** adds `credibility × reach × (1 + 0.5 × fragmentation)` to existing strength, clamped to `[0, 1]`. Fragmented audiences reinforce faster.
- `credibility` and `reach` take the *max* of pre-existing and new values (a higher-reach reinforcement pulls the live narrative up, not down).
- `favors` is sticky to the first firing's choice — a later "switch-sides" reinforcement cannot silently flip dominance attribution.
- Each firing pushes a `NarrativeEvent` onto `SimulationState.narrative_events` with `was_new` distinguishing introductions from reinforcements.

`tick::narrative_phase` runs at step 8 after `information_phase`:
1. Decays each narrative's strength by `BASE_NARRATIVE_DECAY × (1 - 0.5 × reach)`. High-reach narratives decay at half the rate.
2. Drops entries below `NARRATIVE_DROP_EPSILON = 0.005`.
3. Scores per-faction dominance as `sum(strength × credibility)` over narratives favoring each faction. The leading faction (lexicographic tie-break) accrues a tick on `SimulationState.narrative_dominance_ticks`.
4. Applies a sympathy nudge toward the leader scaled by `disinformation_susceptibility × leader_score`.
5. Adds a tension delta capped at `NARRATIVE_MAX_TENSION_DELTA = 0.02`.
6. Updates `non_kinetic.information_dominance` to the leading score (clamped to `[0, 1]`).

The phase short-circuits when the narrative store is empty (legacy scenarios pay zero overhead).

**Validation** rejects: empty `MediaEvent.narrative`; non-finite or out-of-range `credibility` / `reach`; unknown `favors` faction.

### Displacement store and `displacement_phase` (step 9)

`EventEffect::Displacement { region, magnitude }` adds `magnitude.clamp(0, 1)` displaced fraction to `SimulationState.displacement[region]`, clamping to `[0, 1]`. `total_inflow` accrues by the actually-applied delta.

`tick::displacement_phase` runs at step 9 after `narrative_phase`:
- Each region's pre-tick displaced fraction contributes `outflow = displaced × DISPLACEMENT_PROPAGATION_RATE` (10%/tick) split evenly across `Region.borders`.
- `absorbed = displaced × DISPLACEMENT_ABSORPTION_RATE` (5%/tick) merges back into the resident population.
- Receiving regions accumulate inflows in a separate `BTreeMap` before applying — single-pass, mirroring the network and supply phase conventions.
- Tension delta proportional to average displaced fraction, capped at `DISPLACEMENT_MAX_TENSION_DELTA = 0.005`.

`CivilianAction::Flee` (the existing population-segment action) also pushes displacement: a flee event with `rate = 0.10` spread across two concentrated regions adds 0.05 displaced to each. The segment `fraction` shrinking behavior is unchanged.

**Validation** rejects: unknown `Displacement.region`; non-finite, out-of-range, or zero `Displacement.magnitude`.

Per-run output: `RunResult.narrative_events` and `RunResult.displacement_reports`. Cross-run: `MonteCarloSummary.narrative_dynamics` and `MonteCarloSummary.displacement_summaries`. Both gate on per-mechanic data presence; legacy scenarios elide entirely.

Bundled archetype: `scenarios/narrative_competition_demo.toml`.

---

## Command effectiveness as a separate axis (R3-4)

Before R3-4, `LeadershipDecapitation` pushed degradation directly into `morale` via a per-tick clamp step, conflating chain-of-command efficiency (*capacity to direct*) with rank-and-file motivation (*will to fight*). R3-4 separates them.

**`RuntimeFactionState.command_effectiveness ∈ [0, 1]`** (default `1.0`) is the separate runtime field for chain-of-command capacity. Combat and AI threat-scoring read `morale × command_effectiveness` via the helper `tick::effective_combat_morale`.

**`tick::update_command_effectiveness`** (step 11, replacing the old `apply_leadership_caps`) writes the leadership-cadre degradation factor into `command_effectiveness` end-of-tick, after `campaign_phase`. When no faction declares a `leadership` cadre, the function short-circuits and every faction's `command_effectiveness` stays at `1.0` — preserving bit-identical output for all scenarios without leadership cadres.

Consequences:
- Morale stays untouched by the leadership writer. The `MoraleFloor` alliance-fracture condition no longer incidentally fires from a leadership strike — it fires only from the explicit `morale_shock` in the phase output and from political-phase / combat-loss morale drift.
- `ai::determine_weights` reads `effective_combat_morale` instead of raw morale, so a faction with intact rank-and-file morale but degraded command correctly shifts toward defensive posture.
- Future command-degrading effects (logistics strikes, command-jamming) can multiply directly into `command_effectiveness` without colliding with morale's other consumers.

**Snapshot exposure** — `FactionState.command_effectiveness` (in `crates/faultline-types/src/strategy.rs`) is part of every per-tick snapshot, with `#[serde(default = "default_command_effectiveness")]` defaulting to `1.0` so legacy snapshots deserialize unchanged.

Coverage: `crates/faultline-engine/tests/integration.rs` (updated Epic D round-one tests) plus `r3_4_decapitation_does_not_pollute_raw_morale` and `r3_4_no_cadre_legacy_path_leaves_morale_and_command_unchanged`.

---

## Multi-term utility and adaptive AI (Epic J rounds one and two)

Factions may declare a `[factions.<id>.utility]` block that re-weights AI action scoring along named analyst-facing axes. The utility surface composes *additively* on top of the existing doctrine-based scoring; scenarios without `[utility]` are bit-identical to the legacy path.

Round two (paired with the round-two belief model) closes the "score against believed state" item: when `simulation.belief_model.intelligence_weighting = true`, the evaluator consumes the belief-derived (uncertain) world view *and* discounts opponent-strength reads by detection confidence. See the belief-asymmetry section below for the integration.

### Types (`crates/faultline-types/src/faction.rs`)

- **`UtilityTerm`** — seven axes: `Control`, `CasualtiesSelf`, `CasualtiesInflicted`, `AttributionRisk`, `TimeToObjective`, `ResourceCost`, `ForceConcentration`. Derives `Hash + Ord` for deterministic `BTreeMap` keys. The wire-stable `as_key()` method (`"control"`, `"casualties_self"`, ...) keeps manifest hashes stable across binary representations.
- **`FactionUtility`** — `terms: BTreeMap<UtilityTerm, f64>` (base weights), `triggers: Vec<AdaptiveTrigger>` (adaptive adjustments), `time_horizon_ticks: Option<u32>` (per-faction deadline override).
- **`AdaptiveTrigger` / `AdaptiveCondition`** — seven condition variants: `MoraleBelow`, `MoraleAbove`, `TensionAbove`, `TickFraction`, `ResourcesBelow`, `StrengthLossFraction`, `AttributionAgainstSelf`. Each is a pure function of state + scenario; matched triggers compose multiplicatively against base term weights.

### Engine integration (`crates/faultline-engine/src/utility.rs`)

- `effective_weights(profile, faction, state, scenario, campaigns)` — evaluates all declared triggers against current state, returns per-term effective weights plus IDs of triggers that fired.
- `evaluate_action_utility(weights, faction, action, state, scenario, map)` — computes the per-action utility delta from the round-one heuristic table.

Both are pure functions — no RNG, no `HashMap`, `BTreeMap`-ordered iteration.

**AI integration** — `ai.rs::evaluate_actions` and `evaluate_actions_fog` apply the utility delta on top of doctrine scoring via `apply_utility_score`. The RNG draw in `evaluate_attack_actions` happens before utility scoring, so adding `[utility]` to a legacy scenario does not desync the RNG sequence.

**Decision-phase orchestration** (`tick.rs::decision_phase`, step 2) — captures per-term contributions across the top-3 selected actions into `state.utility_decisions: BTreeMap<FactionId, UtilityDecisionLog>`. This is the only mutation site for `utility_decisions`.

**Validation** rejects nine shapes: empty `[utility.terms]`; NaN/non-finite term weights; zero `time_horizon_ticks`; duplicate trigger ids; empty trigger adjustments; NaN/non-finite trigger multipliers; out-of-range/NaN `MoraleBelow` / `MoraleAbove` / `TensionAbove` thresholds; out-of-range `StrengthLossFraction` / `AttributionAgainstSelf`; negative/NaN `ResourcesBelow`; negative/NaN `TickFraction`.

Per-run output: `RunResult.utility_decisions`. Cross-run rollup and report section documented in `docs/analytics.md`.

Bundled archetype: `scenarios/adaptive_utility_demo.toml`.

---

## Belief asymmetry and deception (Epic M / J — rounds one and two)

The engine carries a persistent per-faction belief state separate from ground truth, with observation-driven refresh, per-tick decay, deception / intel-share event hooks, and AI consumption. Round two (Epic M / J) layers intelligence-weighted observation fidelity, asymmetric proximity-driven information, and belief-driven utility scoring on top — all opt-in so round-one scenarios are bit-identical.

### Types (`crates/faultline-types/src/belief.rs`)

- **`BeliefSource`** — `DirectObservation`, `Stale`, `Inferred`, `Deceived`. Tracks provenance so post-run analytics can distinguish "saw the truth", "saw it once and aged out", "estimated under uncertainty" (round two), "got planted by a deception event". Round two finally produces `Inferred`: any foreign belief touched under intelligence weighting is an estimate, not a perfect observation.
- **`BeliefScalar` / `BeliefForce` / `BeliefRegion`** — per-axis belief entries with `value`, `confidence ∈ [0, 1]`, `last_observed_tick`, and `source`. `BeliefForce` carries `region` + `estimated_strength`; `BeliefRegion` carries `controller` (option-typed).
- **`FactionBelief`** — `BTreeMap`-keyed maps for regions, forces, faction morale, faction resources. Plus `last_updated_tick` and `deception_events_received`.
- **`BeliefModelConfig`** — opt-in toggle (`enabled: bool` defaults `false`), per-axis decay rates (`force_decay_per_tick = 0.05`, `region_decay_per_tick = 0.02`, `scalar_decay_per_tick = 0.03`), `prune_threshold = 0.05`, optional `snapshot_interval`, and the round-two `intelligence_weighting: bool` / `believed_attribution: bool` (both default `false`, both requiring `enabled = true`). All fields `#[serde(default)]`. Config fields are validated unconditionally — even when `enabled = false` — so a typo in a disabled-but-authored config surfaces at load time.
- **`observation_confidence(intelligence)`** — maps `intelligence ∈ [0, 1]` to a foreign-observation confidence ceiling in `[0.3, 0.95]` (`0.3 + 0.65·intel`, clamped). Even a maximally-capable observer never reaches `1.0` confidence about an opponent's hidden state (irreducible fog of war).
- **`DeceptionPayload`** — four variants: `FalseForceStrength`, `FalseRegionControl`, `FalseFactionMorale`, `FalseFactionResources`.
- **`IntelligencePayload`** — four variants: `ForceObservation`, `RegionControl`, `FactionMorale`, `FactionResources`.

### EventEffect variants

- **`DeceptionOp { source_faction, target_faction, payload }`** — plants a `BeliefSource::Deceived`-tagged entry in the target's belief at confidence 1.0. Seamless from the AI's perspective (the world view it consumes cannot distinguish deception from direct observation), but the source tag persists through decay so terminal deceived beliefs are countable. Under round-two intelligence weighting, repeated direct observation blends the planted value *toward* the truth and re-tags the entry `Inferred` — "the lie is corrected by reality".
- **`IntelligenceShare { source_faction, target_faction, payload }`** — unilateral source→target transfer; lands as `DirectObservation` at confidence 1.0 (round-one) or intelligence-weighted (round-two), populated from the *current ground truth* of the referenced entity. Models alliance intel sharing, captured prisoners, third-party reporting.
- **`AmbientIntel { region }`** (round two) — radiates field intelligence about `region` to *every* faction with a force in or adjacent to it, at fidelity scaled by each listener's `intelligence`. Models an observable field event (a firefight, a moving column, a sensor trip) that anyone nearby learns about asymmetrically. A faction's belief about its own forces is left untouched.

All three are wired in `tick::apply_event_effects` and are no-ops when the belief model is disabled.

### `belief_phase` (step 15)

`crates/faultline-engine/src/belief.rs::belief_phase` runs at step 15, after `campaign_phase` (step 10), `update_command_effectiveness` (step 11), `fracture_phase` (step 12), network sample capture (step 13), and `update_region_control` (step 14). It operates in three steps:

1. **Decay** — every entry's confidence is reduced by the per-axis rate; non-`Deceived` entries are marked `Stale` (`Inferred` entries keep their tag through decay).
2. **Refresh** — every entry visible to the believer from current ground truth is refreshed. Round-one fidelity: reset to confidence 1.0 with `DirectObservation` source, clearing any prior `Deceived` tag. Round-two (see below): intelligence-weighted.
3. **Prune** — entries with confidence strictly below `prune_threshold` are removed.

Per-faction accuracy counters in `SimulationState.belief_counters` are updated in lock-step, including the round-two `force_confidence_sum` and `ambient_intel_received`.

### Round-two: intelligence-weighted fidelity

When `intelligence_weighting = true`, the refresh step (and `AmbientIntel`) route foreign observations through `refresh_force_belief` / `refresh_region_belief` with the observer's confidence ceiling `obs_conf = observation_confidence(intelligence)`:

- **Force strength** is a Kalman-style update toward the new observation, `value = prior + gain·(obs − prior)` with `gain = obs_conf`. A high-intelligence observer (`gain ≈ 0.88`) snaps its estimate to truth; a low-intelligence one (`gain ≈ 0.43`) moves only partway, so its belief *lags* a changing ground truth — making belief error scale with intelligence. The resulting confidence is exactly `obs_conf`: capability is a hard ceiling, so repeated observation never manufactures false `1.0` certainty.
- **Region control** adopts the observed controller at confidence `obs_conf` (control is more observable than strength).
- **Own-faction facts** stay perfect (confidence 1.0, `DirectObservation`) — a faction always knows its own posture.
- Any foreign entry touched under weighting is tagged `Inferred`; a prior `Deceived` value blends toward truth.

### AI consumption (Epic J)

`world_view_from_belief` constructs a `FactionWorldView` from a persistent `FactionBelief` so the existing `evaluate_actions_fog` evaluator can consume beliefs as if they were observations. Under round-two weighting the beliefs it carries are *uncertain* (capped confidence, lagging estimates), so the AI already scores against believed — not ground-truth — state (Epic J round-two item 1).

In addition, when `intelligence_weighting = true` the utility evaluator's `EffectiveWeights.confidence_weighted` flag is set, and `enemy_strength_in_region_fog` / `enemy_strength_in_adjacent_fog` discount each detection's `estimated_strength` by its `confidence`. A faction running the round-two model treats what it isn't sure of as a proportionally smaller threat — appropriately cautious decisioning under uncertainty.

`decision_phase` (step 2) consults `belief::belief_enabled(scenario)` to choose between:
1. Belief path (takes precedence when `enabled = true`).
2. Fog-of-war path (when `simulation.fog_of_war = true`).
3. Ground-truth path (default).

**Validation** rejects ten shapes: unknown source/target faction in `DeceptionOp` / `IntelligenceShare`; self-targeting; unknown region/faction references in payloads; empty force ID in payloads; out-of-range/NaN morale or resources in deception payloads; out-of-range/NaN/negative decay rates; out-of-range/NaN prune threshold; and (round two) an `AmbientIntel` referencing an unknown region.

### Believed-attribution rolls (round two)

When `simulation.belief_model.believed_attribution = true` (requires `enabled = true`), kill-chain attribution is routed through the defender's belief instead of ground truth. At the moment a defender (the chain's `target`) detects a phase, `campaign::draw_believed_attribution` draws a *believed attacker* from a weighted categorical distribution over every faction except the defender, in `BTreeMap` order:

- the true attacker carries `attribution_true_weight(defender.intelligence) ∈ [0.5, 0.98]` (a high-intelligence defender almost always attributes correctly);
- every other candidate carries a flat residual "confusion" mass summing to `1 - true_weight`;
- any candidate the defender holds a planted `BeliefSource::Deceived` belief about (a false-flag region-control belief naming it, or a deceived force belief it owns — collected by `collect_deceived_implications`) gets an additional `DECEPTION_ATTRIBUTION_WEIGHT = 3.0` mass.

The chosen faction is stored on `CampaignState::attributed_faction`. `fracture::mean_attribution` then credits each chain's `attribution_confidence` to its *effective attributed faction* (`attributed_faction` when set, else the true `attacker`), so an `AttributionThreshold { attacker }` fracture rule fires against whoever the defender *believes* did it — a misattribution can break an alliance against an innocent ally.

**Determinism** — the draw consumes exactly one `ChaCha8Rng` value per detection and is gated entirely behind the sub-flag, so a scenario that leaves `believed_attribution = false` consumes the RNG in the exact legacy order and is bit-identical to the pre-feature engine; `attributed_faction` stays `None` and `mean_attribution` falls back to the true attacker. The draw itself is the only randomness — the weight computation is a pure function of belief state + intelligence.

**Validation** rejects `believed_attribution = true` without `enabled = true`, and `believed_attribution = true` with no kill chains (a silent no-op — no detection ever fires a roll). An `AttributionThreshold` rule naming a faction that owns no kill chain is rejected under the legacy path (it could never fire) but *accepted* under believed-attribution, where misattributing an attack to a non-attacker is the intended shape.

**Backward-compat** — `None` or `enabled = false` means the belief phase short-circuits in O(1); `intelligence_weighting = false` keeps the round-one perfect-observation path bit-identical; `believed_attribution = false` keeps the legacy ground-truth attribution path bit-identical. `AmbientIntel` is a no-op when belief mode is off.

Per-run output: `RunResult.belief_accuracy` (now carrying `force_confidence_sum`, `ambient_intel_received`, `inferred_beliefs_terminal`), `RunResult.belief_snapshots`, and `RunResult.attribution_events` (the believed-attribution roll log). Cross-run rollups (`belief_summaries`, `misattribution_summary`) and report sections documented in `docs/analytics.md`.

Bundled archetypes: `scenarios/false_flag_demo.toml` (round one — Alpha plants a phantom 500-strength force in Bravo's belief; mid-run `IntelligenceShare::FactionResources` refreshes Bravo's resource belief from ground truth), `scenarios/recon_fidelity_demo.toml` (round two — a high-intelligence vs low-intelligence pair under `intelligence_weighting` with `AmbientIntel` radiation over two contested regions; the report's belief-fidelity sub-section contrasts the two), and `scenarios/misattribution_demo.toml` (round two — a low-intelligence defender, framed by a red false flag, misattributes red's covert chain to an innocent ally ~40% of the time and fractures the alliance against it).
