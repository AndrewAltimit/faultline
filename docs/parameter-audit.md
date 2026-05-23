# Parameter Audit (R3-2)

The parameter audit (R3-2) is a systematic effort to find scenario fields that were
authored across dozens of bundled scenarios with non-trivial values but had zero effect
on simulation output. A field that is declared in TOML, validated as a number in range,
and then never read by the engine is a correctness problem: the analyst who set
`mobility = 0.3` or `deployment_cost = 50` is making a modelling claim that the engine
quietly ignores. Each round of R3-2 identifies the highest-leverage silent fields, wires
them into the engine hot path, adds load-time validation rejections for degenerate shapes,
and pins the new behavior with regression tests in
`crates/faultline-engine/tests/audit_unread_params.rs`.

The priority order for the round-two items is documented in
`docs/improvement-plan.md` under "R3-2 round two: unread-parameter audit follow-up".

---

## Round one

**Three previously-silent fields now affect simulation outcomes; each was authored in
dozens of bundled scenarios but had zero engine effect.**

### `Faction.command_resilience`

- Type: `f64 ∈ [0, 1]`
- Formula: `effective_shock = morale_shock × (1 − resilience)`
- Hook point: `campaign::apply_leadership_decapitation`
- Semantics: attenuates the one-shot morale drop applied when a
  `PhaseOutput::LeadershipDecapitation` fires against this faction. A faction with
  `command_resilience = 0.5` absorbs only half the authored `morale_shock`.
- No-op condition: factions without a `[leadership]` cadre are unaffected (no
  decapitation can fire against them).

### `ForceUnit.morale_modifier`

- Type: `f64` (no explicit range cap; floored at `0` in the engine)
- Formula: `effective_contribution = base_strength × (1.0 + morale_modifier)`
- Hook point: `tick::find_contested_regions`
- Semantics: multiplies the unit's effective combat contribution by
  `(1.0 + morale_modifier)`. Values above `0` improve performance; values below `0`
  degrade it. The floor at `0` prevents a pathological override below `-1.0` from
  inverting the combat math.

### `Scenario.defender_budget`

- Type: `Option<f64>` (symmetric mirror of `attacker_budget`)
- Semantics: reactive budget cap. Once cumulative `defender_spend` exceeds the cap,
  `SimulationState.defender_over_budget_tick` latches sticky and a
  `DEFENDER_OVER_BUDGET_DETECTION_FACTOR` = 0.5× detection-probability multiplier
  applies to all subsequent kill-chain phase rolls.
- Latch timing: latched at tick-start so chain-processing order can never affect
  which phase first incurs the penalty.
- Hook point: `tick.rs` kill-chain phase loop (reads `defender_over_budget_tick` latch).

### Regression coverage

`crates/faultline-engine/tests/audit_unread_params.rs` — 10 tests, including a
32-seed statistical regression for the defender-budget detection penalty.

---

## Round two — movement rate

**Three movement-related fields that were silent in round one now compose into a single
per-tick "effective mobility" gate.**

Primary wiring: `crates/faultline-engine/src/tick.rs` — `movement_phase` and
`environment_movement_factor`. A new runtime field `ForceUnit.move_progress` was added
(`#[serde(default)]`, so legacy TOML loads unchanged).

### Fields wired

#### `ForceUnit.mobility`

- Type: `f64` (must be finite and non-negative; validated at load time)
- Semantics: per-unit movement rate. Default `1.0` preserves the legacy "unit moves
  every tick when queued" behavior.

#### `TerrainModifier.movement_modifier`

- Type: `f64` (must be finite and non-negative; validated at load time)
- Semantics: per-region movement attenuator. Read from the unit's *source* region —
  a unit moving out of a 0.5-modifier region is gated by 0.5 regardless of destination.
  Negative values would silently invert the gate; NaN would propagate via
  `(1.0 + NaN).max(0.0) → 0.0` freezing the unit permanently — both shapes are
  rejected at load time.

#### `EnvironmentWindow.movement_factor`

- Type: `f64` (non-finite already rejected by `validate_environment_window` from
  Epic D round-one)
- Semantics: globally-scoped weather / time-of-day attenuator on top of the per-region
  modifier. Composes via `tick::environment_movement_factor` — multiplicative over every
  active window covering the source-region terrain.

### Effective mobility formula

```
effective_mobility = (mobility × terrain_modifier × env_factor).max(0.0)
```

This value is added to `move_progress` each tick, capped at `1.0`. The queued `MoveUnit`
action fires only once `move_progress >= 1.0`, at which point exactly `1.0` is consumed.

- A unit with `mobility = 0.5` takes two attempts to move.
- A unit with `mobility = 2.0` still moves every tick — the cap prevents saved-up moves.
- Default authoring (`mobility = 1.0`, terrain modifier `1.0`, no env windows) reproduces
  the previous "unit moves every tick when queued" behavior exactly.

### Validation rejections

- Non-finite or negative `ForceUnit.mobility`
- Non-finite or negative `TerrainModifier.movement_modifier`
- Non-finite `EnvironmentWindow.movement_factor` (already covered from Epic D round-one)

### Regression coverage

`crates/faultline-engine/tests/audit_unread_params.rs` gains 10 tests pinning:
rate-gate semantics, multiplicative composition, cap behavior, and the three validation
rejections. The integration-test fixture in
`crates/faultline-engine/tests/integration.rs::base_scenario` was tightened to use
uniform `movement_modifier = 1.0` so combat / tech tests are not accidentally exercising
the new gate.

---

## Round two — population-segment activation

**Closes the "half-built" caveat on `[political_climate.population_segments]`. Three
previously-silent `MediaLandscape` fields are now load-bearing on the political /
information phases, and every civilian-segment activation is now tracked, aggregated
across runs, and surfaced in the post-run report.**

Note: the other `PopulationSegment` fields listed in the original round-two plan
(`activation_threshold`, `activation_actions`, `volatility`) turned out to already be
wired in the latch and post-activation processor. The gap was the missing *reporting*
layer, not the activation logic itself.

### Fields wired

#### `MediaLandscape.fragmentation`

- Type: `f64 ∈ [0, 1]`; validated at load time alongside the legacy fields.
- Semantics in `faultline_politics::update_civilian_segments`:
  amplifies sympathy noise via `noise_amp = 1.0 + 0.5 * fragmentation + 0.5 * effective_social_media`
  and dampens tension pull via `tension_scale = 1.0 - fragmentation`.
- Semantics in `tick::information_phase`: high fragmentation × high effective social
  media amplify the disinfo→tension delta by up to 2× when both are at `1.0`.

#### `MediaLandscape.social_media_penetration`

- Type: `f64 ∈ [0, 1]`; validated at load time.
- Semantics: multiplied by `internet_availability` to give `effective_social_media`
  before being used in the noise and disinfo amplifiers.

#### `MediaLandscape.internet_availability`

- Type: `f64 ∈ [0, 1]`; validated at load time.
- Semantics: the "lights out" guard.
  `effective_social_media = social_media_penetration × internet_availability` — if
  internet is offline (`internet_availability = 0`), social-media penetration alone has
  no amplification effect.

### Activation tracking

- `SimulationState.civilian_activations` — per-run emission-ordered log of every segment
  activation.
- `RunResult.civilian_activations` — surfaces the log post-run.
- `MonteCarloSummary.civilian_activation_summaries` — cross-run rollup in
  `crates/faultline-stats/src/civilian_activations.rs`: activation rate, mean activation
  tick, per-action firing counts, modal favored faction (`BTreeMap`-order tie-break).
- `CivilianActivationEvent` carries action discriminants as `Vec<String>` (not the typed
  enum) so cross-run aggregation can count action firings without coupling the manifest
  schema to the typed payload.
- `tick::civilian_action_kind` is the canonical discriminant mapping; the function is
  exhaustive on `CivilianAction` so adding a new variant forces a deliberate decision
  here at compile time.

### Report

New `## Civilian Activations` section in
`crates/faultline-stats/src/report/civilian_activations.rs`. Elides when
`summary.civilian_activation_summaries` is empty. A scenario that declared segments
but produced zero activations across the run set still emits — the analyst sees
"segment declared, never tripped" rather than an unexplained absence. The report-render
gate in `faultline-cli/src/main.rs` was extended so scenarios that only have civilian
segments (no kill chains, no networks) get a `report.md` written.

### Validation rejections

- Out-of-range or non-finite `MediaLandscape.*` fields (all six: the three new ones plus
  the legacy `disinformation_susceptibility` and `state_control` are now validated
  together)
- Duplicate segment ids
- Segments concentrated in unknown regions
- `volatility` / `activation_threshold` / `fraction` / sympathy values out of range or
  non-finite

### Regression coverage

`crates/faultline-engine/tests/audit_unread_params.rs` gains 7 tests pinning:
(a) end-to-end activation event capture with action-kind ordering,
(b) fragmentation amplifies drift,
(c) `internet = 0` zeroes social-media amplification (the lights-out guard),
(d) determinism across same-seed runs, plus the four validation rejections.
The cross-run aggregator and the report section have their own unit tests in their
respective modules.

---

## Round two — tech-card costs

**Closes the "tech is free, instant, and unbounded" caveat for `[technology.<id>]`
entries. Three previously-silent `TechCard` fields are now load-bearing across the
engine. Authored in dozens of bundled scenarios with non-trivial values; until this
round, every value was inert.**

### Fields wired

#### `TechCard.deployment_cost`

- Hook point: `crates/faultline-engine/src/engine.rs::initialize_state`
- Semantics: deducted at engine init from the faction's `initial_resources`. The init
  loop walks `Faction.tech_access` in declaration order: each card whose
  `deployment_cost <= resources` is deployed and the cost subtracted; cards whose cost
  exceeds what is left are *denied* (skipped, not added to `tech_deployed`) and recorded
  for reporting. Iteration continues past a denial — a denied big-ticket card does not
  prevent a later cheaper card from fitting. Cards referenced in `tech_access` but absent
  from `scenario.technology` are deployed at zero cost, preserving the legacy
  "missing tech is a silent no-op at combat time" contract.

#### `TechCard.cost_per_tick`

- Hook point: `crates/faultline-engine/src/tick.rs::attrition_phase`
- Semantics: deducted per-tech after income (with supply-pressure attenuation) and
  upkeep have settled. Each card whose maintenance cost exceeds the faction's current
  resources is *decommissioned* — removed from `tech_deployed` for the rest of the run,
  no further charges, no refund. Decommissioning is final: the card does not re-deploy
  if resources later recover. Iteration is in `tech_deployed` declaration order
  (deterministic).
- Observable effect: `tutorial_asymmetric.toml` hits a 100% decommission rate against
  both factions because the `cost_per_tick` for `surveillance_drone` (3.0) and
  `concealment_network` (1.0) outpaces the factions' modest `resource_rate` after
  upkeep. This is a legitimate diagnostic the audit was designed to surface.

#### `TechCard.coverage_limit`

- Hook point: `crates/faultline-engine/src/tick.rs::combat_phase` and
  `compute_tech_combat_modifier`
- Semantics: when `Some(n)`, caps the per-tick number of (region, opponent) pairs the
  card contributes to during combat. `compute_tech_combat_modifier` reads the per-faction
  `tech_coverage_used` counter (cleared at the top of `combat_phase`) and skips a card
  whose count has reached the limit. Cards without a `coverage_limit` (the legacy default
  `None`) bypass the gate entirely, so legacy scenarios pay zero bookkeeping overhead.
  The gate's iteration order — `BTreeMap` over contested regions, then over factions in
  each region — is deterministic, so which (region, opponent) pairs receive the benefit
  when supply is constrained is reproducible across runs.

### Per-run output

`RunResult.tech_costs` (`BTreeMap<FactionId, TechCostReport>`) records per-faction
deployed / denied / decommissioned card lists plus total deployment and maintenance
spend. The map elides factions whose tech roster never engaged the cost mechanic
(zero-cost cards, no denials, no decommissions), so legacy scenarios with all-zero tech
costs see no change in `RunResult` shape.

### Cross-run rollup

`MonteCarloSummary.tech_cost_summaries` (`BTreeMap<FactionId, TechCostSummary>`).
Per-faction mean deployment / maintenance / total spend, plus `runs_with_denial` and
`runs_with_decommission` (count-style diagnostics so the report renders both the
proportion and the underlying sample size). Producer:
`compute_tech_cost_summaries` in `crates/faultline-stats/src/lib.rs`.

### Report

New `## Tech-Card Costs` section in `crates/faultline-stats/src/report/tech_costs.rs`.
Elides when `summary.tech_cost_summaries` is empty.

### Validation rejections

- Non-finite or negative `deployment_cost`
- Non-finite or negative `cost_per_tick`
- `coverage_limit = Some(0)` — the gate's `used >= limit` check is true on the first
  attempt, so the card never contributes

### Regression coverage

`crates/faultline-engine/tests/audit_unread_params.rs::tech_costs` pins:
(a) deployment cost deduction, (b) deployment denial when unaffordable,
(c) iteration-past-denial, (d) `cost_per_tick` deduction, (e) decommission on
unaffordable maintenance, (f) coverage uncapped → no tracking, (g) `coverage_limit = 1`
caps, (h) `coverage_limit > demand` still tracks actual usage, (i) determinism across
same-seed runs, (j) report emission gate, (k) zero-cost roster elides, plus the three
validation rejections.

---

## Status: closed vs. deferred

All items are drawn from the R3-2 priority list in `docs/improvement-plan.md`.

### Closed

| Item | Field(s) | How closed |
|---|---|---|
| R3-2 round one | `Faction.command_resilience`, `ForceUnit.morale_modifier`, `Scenario.defender_budget` | Wired in engine; regression suite added. |
| R3-2 round two item 1 | `ForceUnit.mobility`, `TerrainModifier.movement_modifier`, `EnvironmentWindow.movement_factor` | Per-tick `move_progress` accumulator gate. `ForceUnit.upkeep` turned out to already be wired (`tick::attrition_phase` already summed it); the original audit entry was incorrect. |
| R3-2 round two item 2 | `Faction.diplomacy` | Closed by Epic D round-three item 1 (combat + AI behavioral coupling for `Allied` and `Cooperative` stance). Political-phase and victory-check coupling remain deferred. |
| R3-2 round two item 3 | `MediaLandscape.fragmentation`, `MediaLandscape.social_media_penetration`, `MediaLandscape.internet_availability` | Wired into `update_civilian_segments` and `information_phase`; activation tracking + report section added. `PopulationSegment.activation_threshold` / `activation_actions` / `volatility` turned out to already be wired; only the reporting layer was missing. |
| R3-2 round two item 4 | `TechCard.deployment_cost`, `TechCard.cost_per_tick`, `TechCard.coverage_limit` | Wired at engine init, attrition phase, and combat phase; per-faction cost report + report section added. |
| R3-2 round two item 5 | `Region.centroid`, `Faction.color` | Resolved as documentation-only: both fields now carry explicit doc comments identifying them as visualization-only metadata with no engine effect. Engine validation deliberately does not constrain their format. No code changes needed. |

### Deferred

| Item | Field | Notes |
|---|---|---|
| R3-2 round two item 6 | `ForceUnit.force_projection` | Declared but zero scenarios set it. Drop-or-wire decision; lean toward drop unless a future epic calls for it. See `docs/improvement-plan.md`. |
| Epic D follow-up | `Faction.diplomacy` (political phase + victory-check) | The combat and AI coupling is closed (Epic D r3 item 1). Victory-check and political-phase coupling are explicitly deferred pending a use case. |
