# CLAUDE.md

This file provides guidance to AI coding agents working with code in this repository.

## Project Overview

Faultline is an analytical research tool for conflict simulation. It takes TOML scenario configurations and runs deterministic Monte Carlo simulations producing probability distributions of outcomes. Primary targets: WASM (browser) and native CLI.

All scenario data must be derived from publicly available open-source intelligence (OSINT). See [LEGAL.md](LEGAL.md) for sourcing requirements and export control analysis.

All code is authored by AI agents under human direction. No external contributions are accepted (see `CONTRIBUTING.md`).

## Build and Test Commands

This is a Cargo workspace. All CI runs containerized via Docker but the commands work locally:

```bash
# Format check
cargo fmt --all -- --check

# Lint (warnings are errors in CI)
cargo clippy --all-targets -- -D warnings

# Run all tests
cargo test

# Run a single crate's tests
cargo test -p faultline-types

# Run a specific test by name
cargo test -p faultline-engine -- combat_lanchester

# Build release
cargo build --release

# License and advisory audit
cargo deny check

# Run a single simulation
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --single-run

# Run Monte Carlo batch
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000

# Network resilience archetype (Epic L) — supply + comms graphs under
# scripted interdiction. The report's "Network Resilience" section shows
# per-network mean/max disrupted-node and component counts plus the
# Brandes critical-node ranking on the static topology.
cargo run -p faultline-cli -- scenarios/network_resilience_demo.toml -n 16

# Supply-network interdiction archetype (Epic D round-three item 2).
# A Blue defender owns two `kind = "supply"` networks. A scripted
# attacker chains three interdiction events that progressively cut
# Blue's residual supply capacity. The report's "Supply Pressure"
# section quantifies the resulting per-tick income attenuation —
# pressure = residual / baseline, multiplied into resource_rate
# every attrition tick.
cargo run -p faultline-cli -- scenarios/supply_interdiction_demo.toml -n 16

# Multi-front resource contention archetype (Epic D round-three item 3).
# A 3-tier SOC defender (tier-1 triage → tier-2 IR → tier-3 forensics)
# with declared cross-role escalation policy. Tier-1 saturates first,
# spills to tier-2 at 80% capacity; tier-2 saturates next, spills to
# tier-3 at 70%; tier-3 (terminal) absorbs the residual. The report's
# "Defender Capacity" section gains a "Cross-role escalation" sub-table
# whose `In` / `Out` columns trace the spillover chain by inspection.
cargo run -p faultline-cli -- scenarios/multifront_soc_escalation.toml -n 16

# Calibration scaffold demo (Epic N — calibration discipline). The
# scenario declares a `[meta.historical_analogue]` block with three
# observations (Winner, WinRate, DurationTicks); the report's
# `## Calibration` section computes a per-observation verdict
# (Pass/Marginal/Fail) plus a roll-up. Scenarios without a declared
# analogue render a "purely synthetic" disclaimer in the same section.
cargo run -p faultline-cli -- scenarios/calibration_demo.toml -n 100

# Narrative competition + displacement flows (Epic D round-three item
# 4). Two-region archetype with three factions: Red and Blue push
# competing `MediaEvent` narratives (Red reinforces twice, Blue once);
# a scripted `Displacement` event seeds 30% displaced fraction in
# `frontier_north` that propagates to `frontier_south` over the run;
# a population segment's `Flee` action adds organic displacement once
# its sympathy crosses the activation threshold. The report's
# `## Narrative Dynamics` section ranks per-faction information
# dominance and per-narrative trajectory (firing rate, peak strength,
# modal favored faction); the `## Displacement Flows` section
# captures peak / mean / inflow / outflow per region.
cargo run -p faultline-cli -- scenarios/narrative_competition_demo.toml -n 16

# Counterfactual override + delta report (Epic B)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --counterfactual "faction.alpha.initial_morale=0.3"

# Side-by-side comparison of two scenarios (Epic B)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --compare scenarios/tutorial_asymmetric.toml

# Strategy search over a scenario's [strategy_space] (Epic H)
cargo run -p faultline-cli -- scenarios/strategy_search_demo.toml \
    --search --search-trials 16 --search-runs 50 \
    --search-method grid \
    --search-objective maximize_win_rate:alpha \
    --search-objective minimize_duration

# Defender-posture optimization (Epic I) — same --search command,
# different objective set; the report's Counter-Recommendation section
# ranks Pareto-frontier postures against the do-nothing baseline.
cargo run -p faultline-cli -- scenarios/defender_posture_optimization.toml \
    --search --search-trials 8 --search-runs 30 \
    --search-method grid \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    --search-objective maximize_detection

# Defender-posture robustness analysis (Epic I — round two). Evaluates
# every defender posture against every attacker profile declared in
# `[strategy_space.attacker_profiles]` and ranks postures by worst-case
# profile. Either feed in a saved `search.json` (full pipeline) or omit
# `--robustness-from-search` to evaluate the natural-state baseline.
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --search --search-method grid --search-trials 8 --search-runs 16 \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    -o ./output/search_phase
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --robustness \
    --robustness-from-search ./output/search_phase/search.json \
    --robustness-runs 16 \
    --robustness-objective "maximize_win_rate:blue" \
    --robustness-objective minimize_max_chain_success \
    -o ./output/robustness_phase

# Adversarial co-evolution between an attacker and defender
# (Epic H — round two). Both sides must own at least one
# `[strategy_space]` variable via the `owner = "<faction>"` tag.
# `--coevolve-method grid` enumerates each side's full sub-space per
# round; the loop terminates when the joint state stabilises across
# two consecutive rounds (Nash equilibrium), when a cycle of any
# period >= 2 is detected, or at `--coevolve-rounds`.
cargo run -p faultline-cli -- scenarios/coevolution_demo.toml --coevolve \
    --coevolve-attacker red --coevolve-defender blue \
    --coevolve-attacker-objective "maximize_win_rate:red" \
    --coevolve-defender-objective minimize_max_chain_success \
    --coevolve-method grid \
    --coevolve-trials 4 --coevolve-runs 10 \
    --coevolve-rounds 6 --coevolve-seed 1

# Coalition fracture demo (Epic D — round two). The scenario declares
# two alliance_fracture rules on a Cooperative `gray_partner` faction:
# one trips on attribution accumulation against `red_attacker`'s
# kill chain, the other on political tension. The report's
# `## Alliance Dynamics` section ranks per-rule fire rate, mean fire
# tick, and terminal-stance distribution across runs.
#
# Note: as of Epic D round-three item 1 the post-fracture stance is
# now consumed by combat targeting and AI decision-making (see the
# "Diplomatic stance behavioral coupling" section below). The
# victory-check phase still ignores diplomacy.
cargo run -p faultline-cli -- scenarios/coalition_fracture_demo.toml -n 32

# Replay a saved manifest and assert bit-identical output (Epic Q)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml \
    --verify ./output/manifest.json

# Migrate a scenario forward to the current schema version (Epic O)
# Prints to stdout by default; --in-place rewrites the source file.
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate --in-place

# Explain a scenario without running the engine (Epic P sub-item).
# Pure schema view — surfaces factions, kill chains, victory
# conditions, the [strategy_space] decision-variable surface, and any
# author-flagged Low-confidence parameters. Markdown to stdout by
# default; --explain-format json emits the structured ExplainReport.
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --explain
cargo run -p faultline-cli -- scenarios/strategy_search_demo.toml \
    --explain --explain-format json

# Build WASM
wasm-pack build crates/faultline-backend-wasm --target web --out-dir ../../site/pkg --no-typescript

# Run frontend JS unit tests (Node 22+; uses node:test, no install required)
node --test tests/integration/*.test.mjs
```

## Analytics surfaced in `report.md` (Epic C)

Beyond the win-rate / feasibility / kill-chain tables that earlier
epics shipped, every Monte Carlo run now also emits:

- **Time & Attribution Dynamics** — per-chain time-to-first-detection
  (right-censored when never detected), defender-reaction-time
  distribution (gap from first detection to run end), and per-phase
  Kaplan-Meier survival curves with cumulative hazard. Sections elide
  when the chain produces no signal.
- **Pareto Frontier** — non-dominated runs across (attacker cost,
  success, stealth = `1 - max chain detection`). Surfaces the
  achievable trade-off envelope before reaching for a sweep.
- **Output Correlation Matrix** — Pearson correlations across the
  six built-in per-run scalars (duration, casualties, attacker /
  defender spend, mean attribution, max detection). Constant series
  show as `—` (correlation undefined; deliberately not zero).

The schema for all five outputs lives on `MonteCarloSummary` /
`CampaignSummary` in `crates/faultline-types/src/stats.rs`. The
producers are pure functions of `RunResult` data and live in
`crates/faultline-stats/src/time_dynamics.rs` — they never re-run
the engine. Morris elementary-effects screening (the
variance-decomposition replacement for pure OAT sensitivity sweeps)
lives in `crates/faultline-stats/src/morris.rs`; not currently CLI-
exposed but callable from library consumers.

`BranchCondition::EscalationThreshold` (Epic C) adds hysteresis to
phase branching — a branch that only fires when a global metric has
stayed on the requested side of a threshold for `sustained_ticks`
consecutive end-of-tick snapshots. The engine sizes its rolling
metric-history buffer to the longest window any branch in the
scenario asks for; legacy scenarios with no such branch pay zero
overhead. Schema reference is in `docs/scenario_schema.md` under
`PhaseBranch`.

**Network primitives (Epic L).** Scenarios may declare any number of
typed graphs via `[networks.<id>]` (see `docs/scenario_schema.md`).
Each network has nodes and directed weighted edges with per-edge
metadata (`capacity`, `latency`, `bandwidth`, `trust`). Three new
`EventEffect` variants — `NetworkEdgeCapacity`, `NetworkNodeDisrupt`,
`NetworkInfiltrate` — drive runtime mutation of the per-network
state stored on `SimulationState.network_states`. `NetworkEdgeCapacity`
composes multiplicatively with prior events and is clamped to
`[0, 4]` so a runaway author chain can't poison the residual-capacity
series. Per tick (after the campaign and leadership-cap phases) the
engine appends one `NetworkSample` per declared network — component
count, largest-component size, residual capacity, disrupted-node
count. Cross-run analytics in `faultline_stats::network_metrics`
roll those into `MonteCarloSummary.network_summaries`: mean / max
disrupted-node and component counts, fragmentation rate, plus a top-N
**critical-node ranking** by Brandes betweenness centrality on the
static topology (treating the graph as undirected for centrality —
removing the most-central node is what hurts most regardless of who
removes it). The same module also exposes `max_flow` (Edmonds-Karp,
deterministic via `BTreeMap` BFS ordering) and a `mean_infiltration_per_faction`
helper. Validation rejects edges with unknown endpoints, self-loops,
and event effects targeting unknown networks / nodes / factions.
Engine path is zero-overhead for scenarios without `[networks.*]`.
Bundled archetype: `scenarios/network_resilience_demo.toml`.

**Defender capacity model (Epic K).** Factions can declare per-role
investigative queues via `[factions.<id>.defender_capacities.<role>]`
(see `docs/scenario_schema.md`). Kill-chain phases hook in via
`defender_noise` (Poisson-sampled per-tick alerts pushed onto a
named role's queue) and `gated_by_defender` (multiplies that phase's
per-tick detection roll by the role's `saturated_detection_factor`
when the queue is at capacity). Per-tick order in
`crates/faultline-engine/src/campaign.rs::campaign_phase` is
**arrive → assess → service**: a phase enqueues its noise, the
detection roll reads the post-arrival depth, and the queue is
serviced at end-of-tick — that ordering reproduces the alert-fatigue
effect when a sequential phase 2 inherits the backlog phase 1
created. Output lives on `RunResult.defender_queue_reports` per run
and aggregates to `MonteCarloSummary.defender_capacity` (mean
utilization, time-to-saturation, mean shadow detections); both
elide entirely when no faction declares queues. Bundled archetype:
`scenarios/alert_fatigue_soc.toml`.

**Strategy search (Epic H — round one).** Scenarios may opt into a
`[strategy_space]` block declaring decision variables (continuous or
discrete) and search objectives. The `--search` CLI mode samples
assignments via `random` or `grid` methods, evaluates each via Monte
Carlo, and reports best-by-objective plus the non-dominated Pareto
frontier. Search uses its own seed (`--search-seed`) independent of
the inner MC seed (`--seed`) so search-then-evaluate is bit-identical
and trial-to-trial deltas isolate parameter effects from sampling
noise. Round-one objectives are derived from existing
`MonteCarloSummary` / `CampaignSummary` shape — no new analytics
modules. Manifests record objective *labels* (not the structured
enum) so adding new variants stays additive. Adversarial co-evolution
is deferred to a follow-up round. See
`crates/faultline-types/src/strategy_space.rs`,
`crates/faultline-stats/src/search.rs`,
`scenarios/strategy_search_demo.toml`, and the
`[strategy_space]` reference in `docs/scenario_schema.md`.

**Defender-posture optimization (Epic I — round one).** Builds on
Epic H. Four defender-aligned `SearchObjective` variants
(`MaximizeAttackerCost`, `MaximizeDetection`, `MinimizeDefenderCost`,
`MinimizeMaxChainSuccess`) compose with the existing attacker-aligned
set so a single `[strategy_space]` declaration can express either
side's optimization. The `set_param` path layer is extended to reach
`faction.<id>.force.<force_id>.{strength,mobility,upkeep}` so force
posture is a decision variable. Search runs now compute an optional
"do-nothing" baseline trial alongside sampled trials (toggle via
`SearchConfig.compute_baseline`, default true in the CLI). The new
**Counter-Recommendation** report section ranks Pareto-frontier trials
by per-objective improvement against the baseline with direction-aware
"improvement?" tags and Wilson 95% CIs on rate-valued win-rate
objectives; section gates on baseline + at least one decision variable
with `owner` set, so legacy attacker-only spaces stay unchanged.
`ManifestMode::Search` records `compute_baseline` so verify replays
match. Bundled archetype: `scenarios/defender_posture_optimization.toml`.

**Defender-posture robustness (Epic I — round two).** Closes the
deferred robustness-analysis item from Epic I round-one by adding
a `--robustness` CLI mode and a new
`faultline_stats::robustness::run_robustness` runner. Given a set of
defender postures (typically the Pareto frontier of a prior `--search`)
and a library of named attacker profiles declared in
`[strategy_space.attacker_profiles]`, the runner evaluates every
(posture × profile) cell via Monte Carlo and surfaces per-posture
worst / best / mean / stdev rollups across profiles. The expected
analyst flow is search → robustness: first identify Pareto-optimal
postures against a single attacker baseline, then re-rank them by
worst-case profile to surface which postures are fragile to which
attacker strategies. Worst/best are direction-aware on the objective:
for a `MinimizeMaxChainSuccess` objective, "worst" is the largest cell
value (chain succeeds most often). The runner has no RNG of its own —
the cross-product is iterated deterministically and every cell reuses
the same inner MC seed, so cell-to-cell deltas reflect parameter
changes only. `ManifestMode::Robustness` records the full posture list
inline plus the SHA-256 of the source `search.json` (when one was
provided) so `--verify` refuses a stale source file. Bundled archetype:
`scenarios/defender_robustness_demo.toml`.

**Adversarial co-evolution (Epic H — round two).** Closes the deferred
adversarial-co-evolution item from Epic H by layering an alternating
best-response loop on top of `run_search`. Each round, one side
("mover") re-optimizes only the variables it owns against the
opponent's currently-frozen assignment via a sub-search. The loop
terminates when (a) the joint `(attacker, defender)` state matches the
prior round (Nash equilibrium in pure strategies on the discrete
strategy space the search visits), (b) a cycle of any period >= 2 is
detected (joint state repeats with the detected period; the reported
`period` is the shortest matching distance ≥ 2), or (c)
`--coevolve-rounds` is reached (`NoEquilibrium`). All `[strategy_space]` variables must declare
`owner = "<faction>"` matching either `--coevolve-attacker` or
`--coevolve-defender`; un-owned or mis-owned variables are rejected at
validation. Determinism is triple-seeded: `coevolve_seed` drives
per-round sub-search sampling via
`coevolve_seed.wrapping_add(round_index)`, the inner MC seed is
identical across rounds and across trials so trial-to-trial deltas are
pure parameter-change effects, and the per-round `SearchConfig` is
derived deterministically from the coevolve seed. `ManifestMode::Coevolve`
records all per-side knobs so `--verify` replays bit-identical, and a
`COEVOLVE <status> rounds=N manifest_hash=...` line is printed on
stdout for CI scripts. Bundled archetype: `scenarios/coevolution_demo.toml`.
The implementation lives in `crates/faultline-stats/src/coevolve.rs`;
the report renderer (`render_coevolve_markdown`) lives in
`crates/faultline-stats/src/report/coevolve.rs` (the report module is
decomposed by section — see "Report module layout" below).

**Engine model depth (Epic D — round one).** Three additions
expand authoring expressiveness without touching the determinism
contract; all are `#[serde(default)]` so legacy scenarios load
unchanged.

- `BranchCondition::OrAny { conditions }` composes inner conditions
  with short-circuit OR semantics. `max_escalation_window`
  recurses through it so an `EscalationThreshold` nested in an OR
  still registers its history requirement. Empty `conditions` is
  rejected at validation.
- Optional global `[[environment.windows]]` schedule with `Always`
  / `TickRange` / `Cycle` activation. Per-terrain `defense_factor`
  multiplies into combat `terrain_defense`; global `detection_factor`
  multiplies into every kill-chain phase's per-tick detection
  probability *before* saturation gating, naturally narrowing the
  shadow-detection window between unattenuated and saturated rolls.
  See `crates/faultline-engine/src/tick.rs::environment_detection_factor`
  and `environment_defense_factor`.
- Optional `[factions.<id>.leadership]` cadre with named ranks plus
  `succession_recovery_ticks` / `succession_floor`. The
  `PhaseOutput::LeadershipDecapitation { target_faction, morale_shock }`
  variant advances the rank index, applies a one-shot morale drop,
  and records the strike tick. A new per-tick step
  (`tick::apply_leadership_caps`) clamps each faction's morale at
  `current_rank.effectiveness × recovery_ramp` so combat reads the
  degraded value directly. Past-end = leaderless: morale floors at
  zero. Validation rejects decapitation against a faction without a
  cadre as an authoring mistake (silent runtime no-op otherwise).

**Coalition fracture (Epic D — round two).** Adds declarative
alliance-fracture rules so authors can express "this alliance breaks
when conditions X, Y, Z are met". Pairs with the previously-
unhandled `EventEffect::DiplomacyChange` event effect, which is now
wired in `tick.rs::apply_event_effects`. Both write to a shared
runtime override map (`SimulationState.diplomacy_overrides`) so
runtime stance is direction-aware and queryable via
`fracture::current_stance` / `fracture::baseline_stance`.

**Scope caveat (now partially closed by Epic D round three —
behavioral coupling).** As of round three, combat targeting and the
AI consume diplomatic stance directly: mutually-Allied pairs skip
combat entirely, and Cooperative neighbors are de-rated to 0.3× in
both threat presence and attack scoring. See the "Diplomatic stance
behavioral coupling" section below for the contract. A fracture
remains observable post-run via `RunResult.fracture_events` and the
`## Alliance Dynamics` report section, *and* it now flips behavior
at the tick the rule fires.
The victory-check and political phases still do not consult
diplomacy — that piece is left for a follow-up. Treat fire rates
under scenarios authored before round three as a scenario-design
diagnostic: they describe when the rule trips, not the
counter-factual behavior that would have unfolded if the engine had
consumed the stance from the start.

- Optional `[factions.<id>.alliance_fracture]` block declares one or
  more `FractureRule { id, counterparty, new_stance, condition }`.
  Five condition variants: `AttributionThreshold { attacker,
  threshold }` (mean attribution across attacker's chains crosses
  threshold), `MoraleFloor { floor }`, `TensionThreshold { threshold }`,
  `EventFired { event }`, and `StrengthLossFraction { delta_fraction }`.
  Evaluation runs end-of-tick after the campaign phase via the new
  `fracture_phase` in `crates/faultline-engine/src/fracture.rs`. One-
  shot per rule (latched in `SimulationState.fired_fractures`). Pure
  function of state — no RNG, so determinism is preserved.
- Validation rejects empty rules vector, unknown counterparty /
  attacker / event ids, self-targeting rules, duplicate rule ids
  within a faction, NaN / out-of-range thresholds, and
  `AttributionThreshold` against a faction that owns no kill chain
  (the silent-no-op shape).
- Per-run output on `RunResult.fracture_events` (one
  [`FractureEvent`](crates/faultline-types/src/stats.rs) per firing
  with previous and new stance captured live). Cross-run rollup in
  `MonteCarloSummary.alliance_dynamics` via
  `faultline_stats::alliance_dynamics::compute_alliance_dynamics` —
  per-rule fire rate, mean fire tick, and terminal-stance
  distribution. Both fields skip serialization when empty/None so
  legacy-scenario manifest hashes are unchanged.
- New `## Alliance Dynamics` report section in
  `crates/faultline-stats/src/report/alliance_dynamics.rs`. Elides
  entirely when no scenario declares `alliance_fracture`.
- Bundled archetype: `scenarios/coalition_fracture_demo.toml`. A
  Cooperative `gray_partner` declares both an attribution-driven
  rule and a tension-driven rule against `red_attacker`. Demo run
  produces ~22% attribution-fracture rate and 100% tension-fracture
  rate over 32 runs.

**Diplomatic stance behavioral coupling (Epic D — round three,
item 1; also closes R3-2 round-two item 2).** Closes the round-two
"analytical accounting only" caveat for the combat and AI phases.
Adds `crates/faultline-engine/src/diplomacy.rs`, two helpers, and
two integration points:

- `diplomacy::combat_blocked(state, scenario, a, b)` — true iff
  both A→B and B→A current stances are `Diplomacy::Allied`. Mutual
  alliance is required; one-sided declarations don't bind the other
  party. Reads `fracture::current_stance` so post-fracture and
  `EventEffect::DiplomacyChange` overrides are respected.
- `diplomacy::ai_threat_multiplier(state, scenario, self_id, other)`
  — scales `other`'s contribution to `self_id`'s perceived threat
  and attack-priority: `Allied` → 0.0 (excluded), `Cooperative` →
  0.3 (`COOPERATIVE_AI_FACTOR`, soft de-prioritization), else 1.0.
  Self-perspective only: a faction that mistakenly views a hostile
  party as Allied will fail to defend against them; that asymmetry
  is the intended signal in scenarios modeling miscalibrated
  diplomacy.
- Combat hook: `tick::combat_phase` calls `combat_blocked` before
  resolving each faction pair. Cooperative pairs still fight if
  their forces collide — the relationship is "we cooperate but
  aren't sworn allies", and accidental engagement is plausible.
- AI hook: `ai::compute_enemy_presence` and the two
  `evaluate_attack_actions` variants (ground-truth + fog) consult
  `ai_threat_multiplier`. The fog-of-war path reads stance from
  ground truth on the principle that a faction always knows its
  own declared posture; when `FactionWorldView.diplomacy` is wired
  up in a future epic, this can shift to consulting the world-view
  directly.
- The RNG draw in `evaluate_attack_actions` happens *before* the
  diplomacy multiplier check so adding an `Allied` declaration to a
  legacy scenario does not desync the RNG sequence for any
  unaffected pair — preserves bit-identical replay across legacy
  seeds.
- Validation rejects three silent-no-op shapes: self-stance
  declarations, unknown `target_faction`, and duplicate target
  entries (which silently shadow under first-match resolution).
- Determinism: every helper is a pure function of state and
  scenario — no RNG, no allocation. Adding a `Cooperative` /
  `Allied` declaration to a scenario *will* change combat /
  AI output, but determinism for any fixed seed holds.
- Backward-compat: scenarios without authored diplomacy default
  every pair to `Neutral`, which preserves legacy combat semantics.
  All bundled scenarios except `coalition_fracture_demo.toml`
  retain their pre-round-three behavior.
- Coverage: `crates/faultline-engine/tests/diplomacy_behavior.rs`
  pins (a) Allied pair skips combat, (b) Neutral default still
  fights, (c) Cooperative still fights but AI de-rates, (d)
  one-sided alliance does not block, (e) `DiplomacyChange` event
  flips behavior live, plus the three validation rejections.

**Supply-network interdiction (Epic D round three, item 2).** Closes
the round-one and round-two graph-shipped-but-passive caveat for
`kind = "supply"` networks: the per-tick attrition phase now reads
each owned supply network's residual capacity and multiplies the
owner's `resource_rate` by the resulting pressure ratio. Builds
directly on Epic L network primitives — no new schema fields, no
new event variants; just a new phase that consumes the existing
`NetworkRuntimeState` data.

- `crates/faultline-engine/src/supply.rs` is the producer. Two
  helpers: `is_active_supply_network(net)` (true iff
  `kind` matches `"supply"` case-insensitively *and* `owner` is
  `Some`), and `supply_pressure_for_faction(scenario, state, faction)`
  returning `(pressure ∈ [0, 1], sampled: bool)`. The `sampled` bit
  is what the attrition phase keys per-faction reporting on — it's
  `true` iff at least one non-degenerate owned supply network
  contributed to the product, so a faction whose only supply networks
  have zero baseline capacity doesn't get phantom "supply intact"
  samples. Pure functions of `(scenario, state)` — no RNG, no
  `HashMap`, no allocation in the hot path; iteration is
  `BTreeMap`-ordered.
- Pressure formula: for each owned supply network,
  `pressure_n = (residual_capacity / baseline_capacity).clamp(0, 1)`;
  per-faction pressure is the product across all owned supply
  networks. Residual matches `network::compute_sample`'s definition
  exactly so the live supply-pressure value and the post-tick
  resilience curve agree at every tick. Networks with
  `baseline = 0` (degenerate authoring — every edge has zero
  capacity) are skipped rather than treated as fully broken.
- Hook point: top of `tick::attrition_phase`. The pressure value is
  captured to `RuntimeFactionState.current_supply_pressure` and
  rolled into per-faction running counters (`supply_pressure_sum`,
  `supply_pressure_min`, `supply_pressure_pressured_ticks`) for
  the post-run report. Income is then `resource_rate × pressure`;
  upkeep is **not** attenuated — units still consume regardless of
  whether resupply is reaching them, which is the point of cutting
  supply lines. The capture only fires for factions that own at
  least one active supply network so legacy factions don't pollute
  the mean denominator.
- Validation: `kind = "supply"` (case-insensitive) without `owner`
  is rejected at scenario load — the engine has no faction to
  attenuate, so this shape is a silent no-op. The check matches the
  project pattern of failing loud at load time rather than at tick N.
- Per-run output: `RunResult.supply_pressure_reports`
  (`BTreeMap<FactionId, SupplyPressureReport>`) with
  `samples` / `mean_pressure` / `min_pressure` / `pressured_ticks`
  per owning faction. Cross-run rollup:
  `MonteCarloSummary.supply_pressure_summaries` (mean of means,
  mean of mins, worst min, mean pressured ticks, runs-with-any-
  pressure count). Both fields skip serialization when empty so
  legacy-scenario manifest hashes are unchanged.
- New `## Supply Pressure` report section in
  `crates/faultline-stats/src/report/supply_pressure.rs`. Elides
  when `summary.supply_pressure_summaries` is empty.
- `PRESSURE_REPORTING_THRESHOLD = 0.9` — pressure values strictly
  below this count toward `pressured_ticks`. Cosmetic; not load-
  bearing for any decision the engine makes (income scaling reads
  the raw value, not a thresholded one).
- Determinism: every helper is a pure function of state, the per-
  faction pressure is computed in `BTreeMap`-ordered iteration,
  and the running counters update deterministically. Adding a
  `kind = "supply"` network with an owner *will* change the
  affected faction's resource trajectory (and downstream observable
  outcomes), but determinism for any fixed seed holds.
- Backward-compat: scenarios without `kind = "supply"` networks
  (or without `owner` on those networks) see no change. The two
  bundled scenarios with supply-kind networks
  (`network_resilience_demo.toml` and the new
  `supply_interdiction_demo.toml`) now reflect the round-three
  income attenuation in their reports.
- Coverage: `crates/faultline-engine/tests/supply_interdiction.rs`
  pins (a) legacy/no-network → no report, (b) pristine network →
  pressure 1.0, (c) severed edge → proportional drop,
  (d) full severance → income gap matches `resource_rate × ticks`,
  (e) determinism across same-seed runs, plus the three validation
  rejections.

**Multi-front resource contention (Epic D round three, item 3).** Closes
the third Epic D round-three item: defender capacity (Epic K) was
modelled as a per-role silo, so two campaigns competing for the same
faction's defender attention either piled onto a single shared queue or
ran independently against unrelated queues. Real SOC operations
escalate: when tier-1 alert triage saturates, new alerts get pushed up
to tier-2 incident response; when tier-2 itself saturates, work
escalates further to a tier-3 forensics cell. This round adds the
declarative escalation chain.

- Two new optional fields on `DefenderCapacity` (in
  `crates/faultline-types/src/faction.rs`): `overflow_to:
  Option<DefenderRoleId>` names another role on the *same faction*
  whose queue receives spillover when this role saturates;
  `overflow_threshold: Option<f64>` (defaults to `1.0` in the engine)
  is the queue-depth fraction at which spillover engages. Setting
  `overflow_threshold = 0.8` against `queue_depth = 100` means
  "escalate once depth crosses 80" — modelling proactive load-shed
  policy versus the reactive default of "escalate only when full".
  Both fields use `#[serde(default, skip_serializing_if =
  "Option::is_none")]` so legacy scenario TOML loads byte-identically
  and roles without `overflow_to` cost zero overhead on the hot
  path.
- Engine spillover lives in `crates/faultline-engine/src/campaign.rs`
  as `enqueue_with_overflow`. Per per-phase Poisson noise draw:
  resolve the role's `DefenderCapacity`; if `overflow_to.is_some()`,
  split the count into `(direct, spillover)` where `direct` fills
  whatever headroom remains under `ceil(queue_depth × threshold)` and
  `spillover` is the rest; apply the existing `OverflowPolicy` to
  `direct` only; recursively call `enqueue_with_overflow` on the
  spillover portion against the named target role. Spillover takes
  precedence over `OverflowPolicy::DropNew` — declaring `overflow_to`
  is the analyst's signal that "escalate, don't drop" is the
  intended semantic.
- Per-role queue accounting on
  `state::DefenderQueueState`: `spillover_in: u64` is the cumulative
  count that arrived at this role via another role's overflow chain
  — it tracks the conservation chain link from upstream regardless of
  what this role then does with the items (so when this role itself
  further spills, `spillover_in` may exceed `total_enqueued`).
  `spillover_out` is the cumulative count this role redirected to its
  overflow target (not in `total_enqueued`; the items left this queue
  without ever being enqueued here). `total_enqueued` charges only
  the items that actually entered this role's queue policy — items
  that arrived but immediately spilled onward to another role are
  *not* counted, so the throughput counter stays meaningful for
  drop-rate analytics. Conservation invariant: for any saturated
  role `A` whose `overflow_to = B`, `A.spillover_out` exactly equals
  `B.spillover_in` (modulo the `MAX_OVERFLOW_CHAIN_DEPTH = 32`
  recursion guard, which surfaces as `total_dropped` on the would-be-
  target queue if it ever trips — defense in depth for hand-built
  fixtures only, never trips on validated authoring).
- `MAX_OVERFLOW_CHAIN_DEPTH = 32` is defense in depth against
  hand-built `SimulationState` fixtures that bypass the loader, and
  against any future schema mutation that might let a chain grow past
  a sane operational depth. Validation already rejects authored
  cycles at scenario load, so this guard never trips on a TOML-
  loaded scenario; a real SOC escalation ladder is at most 3–4 deep.
- Per-run output: `RunResult.defender_queue_reports` rows gain
  `spillover_in` / `spillover_out`; `MonteCarloSummary.defender_capacity`
  rows gain `mean_spillover_in` / `mean_spillover_out`. Both use
  `#[serde(default)]` so legacy summaries shaped by older engine
  versions deserialize cleanly. Report rendering lives in
  `crates/faultline-stats/src/report/defender_capacity.rs` as a new
  "Cross-role escalation" sub-section gated on `any role has non-zero
  spillover` — legacy single-queue scenarios (e.g.
  `alert_fatigue_soc.toml`) elide it entirely so the existing analyst
  view doesn't gain noise rows.
- Validation rejects six silent-no-op shapes at scenario load:
  unknown `overflow_to` role; cross-faction overflow (rejected as
  unknown-on-this-faction; the engine has no faction to escalate
  *to* under cross-faction routing without a wider design discussion
  about shared-services agreements); self-loops (`tier1 -> tier1`);
  cycles in the chain (BFS from each role, reject on revisit);
  `overflow_threshold` outside `[0, 1]` or NaN; `overflow_threshold`
  set without `overflow_to`. Mirrors the load-time-fail-loud pattern
  from every prior round.
- Determinism: every helper is a pure function of `(scenario, state,
  count)` — no RNG (the Poisson draw happens once at the top of the
  phase-noise loop), no `HashMap`, no allocation in the hot path.
  Adding `overflow_to` to a role *will* change the affected
  scenario's queue trajectory and downstream observable outcomes
  (detection rolls gated on a now-relieved tier-1 will catch more;
  rolls gated on a now-saturated tier-3 will catch less), but
  determinism for any fixed seed holds. All 19 bundled scenarios
  (including the new `multifront_soc_escalation.toml`) still
  `verify-bundled` deterministically.
- Backward-compat: scenarios without `overflow_to` reproduce the
  Epic K single-queue behavior exactly. The existing
  `alert_fatigue_soc.toml` declares two roles without `overflow_to`,
  so its behavior is unchanged. The new
  `scenarios/multifront_soc_escalation.toml` archetype declares a
  3-tier escalation cascade (tier-1 → tier-2 → tier-3 forensics)
  driven by a sustained 4-phase intrusion; the report demonstrates
  that the chokepoint moves from tier-1 (which would saturate at
  100% under the legacy single-queue model) to tier-3 (which
  saturates instead, absorbing the cascade's residual).
- Coverage: `crates/faultline-engine/tests/multifront_overflow.rs`
  pins (a) no-overflow_to → legacy single-queue behavior preserved,
  (b) tier-1 → tier-2 routes excess and conserves the chain,
  (c) tier-1 → tier-2 → tier-3 propagates spillover end-to-end,
  (d) lower threshold yields strictly more spillover than the 1.0
  baseline, (e) determinism across same-seed runs, plus the six
  validation rejections (unknown role, self-loop, cycle, out-of-
  range threshold, NaN threshold, threshold-without-target).

**Calibration scaffold (Epic N — round one).** Closes the foundational
piece of Epic N: every Monte Carlo report now carries a `## Calibration`
section that either back-tests against an authored historical analogue
(verdict ladder of Pass / Marginal / Fail per observation, plus a
roll-up) or surfaces a "purely synthetic" disclaimer for scenarios that
make no calibration claim. The scope is the framework, not the
reference scenario set — filling in cleanly-sourced single-event
analogues for the rest of the bundled scenarios is an explicit Epic N
follow-up.

- New optional `[meta.historical_analogue]` block on `ScenarioMeta`
  declares the precedent: `name`, `description`, `period` (free-form
  date label), `sources` (open-source citations — required non-empty),
  `confidence` (author confidence in analogue *fit*), and one or more
  `observations`. `#[serde(default, skip_serializing_if = ...)]` so
  scenarios without an analogue stay byte-identical on the wire.
- Three `HistoricalMetric` variants:
  - `Winner { faction }` — historical victor was a specific faction.
  - `WinRate { faction, low, high }` — across a reference set, the
    named faction won at this rate.
  - `DurationTicks { low, high }` — conflict resolved within this tick
    interval (inclusive on both ends).
- Calibration computation: pure function in
  `crates/faultline-stats/src/calibration.rs`. Per-observation:
  - `Winner`: Pass when MC modal *and* mass ≥ 50%; Marginal when
    modal-but-below-majority *or* non-modal-but-≥ 25%; Fail otherwise.
  - `WinRate`: Pass when MC point estimate ∈ `[low, high]`; Marginal
    when Wilson 95% CI overlaps the interval; Fail otherwise.
  - `DurationTicks`: Pass when ≥ 50% of MC `final_tick` values fall in
    the interval; Marginal when ≥ 25%; Fail otherwise.
  Overall verdict = worst per-observation verdict. Calibration claims
  compose as ANDs, not ORs.
- Wired into `compute_summary` after win-rate computation (so the win-
  rate denominator is shared, not recomputed). Output lives on
  `MonteCarloSummary.calibration: Option<CalibrationReport>` —
  serialization-skipped when `None` so legacy-scenario manifest hashes
  for synthetic scenarios are unaffected by the addition.
- New `## Calibration` report section
  (`crates/faultline-stats/src/report/calibration.rs`). One of three
  always-emit sections (alongside `Header` and `Methodology`):
  - Scenario has `historical_analogue` + summary has `CalibrationReport`:
    renders the analogue header (name, period, description, sources)
    plus the per-observation table and roll-up.
  - Scenario has `historical_analogue` but summary lacks a
    `CalibrationReport` (the empty-runs early return path): renders the
    analogue header plus a "no MC runs available" disclaimer.
  - Scenario has no analogue: renders a synthetic-scenario disclaimer
    explaining what the absence means for result interpretation. The
    reasoning: a report without a calibration statement leaves the
    reader to assume the numbers are externally anchored, which is
    exactly the trust gap Epic N exists to close.
- The CLI's `report.md` emission gate
  (`faultline-cli/src/main.rs::write_markdown_report`) was extended so
  scenarios that *only* have a `historical_analogue` (no kill chains,
  no networks, no civilian segments) get a `report.md` written. The
  calibration verdict would otherwise only surface on scenarios that
  already had one of the other analytical surfaces.
- Validation rejects four silent-no-op shapes at scenario load:
  empty `sources`, empty `observations`, `Winner` / `WinRate` against
  unknown faction (typos would silently produce 0% MC mass and a
  near-guaranteed `Fail`, which reads as a model failure when the real
  issue is the typo), and inverted / out-of-range / NaN bounds on
  `WinRate` and `DurationTicks`. Mirrors the load-time-fail-loud
  pattern from every prior round.
- Determinism: every helper is a pure function of state — no RNG, no
  `HashMap`, no engine re-runs. Adding a `historical_analogue` *will*
  change the affected scenario's manifest content hash because the
  calibration report is serialized into `MonteCarloSummary`; that's
  intended, since the analogue is part of the scenario's analytical
  claim. Scenarios without an analogue see no change.
- Backward-compat: scenarios without `[meta.historical_analogue]` see
  no behavior change. All 18 bundled scenarios still
  `verify-bundled` deterministically; `output_hash` for the 17
  pre-existing scenarios shifts to reflect the new always-emit
  Calibration section's synthetic-disclaimer text. The new
  `scenarios/calibration_demo.toml` demonstrates the full mechanism
  end-to-end (3 Fail + 1 Pass observations under the engine's current
  basic-attrition behavior — illustrating the diagnostic value).
- Coverage:
  - `crates/faultline-stats/src/calibration.rs::tests` (12 tests):
    Pass / Marginal / Fail for each metric variant, the overall-is-
    worst roll-up, no-analogue → None, and same-input-same-output
    determinism (compares JSON form to catch future field additions).
  - `crates/faultline-stats/src/report/calibration.rs::tests` (4
    tests): synthetic disclaimer on no analogue, full table + rollup
    with analogue + summary, header + disclaimer when summary missing,
    always-emit invariant.
  - `crates/faultline-engine/src/lib.rs::tests` (8 tests): each
    validation rejection is pinned, plus a positive case for a
    well-formed analogue with two observations.
- Scope caveat: the "reference scenario set" item from Epic N (5–10
  cleanly-sourced single-event analogues with constrained parameters)
  is explicitly deferred. The framework is foundational; filling it in
  is opportunistic per-scenario work. The bundled
  `calibration_demo.toml` uses a *stylized aggregate* analogue
  (statistical patterns from a reference set) rather than a single
  named historical event.

**Narrative competition + displacement flows (Epic D round-three item 4).** Closes
the final Epic D round-three item: the previously-fire-and-forget
`EventEffect::MediaEvent` now drives a persistent narrative store that
decays each tick, scores per-faction information dominance, nudges
segment sympathy toward the leading faction, and contributes to global
tension. A new `EventEffect::Displacement` variant pairs with the
existing civilian-segment `Flee` action to populate a per-region
displacement store that propagates across `Region.borders` adjacencies
each tick and absorbs back into the resident population at a separate
rate. Both mechanics elide entirely on legacy scenarios — the engine
short-circuits when the narrative store and displacement map are both
empty, so all 19 pre-existing scenarios still `verify-bundled`
deterministically with their prior `output_hash` values.

- `EventEffect::MediaEvent { narrative, credibility, reach, favors }`
  is now wired in `tick::apply_event_effects`. Each firing registers
  or reinforces a `NarrativeRuntimeState` keyed on the narrative
  string. Reinforcement adds `credibility × reach × (1 + 0.5 × fragmentation)`
  to the existing strength (clamped to `[0, 1]`) — fragmented audiences
  reinforce faster because bubble-targeted messages saturate sub-
  audiences without competing against a unified counter-narrative.
  Credibility and reach take the *max* of pre-existing and new values
  rather than averaging, so a higher-reach reinforcement pulls the
  live narrative toward the new value; `favors` stays sticky to the
  first firing's choice so a malicious "switch sides" reinforcement
  can't silently flip dominance attribution. Each firing also pushes a
  `NarrativeEvent` onto `SimulationState.narrative_events` (per-run
  log) with `was_new` distinguishing introductions from reinforcements.
- `EventEffect::Displacement { region, magnitude }` is the new variant.
  Adds `magnitude.clamp(0, 1)` displaced fraction to
  `SimulationState.displacement[region]`, clamping the resulting
  `current_displaced` to `[0, 1]`. Cumulative `total_inflow` accrues
  by the actually-applied delta (so a `magnitude = 0.5` event against
  a region already at 0.7 only adds 0.3 to inflow because the field is
  saturated).
- `tick::narrative_phase` runs end-of-tick after `information_phase`.
  Decays each narrative's strength by
  `BASE_NARRATIVE_DECAY × (1 - 0.5 × reach)` (high-reach narratives
  decay at half the rate of low-reach ones because they're saturated
  in the media landscape). Drops entries below
  `NARRATIVE_DROP_EPSILON = 0.005`. Scores per-faction dominance
  (`sum(strength × credibility)` over narratives that favor each
  faction); the leading faction (max with lexicographic tie-break)
  accrues a tick on `SimulationState.narrative_dominance_ticks` and
  its peak attribution is captured for the cross-run rollup. Applies
  a sympathy nudge toward the leader scaled by
  `disinformation_susceptibility × leader_score`. Adds a tension
  delta capped at `NARRATIVE_MAX_TENSION_DELTA = 0.02`. Updates
  `non_kinetic.information_dominance` to the leading score (clamped to
  `[0, 1]`).
- `tick::displacement_phase` runs end-of-tick after `narrative_phase`.
  Single-pass propagation: each region's pre-tick displaced fraction
  contributes `outflow = displaced × DISPLACEMENT_PROPAGATION_RATE`
  (10%/tick) split evenly across `Region.borders`, plus
  `absorbed = displaced × DISPLACEMENT_ABSORPTION_RATE` (5%/tick) that
  merges back into the resident population. The remainder stays put
  for the next tick. Receiving regions accumulate inflows in a separate
  `BTreeMap` first, then apply them after every source has computed its
  outflow — so per-tick state mutation is single-pass, mirroring
  `network` and `supply` phase conventions. Tension delta proportional
  to average displaced fraction, capped at
  `DISPLACEMENT_MAX_TENSION_DELTA = 0.005`.
- `CivilianAction::Flee` (the existing population-segment action) was
  extended to *also* push displacement: a segment flee with `rate = 0.10`
  spread across two concentrated regions adds 0.05 displaced to each.
  The previous behavior of shrinking `seg.fraction` is unchanged. This
  is the organic-source path for displacement on scenarios that don't
  author scripted `Displacement` events.
- Per-run output:
  - `RunResult.narrative_events` (`Vec<NarrativeEvent>`) — emission-
    ordered log of every reinforcement event;
  - `RunResult.displacement_reports` (`BTreeMap<RegionId, RegionDisplacementReport>`) —
    one row per region with non-zero peak across the run, capturing
    `peak_displaced` / `terminal_displaced` / `total_inflow` /
    `total_outflow` / `total_absorbed` / `stressed_ticks`. Pristine
    regions are elided so legacy scenarios pay zero `RunResult` shape
    overhead.
- Cross-run rollups in `faultline_stats`:
  - `MonteCarloSummary.narrative_dynamics: Option<NarrativeDynamics>`
    (in `crates/faultline-stats/src/narrative_dynamics.rs`) — per-
    faction `mean_dominance_ticks` / `max_dominance_ticks` /
    `mean_peak_information_dominance` / `total_firings`, plus per-
    narrative-key `firing_runs` / `mean_firings_per_run` /
    `mean_peak_strength` / `mean_first_tick` / `modal_favors`. The
    per-faction dominance proxy here is a stream-level approximation
    (counts events where the favored faction was leader-by-pressure
    among events visible up to that tick), which is directionally
    correct for ranking purposes.
  - `MonteCarloSummary.displacement_summaries: BTreeMap<RegionId, DisplacementSummary>`
    (in `crates/faultline-stats/src/displacement.rs`) — per-region
    `stressed_runs` / `mean_peak` / `max_peak` / `mean_terminal` /
    `mean_total_inflow` / `mean_total_outflow` across the batch. Empty
    when no run had any displacement activity.
- Two new report sections (`crates/faultline-stats/src/report/narrative_dynamics.rs`
  and `crates/faultline-stats/src/report/displacement.rs`). Both gate
  on per-mechanic data presence; legacy scenarios elide entirely.
  Section count grew from 23 to 25 (entries 20 = NarrativeDynamics,
  22 = Displacement; CivilianActivations stays at 21; Calibration
  shifts to 23, Methodology to 24).
- Validation rejects six silent-no-op shapes at scenario load: empty
  `MediaEvent.narrative`; non-finite or out-of-range
  `MediaEvent.credibility` / `MediaEvent.reach`; unknown
  `MediaEvent.favors` faction; unknown `Displacement.region`; non-
  finite or out-of-range `Displacement.magnitude`; zero
  `Displacement.magnitude`. Mirrors the load-time-fail-loud pattern
  from every prior round.
- Determinism: every helper is a pure function of `(state, scenario)`
  — no RNG, no `HashMap`, `BTreeMap`-ordered iteration. Adding a
  `MediaEvent` or `Displacement` event *will* change the affected
  scenario's combat / political trajectory and downstream observable
  outputs (the narrative phase nudges sympathy and tension; the
  displacement phase contributes a small tension delta), but
  determinism for any fixed seed holds.
- Backward-compat: scenarios without `MediaEvent` and without
  `Displacement` see no change. The narrative store and displacement
  map both stay empty for the run, the phases short-circuit, and the
  report sections elide. All 19 pre-existing bundled scenarios
  `verify-bundled` deterministically with their prior `output_hash`
  values; the new `scenarios/narrative_competition_demo.toml`
  demonstrates the full mechanism end-to-end with a Red 2-firing /
  Blue 1-firing dominance asymmetry plus a 30% scripted refugee wave
  in `frontier_north` that propagates to `frontier_south`.
- Coverage:
  `crates/faultline-engine/tests/narrative_and_displacement.rs` (16
  tests): legacy fast paths (no narrative state / no displacement
  state when neither effect is authored), single-firing introduction
  marks `was_new = true`, second firing reinforces with `was_new =
  false` and increased strength, decay over time produces strictly-
  declining strength after introduction, propagation moves displaced
  fraction to adjacent regions, absorption shrinks `terminal_displaced`
  below `peak_displaced`, determinism across same-seed runs, plus the
  six validation rejections (empty narrative, out-of-range credibility,
  unknown `favors`, unknown region, negative / NaN / zero magnitude).
  Plus 4 unit tests in
  `crates/faultline-stats/src/narrative_dynamics.rs::tests`,
  3 unit tests in `crates/faultline-stats/src/displacement.rs::tests`,
  and 3 + 3 unit tests in the per-section report modules.

**Unread-parameter audit (R3-2 round one).** Three previously-silent
fields now affect simulation outcomes; each was authored in dozens of
bundled scenarios but had zero engine effect:

- `Faction.command_resilience` ∈ `[0,1]` attenuates the morale shock
  from `LeadershipDecapitation`: `effective_shock = morale_shock × (1 − resilience)`.
  Wired in `campaign::apply_leadership_decapitation`. No-op for
  factions without a `leadership` cadre.
- `ForceUnit.morale_modifier` multiplies the unit's effective combat
  contribution as `(1.0 + morale_modifier)`. Wired in
  `tick::find_contested_regions`. Floored at `0` so a pathological
  override below `-1.0` cannot invert the combat math.
- `Scenario.defender_budget` is the symmetric mirror of
  `attacker_budget` but uses reactive semantics: once cumulative
  `defender_spend` exceeds the cap, `SimulationState.defender_over_budget_tick`
  latches sticky and a 0.5× detection-probability multiplier
  (`DEFENDER_OVER_BUDGET_DETECTION_FACTOR`) applies to all subsequent
  kill-chain phase rolls. Latched at tick-start so chain-processing
  order can never affect which phase first incurs the penalty.

Regression suite: `crates/faultline-engine/tests/audit_unread_params.rs`
(10 tests including a 32-seed statistical regression for the
defender-budget detection penalty). See `docs/improvement-plan.md` R3-2
for deferred items (`upkeep`, `mobility`, `diplomacy`,
population-segment activation, tech-card costs).

**Unread-parameter audit (R3-2 round two — movement rate).** Three
movement-related fields that were silent in round one now compose
into a single per-tick "effective mobility" gate. The wiring lives in
`crates/faultline-engine/src/tick.rs` (`movement_phase` /
`environment_movement_factor`) and pairs with a new runtime field
`ForceUnit.move_progress` (`#[serde(default)]`, so legacy TOML loads
unchanged).

- `ForceUnit.mobility` — per-unit movement rate.
- `TerrainModifier.movement_modifier` — per-region movement
  attenuator. Read from the unit's *source* region; a unit moving
  out of a 0.5-modifier region is gated by 0.5 regardless of the
  destination.
- `EnvironmentWindow.movement_factor` — globally-scoped weather /
  time-of-day attenuator on top of the per-region modifier.
  Composes via `tick::environment_movement_factor` (multiplicative
  over every active window covering the source-region terrain).

Per move-attempt:
`effective_mobility = (mobility × terrain_modifier × env_factor).max(0.0)`
is added to `move_progress`, capped at `1.0`. The queued
`MoveUnit` action only fires once `move_progress >= 1.0`, at which
point exactly `1.0` is consumed (a unit with mobility 0.5 takes
two attempts to move; a unit with mobility 2.0 still moves every
tick — the cap prevents saved-up moves). Default authoring
(`mobility = 1.0`, terrain modifier 1.0, no env windows) reproduces
the previous "unit moves every tick when queued" behavior exactly.

Validation rejects three silent-no-op shapes: non-finite or
negative `ForceUnit.mobility`, non-finite or negative
`TerrainModifier.movement_modifier` (negative would silently invert
the gate; NaN would propagate via `(1.0 + NaN).max(0.0) → 0.0`,
freezing the unit on that region); env-window factor validation
(`validate_environment_window`) already rejected non-finite
`movement_factor` from Epic D round-one.

Determinism: every helper is a pure function of `(scenario, state,
tick)` — no RNG, no allocation in the hot path. Adding a non-1.0
`mobility`, terrain modifier, or env `movement_factor` *will* change
the affected scenario's combat schedule (and downstream observable
outcomes), but determinism for any fixed seed holds.

Regression suite: `crates/faultline-engine/tests/audit_unread_params.rs`
gains 10 tests pinning rate-gate semantics, multiplicative
composition, the cap behavior, and the three validation rejections.
The integration-test fixture in `crates/faultline-engine/tests/integration.rs::base_scenario`
was tightened to use uniform `movement_modifier = 1.0` so combat /
tech tests aren't accidentally exercising the new gate (defense
modifiers and visibility keep their per-region variation, which the
combat suite still depends on).

Round-two items still deferred: `Faction.diplomacy` (closed by Epic
D round-three item 1), tech-card costs, `Region.centroid` /
`Faction.color` (visualization metadata),
`ForceUnit.force_projection`. See `docs/improvement-plan.md` R3-2
for the priority order. `ForceUnit.upkeep` was already wired in an
earlier change (sums per-tick over `fs.forces` and deducts from
`resources` in `tick::attrition_phase`); the round-two plan entry
listing it as silent is now corrected. Population-segment activation
now closed — see the next section.

**Unread-parameter audit (R3-2 round two — population-segment
activation).** Closes the round-one and round-two "half-built" caveat
on `[political_climate.population_segments]`. Three previously-silent
`MediaLandscape` fields — `fragmentation`, `social_media_penetration`,
and `internet_availability` — are now load-bearing on the political /
information phases, and every civilian-segment activation is now
tracked, aggregated across runs, and surfaced in the post-run report.

- `faultline_politics::update_civilian_segments` reads all three new
  media fields. Per (segment, sympathy) pair it computes
  `noise_amp = 1.0 + 0.5 * fragmentation + 0.5 * effective_social_media`
  and `tension_scale = 1.0 - fragmentation`, then updates
  `sympathy = clamp(sympathy + noise * noise_amp + tension_pull * tension_scale)`.
  `effective_social_media = social_media_penetration × internet_availability`
  — the multiplication is the "lights out" guard: if internet is
  offline, social-media penetration alone has no effect. Determinism
  is preserved: exactly one RNG draw per (segment, sympathy) per
  tick, same as before. The new fields scale the draw, they don't
  add or remove draws.
- `tick::information_phase` reads the same three fields plus the
  legacy `disinformation_susceptibility` and `state_control`. High
  fragmentation × high effective social media amplify the
  disinfo→tension delta by up to 2× when both are at 1.0; legacy
  authoring (`fragmentation = 0`, `social_media_penetration = 0`) is
  reproduced exactly.
- `SimulationState.civilian_activations` (new) logs every activation
  in emission order; `RunResult.civilian_activations` (new) surfaces
  it post-run; `MonteCarloSummary.civilian_activation_summaries`
  (new, in `crates/faultline-stats/src/civilian_activations.rs`)
  rolls per-segment statistics across runs (activation rate, mean
  activation tick, per-action firing counts, modal favored faction
  with `BTreeMap`-order tie-break).
- `CivilianActivationEvent` carries the action discriminants as
  `Vec<String>` (rather than the typed enum) so cross-run
  aggregation can count action firings without dragging the typed
  payload into the manifest schema. `tick::civilian_action_kind`
  is the canonical mapping; the function is exhaustive on
  `CivilianAction` so adding a new variant fails compilation here,
  forcing a deliberate decision about how to surface it.
- New `## Civilian Activations` report section in
  `crates/faultline-stats/src/report/civilian_activations.rs`. Elides
  when `summary.civilian_activation_summaries` is empty (i.e. no
  scenario declared `population_segments`). A scenario that declared
  segments but produced zero activations across the run set still
  emits — the analyst sees "segment X declared, never tripped"
  rather than an unexplained absence. The report-render gate in
  `faultline-cli/src/main.rs` was extended so scenarios that *only*
  have civilian segments (no kill chains, no networks) get a
  `report.md` written.
- Validation rejects four silent-no-op shapes at scenario load:
  out-of-range or non-finite `MediaLandscape.*` fields (legacy
  `disinformation_susceptibility` / `state_control` are now
  validated alongside the three new ones); duplicate segment ids;
  segments concentrated in unknown regions; `volatility` /
  `activation_threshold` / `fraction` / sympathy values out of
  range or non-finite. Mirrors the load-time-fail-loud pattern from
  every prior R3-2 round.
- Backward-compat: scenarios with default-zero `MediaLandscape`
  values reproduce legacy noise / tension behavior exactly. The
  three bundled scenarios with non-trivial population segments
  (`tutorial_asymmetric.toml`, `drone_swarm_destabilization.toml`,
  `us_institutional_fracture.toml`) now reflect the new wiring in
  their `report.md`. All 17 bundled scenarios still
  `verify-bundled` deterministically.
- Coverage: `crates/faultline-engine/tests/audit_unread_params.rs`
  gains 7 tests pinning (a) end-to-end activation event capture
  with action-kind ordering, (b) fragmentation amplifies drift,
  (c) internet=0 zeroes social-media amplification (the lights-out
  guard), (d) determinism across same-seed runs, plus the four
  validation rejections. The cross-run aggregator and the report
  section have their own unit tests in their respective modules.

Round-two items still deferred after this round: tech-card costs
(closed by the section below), visualization metadata
(`Region.centroid` / `Faction.color`), `ForceUnit.force_projection`.

**Unread-parameter audit (R3-2 round two — tech-card costs).** Closes
the round-one and round-two "tech is free, instant, and unbounded"
caveat for `[technology.<id>]` entries. Three previously-silent
`TechCard` fields — `deployment_cost`, `cost_per_tick`, and
`coverage_limit` — are now load-bearing across the engine. Authored
in dozens of bundled scenarios with non-trivial values; until this
round, every value was inert.

- `deployment_cost` is deducted at engine init from the faction's
  `initial_resources`. The init loop walks `Faction.tech_access` in
  declaration order: each card whose `deployment_cost` is `<= resources`
  is deployed and the cost subtracted; cards whose cost exceeds what's
  left are *denied* (skipped, not added to `tech_deployed`) and
  recorded for reporting. Iteration continues past a denial — a
  denied big-ticket card doesn't prevent a later cheaper card from
  fitting. Cards referenced in `tech_access` but absent from
  `scenario.technology` are deployed at zero cost; that preserves the
  legacy "missing tech is a silent no-op at combat time" contract.
  Hook point: `crates/faultline-engine/src/engine.rs::initialize_state`.
- `cost_per_tick` is deducted in the attrition phase per-tech, after
  income (with supply-pressure attenuation) and upkeep have settled.
  Each card whose maintenance cost exceeds the faction's current
  resources is *decommissioned* — removed from `tech_deployed` for
  the rest of the run, no further charges, no refund. Decommissioning
  is final: the card does not re-deploy if resources later recover.
  Iteration is in `tech_deployed` declaration order, deterministic.
  Hook point: `crates/faultline-engine/src/tick.rs::attrition_phase`.
- `coverage_limit` (when `Some(n)`) caps the per-tick number of
  (region, opponent) pairs the card contributes to during combat.
  `compute_tech_combat_modifier` reads the per-faction
  `tech_coverage_used` counter (cleared at the top of `combat_phase`)
  and skips a card whose count has reached the limit; the caller
  bumps the counter for cards that were applied. Cards without a
  `coverage_limit` (the legacy default `None`) bypass the gate and
  stay out of the counter map entirely, so legacy scenarios pay zero
  bookkeeping overhead. The gate's iteration order — `BTreeMap` over
  contested regions, then over factions in each region — is
  deterministic, so which (region, opponent) pairs receive the
  benefit when supply is constrained is reproducible across runs.
  Hook point: `crates/faultline-engine/src/tick.rs::combat_phase` +
  `compute_tech_combat_modifier`.
- Per-run output: `RunResult.tech_costs` (`BTreeMap<FactionId, TechCostReport>`)
  records per-faction deployed / denied / decommissioned card lists
  plus total deployment and maintenance spend. The map elides
  factions whose tech roster never engaged the cost mechanic
  (zero-cost cards, no denials, no decommissions), so legacy
  scenarios with all-zero tech costs see no change in `RunResult` shape.
- Cross-run rollup:
  `MonteCarloSummary.tech_cost_summaries` (`BTreeMap<FactionId, TechCostSummary>`).
  Per-faction mean deployment / maintenance / total spend, plus
  `runs_with_denial` and `runs_with_decommission` (count-style
  diagnostics rather than rates so the report renders both the
  proportion and the underlying sample size). Producer:
  `compute_tech_cost_summaries` in
  `crates/faultline-stats/src/lib.rs`.
- New `## Tech-Card Costs` report section in
  `crates/faultline-stats/src/report/tech_costs.rs`. Elides when
  `summary.tech_cost_summaries` is empty. Surfaces a real signal
  on existing scenarios — `tutorial_asymmetric.toml` for instance
  hits a 100% decommission rate against both factions because the
  `cost_per_tick` for `surveillance_drone` (3.0) and
  `concealment_network` (1.0) outpaces the factions' modest
  `resource_rate` after upkeep. That's a legitimate diagnostic the
  audit was meant to surface, not a regression.
- Validation rejects three silent-no-op shapes at scenario load:
  non-finite or negative `deployment_cost` / `cost_per_tick`, and
  `coverage_limit = Some(0)` (the gate's `used >= limit` check is
  true on the first attempt — the card never contributes). Mirrors
  the load-time-fail-loud pattern from every prior R3-2 round.
- Determinism: every helper is a pure function of state and scenario
  — no RNG, no `HashMap`, no allocation in the hot path beyond what
  the existing combat loop already does. Adding a non-zero
  `deployment_cost` / `cost_per_tick` / `coverage_limit` to a card
  *will* change the affected scenario's combat schedule and
  resource trajectory (and downstream observable outcomes), but
  determinism for any fixed seed holds. All 17 bundled scenarios
  still `verify-bundled` deterministically; their `output_hash`
  values shift to reflect the new mechanic.
- Backward-compat: scenarios with all-zero tech costs (the default
  for cards added after this round) reproduce legacy behavior
  exactly. Tests cover the default-zero baseline along with the
  three field-specific behaviors.
- Coverage: `crates/faultline-engine/tests/audit_unread_params.rs::tech_costs`
  pins (a) deployment cost deduction, (b) deployment denial when
  unaffordable, (c) iteration-past-denial, (d) cost_per_tick
  deduction, (e) decommission on unaffordable maintenance, (f)
  coverage uncapped → no tracking, (g) coverage_limit = 1 caps,
  (h) coverage_limit > demand still tracks actual usage, (i)
  determinism across same-seed runs, (j) report emission gate, (k)
  zero-cost roster elides, plus the three validation rejections.

Round-two items still deferred after this round: visualization
metadata (`Region.centroid` / `Faction.color`),
`ForceUnit.force_projection`.

CI pipeline order: **fmt -> clippy -> test -> build -> cargo-deny -> grep-guard -> verify-bundled -> verify-migration -> verify-robustness -> js-tests**.

The JS tests cover the pure-logic frontend modules (sharing roundtrip,
heatmap aggregation, the Pinned MC results store, the comparison-delta
computation that mirrors `faultline_stats::counterfactual::compute_delta`,
the LCS unified-diff renderer, the grep-guard CI script, and the
site/scenarios symlink contract). They run on the host (not in the
rust-ci container) and only depend on `node:test`; CI provisions the
runtime with `actions/setup-node@v4`.

The grep-guard stage (`tools/ci/grep-guard.sh`) blocks any commit that
re-introduces references coupling Faultline to a specific external
threat-assessment publication series. The patterns it bans, the
whitelist, and the rationale are documented inline in the script. To
run it locally: `./tools/ci/grep-guard.sh` — exit 0 = clean, exit 1 =
banned-pattern match found.

The verify-bundled stage (`tools/ci/verify-bundled-scenarios.sh`)
emits a `manifest.json` for every TOML in `scenarios/` and replays
each one via `faultline-cli --verify` to confirm bit-identical
output. Catches drift in the determinism contract before it leaks
into a release. Run locally: `./tools/ci/verify-bundled-scenarios.sh`.

The verify-migration stage (`tools/ci/verify-migration.sh`) runs
`faultline-cli --migrate` on every TOML in `scenarios/` and
re-validates the migrated form. Catches drift between the schema
migration framework and the bundled scenarios. Schema versioning
lives in `crates/faultline-types/src/migration.rs`; see
`docs/scenario_schema.md` for the schema-evolution policy. Run
locally: `./tools/ci/verify-migration.sh`.

The verify-robustness stage (`tools/ci/verify-robustness-pipeline.sh`)
exercises the full `--search → --robustness --robustness-from-search →
--verify` flow against `scenarios/defender_robustness_demo.toml`, then
tampers with the source `search.json` and confirms `--verify` rejects
on hash mismatch. Catches CLI-glue regressions in the search-then-
robustness flow that the library-level tests in
`crates/faultline-stats/tests/epic_i_robustness.rs` can't reach. Run
locally: `./tools/ci/verify-robustness-pipeline.sh`.

To match CI exactly (containerized):
```bash
docker compose --profile ci run --rm rust-ci cargo test
```

## Scenario explain (Epic P sub-item)

`faultline-cli --explain <scenario>` produces a structured "what does
this scenario actually model?" summary without running the engine.
Pure schema view — no RNG, no simulation, no I/O beyond reading the
scenario file. Output goes to stdout (Markdown by default; pass
`--explain-format json` for the structured form). Mutually exclusive
with the run modes.

The Markdown render emits a stable section sequence: header (name,
author, version, schema version, tags, author confidence, prose
description) → Scale (counts) → Factions → Kill chains → Victory
conditions → Networks → Decision-variable surface → Low-confidence
parameters. The decision-variable surface answers "which parameters
does this scenario actually move under `--search` / `--coevolve` /
`--robustness`?" — the same question R3-2 asks of the engine. The
low-confidence section pulls together every author-flagged Low cell
(scenario-level `confidence`, per-phase `parameter_confidence`,
per-phase-cost `confidence`) so the analyst sees up-front which knobs
to push on under counterfactual.

The producer lives in `crates/faultline-stats/src/explain.rs`. A
single pure function `explain(&Scenario) -> ExplainReport` produces a
[`Serialize`/`Deserialize`] structure; `render_markdown(&report)` is
the Markdown renderer. Both are reusable by other tooling — the
browser, a future Epic P "Explain" button — without dragging in the
CLI. Integration coverage in `crates/faultline-stats/tests/explain_integration.rs`
runs explain against every bundled scenario and pins section
ordering.

## Report module layout (R3-3)

The Markdown report renderer was decomposed in R3-3 from a single
`report.rs` into `crates/faultline-stats/src/report/` — one file per
section.

- `mod.rs` — public API (`render_markdown`, plus `pub use` re-exports
  of the four other render functions), the `ReportSection` trait, and
  the `monte_carlo_sections()` array that declares section ordering.
- One file per Monte Carlo section (`header.rs`, `win_rates.rs`,
  `feasibility.rs`, `phase_breakdown.rs`, `time_dynamics.rs`, etc.).
  Each file defines a unit struct that `impl ReportSection` and lives
  *only* in the array in `mod.rs`.
- `comparison.rs`, `search.rs`, `coevolve.rs`, `robustness.rs` —
  the four other report-type renderers (each with its own input shape).
- `util.rs` — three helpers (`escape_md_cell`, `fmt_scalar`,
  `confidence_word`) that more than one section consumes. Helpers used
  by exactly one section live in that section's module.
- `test_support.rs` — `empty_summary` / `minimal_scenario` fixtures
  for per-section unit tests, gated behind `#[cfg(test)]`.

**Adding a new section** — create one file in `report/` with a unit
struct implementing `ReportSection`, then add one entry to the
`monte_carlo_sections()` array in `mod.rs`. Section gating (elision
when the underlying data is empty) lives in the `impl`, not the
composer.

**Determinism contract** — the rendered Markdown is part of the
manifest content hash, so changing section ordering or adding any
unconditional output flips every bundled scenario's `output_hash`
and breaks `--verify`. The `verify-bundled` CI step catches this.

## Command effectiveness as a separate axis (R3-4)

Closes the round-three-follow-up R3-4: the Epic D round-one leadership
cadre originally pushed decapitation degradation directly into
`morale` via a per-tick clamp step (`apply_leadership_caps`), so a
strike landed as both a chain-of-command effect and a rank-and-file
morale shift. That conflated two distinct axes — *will to fight* (how
hard the troops will hit) versus *capacity to direct that will* (how
effectively the chain of command can convert troops into action) —
and contaminated the morale signal that political / alliance-fracture
phases consume.

R3-4 splits them. `RuntimeFactionState.command_effectiveness ∈ [0, 1]`
(default `1.0`) is now a separate runtime field; combat and AI
threat-scoring read `morale × command_effectiveness` (via the helper
`tick::effective_combat_morale`) rather than raw morale. A new phase
step `tick::update_command_effectiveness` (replacing
`apply_leadership_caps`) writes the leadership factor into
`command_effectiveness` end-of-tick, after the campaign phase.

- The legacy fast path is unchanged: when no faction declares a
  `leadership` cadre, the writer short-circuits and every faction's
  `command_effectiveness` stays at its `1.0` default. Combat reads
  `morale × 1.0`, which is bit-identical to the pre-R3-4 read of raw
  morale. All 19 bundled scenarios verify deterministically with their
  prior `output_hash` values.
- Scenarios with leadership cadres see a behavior shift: morale stays
  untouched by the leadership writer. Combat outcomes reflect the
  decapitation just as before (because the multiplier passes through),
  but the `MoraleFloor` alliance-fracture condition no longer
  incidentally fires from a leadership strike — only from the
  explicit `morale_shock` carried by the phase output and from
  political-phase / combat-loss morale drift. This is the intended
  semantic split: a leader being killed degrades command authority
  without necessarily breaking rank-and-file morale.
- The two bundled scenarios that declared a `leadership` cadre
  (`defender_robustness_demo.toml`, `defender_posture_optimization.toml`)
  shifted their `output_hash` to reflect the new semantics; both still
  `verify-bundled` deterministically.
- AI bias coupling: `ai::determine_weights` now reads
  `effective_combat_morale` instead of raw morale. A faction with
  intact rank-and-file morale but degraded command (recovery ramp
  active) correctly shifts toward defensive posture rather than
  continuing to behave as if its full offensive capability were
  available. The fog-of-war path is unchanged because
  `FactionWorldView.morale` has no current consumer; that field stays
  on raw morale as a placeholder for future Epic M (belief asymmetry)
  work.
- Future composition: command-degrading effects can multiply directly
  into `command_effectiveness` without colliding with morale's other
  consumers. Logistics-targeted strikes, command-jamming, supply-
  pressure tier escalation are the natural next sources. Each new
  source becomes one more multiplicative factor in the writer.
- Snapshot exposure: `FactionState.command_effectiveness` (in
  `crates/faultline-types/src/strategy.rs`) is now part of every
  per-tick snapshot, with `#[serde(default = "default_command_effectiveness")]`
  defaulting to `1.0` so legacy snapshots deserialize unchanged. The
  `## Leadership Cadres` report section's prose was updated to describe
  the new mechanic.
- Determinism: `update_command_effectiveness` is a pure function of
  `(state, scenario)` — no RNG, no `HashMap`, `BTreeMap`-ordered
  iteration. Idempotent across ticks. Adding a `leadership` cadre to
  a scenario *will* change the affected scenario's combat schedule
  and downstream observable outputs (the multiplier propagates), but
  determinism for any fixed seed holds.
- Coverage: the Epic D round-one tests in
  `crates/faultline-engine/tests/integration.rs` were updated to
  assert `command_effectiveness` (not morale) for the cap value, and
  two new R3-4 tests pin the contract:
  `r3_4_decapitation_does_not_pollute_raw_morale` (raw morale stays
  above the leadership factor floor; `effective_combat_morale` equals
  the product) and `r3_4_no_cadre_legacy_path_leaves_morale_and_command_unchanged`
  (legacy fast path still produces `command_effectiveness == 1.0`).

## Multi-term utility & adaptive AI (Epic J round-one)

Closes the first slice of Epic J: factions can now declare a
`[factions.<id>.utility]` block that re-weights AI action scoring
along named analyst-facing axes. The utility surface composes
additively on top of the existing doctrine-based scoring, so
scenarios without `[utility]` are bit-identical to the legacy path
(verified by `verify-bundled` across all 21 pre-existing scenarios).

- New types in `crates/faultline-types/src/faction.rs`:
  - `UtilityTerm` enum with seven analyst-facing axes: `Control`,
    `CasualtiesSelf`, `CasualtiesInflicted`, `AttributionRisk`,
    `TimeToObjective`, `ResourceCost`, `ForceConcentration`. Each
    variant maps to a per-action expected delta in the round-one
    heuristic table documented at the top of
    `crates/faultline-engine/src/utility.rs`. The enum derives `Hash`
    + `Ord` for deterministic `BTreeMap` keys; the wire-stable
    `as_key()` method (`"control"`, `"casualties_self"`, ...) is
    used in serialization to keep manifest hashes stable across
    binary representations of the enum.
  - `FactionUtility` struct: `terms: BTreeMap<UtilityTerm, f64>`
    (base weights), `triggers: Vec<AdaptiveTrigger>` (optional
    adaptive adjustments), `time_horizon_ticks: Option<u32>` (per-
    faction deadline override).
  - `AdaptiveTrigger` and `AdaptiveCondition`: seven condition
    variants (`MoraleBelow`, `MoraleAbove`, `TensionAbove`,
    `TickFraction`, `ResourcesBelow`, `StrengthLossFraction`,
    `AttributionAgainstSelf`). Each is a pure function of state +
    scenario; matched triggers compose multiplicatively against base
    term weights to produce the effective weights the engine uses.

- `crates/faultline-engine/src/utility.rs` is the producer. Two
  helpers: `effective_weights(profile, faction, state, scenario,
  campaigns)` evaluates each declared trigger against current state
  and returns the per-term effective weights plus the IDs of
  triggers that fired this phase; `evaluate_action_utility(weights,
  faction, action, state, scenario, map)` computes the per-action
  utility delta from the round-one heuristic table. Both are pure
  functions — no RNG, no `HashMap`, `BTreeMap`-ordered iteration in
  the hot path.

- AI integration: `crates/faultline-engine/src/ai.rs::evaluate_actions`
  and `evaluate_actions_fog` now accept a
  `campaigns: &BTreeMap<KillChainId, CampaignState>` argument and,
  after the existing doctrine-based scoring loop, apply the utility
  delta on top via `apply_utility_score`. The doctrine score is
  unchanged on the legacy fast path (`Faction.utility == None`); when
  set, `ScoredAction.utility` carries the per-term decomposition for
  the post-run report.

- Decision-phase orchestration in
  `crates/faultline-engine/src/tick.rs::decision_phase`: passes
  `campaigns` through, captures the per-term contributions across the
  *top-3 selected* actions (the actions the engine actually executes,
  not the whole candidate set) into a per-faction
  `state.utility_decisions: BTreeMap<FactionId, UtilityDecisionLog>`,
  and tracks per-trigger fire counts. `tick.rs::decision_phase` is
  the only mutation site for `utility_decisions` so the determinism
  contract is centralized.

- Determinism: every helper is a pure function — no new RNG draws,
  no `HashMap`, `BTreeMap`-ordered iteration. Adding a `[utility]`
  block to a scenario *will* change the affected scenario's combat
  schedule and downstream observable outputs (the score re-ranking
  shifts which top-3 actions the engine picks), but determinism for
  any fixed seed holds. The `verify-bundled` CI step pins this
  across all 21 bundled scenarios.

- Per-run output: `RunResult.utility_decisions` (`BTreeMap<FactionId,
  UtilityDecisionReport>`) records per-faction `tick_count`,
  `decision_count`, `term_sums` (keyed by stable string), and
  `trigger_fires`. Empty when no faction declares `[utility]` so
  legacy `RunResult` shapes are unchanged.

- Cross-run rollup: `MonteCarloSummary.utility_decompositions`
  (`BTreeMap<FactionId, UtilityDecompositionSummary>`). Producer:
  `faultline_stats::utility_decomposition::compute_utility_decompositions`.
  Per-faction `mean_contributions_per_decision` (per-term mean
  contribution averaged across all selected actions in all runs),
  `trigger_fire_rates` (per-trigger firing frequency), and
  `runs_with_contribution`. Pre-seeds entries for every faction that
  *declares* a profile, even with zero runs of contribution — so the
  analyst sees "declared but never fired" explicitly rather than as
  silent omission.

- Validation rejects nine silent-no-op shapes at scenario load:
  empty `[utility.terms]`, NaN / non-finite term weights, zero
  `time_horizon_ticks`, duplicate trigger ids, empty trigger
  adjustments, NaN / non-finite trigger multipliers, out-of-range /
  NaN `MoraleBelow` / `MoraleAbove` / `TensionAbove` thresholds,
  out-of-range `StrengthLossFraction` / `AttributionAgainstSelf`,
  negative / NaN `ResourcesBelow` threshold, and negative / NaN
  `TickFraction` (values >1 are *valid* when `time_horizon_ticks`
  shrinks the denominator below `max_ticks`). Mirrors the
  load-time-fail-loud pattern from every prior round.

- New `## Utility Decomposition` report section in
  `crates/faultline-stats/src/report/utility_decomposition.rs`. Two
  sub-tables: per-faction term means (canonical `UtilityTerm`
  declaration order, not alphabetic) and per-trigger fire rates
  (gated on at least one declared trigger). Elides when
  `summary.utility_decompositions` is empty.

- Bundled archetype: `scenarios/adaptive_utility_demo.toml`. Two
  factions with contrasting profiles (`red`: control-maximizing
  aggressor with deadline-pressure trigger; `blue`: cautious
  defender with morale-panic trigger) on a 4-region square.
  Demonstrates the full mechanic end-to-end. Demo run shows red's
  control mean at +1.04 and time_to_objective at +0.63, vs. blue's
  at +0.12 and +0.08 — a clean signal that the profiles drove the
  AI behavior.

- Backward-compat: scenarios without `[utility]` see no change.
  Adding `[utility]` doesn't shift the RNG sequence — the utility
  evaluator is RNG-free, so the sequence of `r#gen` calls in
  `evaluate_attack_actions` is unchanged. All 20 pre-existing
  bundled scenarios verify deterministically with their prior
  `output_hash`; the new `adaptive_utility_demo.toml` is the
  21st bundled scenario.

- Coverage:
  - `crates/faultline-engine/src/utility.rs::tests` (6 tests):
    base-weights round-trip, unmatched triggers don't adjust,
    matched triggers multiply, multiple triggers compose,
    time-horizon override shrinks the TickFraction denominator,
    StrengthLossFraction fires after measurable loss.
  - `crates/faultline-engine/src/lib.rs::tests` (8 validation
    tests): well-formed profile passes; empty terms, NaN weight,
    zero horizon, duplicate trigger ids, empty adjustments,
    threshold > 1 on MoraleBelow, negative ResourcesBelow,
    NaN adjustment multiplier all rejected.
  - `crates/faultline-engine/tests/utility_adaptive_ai.rs` (7
    integration tests): legacy no-profile path unchanged,
    profile produces non-empty decisions, deadline trigger fires
    past midpoint, same-seed determinism, no-utility legacy
    determinism unchanged, MoraleBelow trigger fires on combat
    loss, empty term weights doesn't panic.
  - `crates/faultline-stats/src/utility_decomposition.rs::tests`
    (5 tests): empty input, faction with profile gets zeroed row
    when no runs, run-averaged means, fire-rate computation,
    determinism.
  - `crates/faultline-stats/src/report/utility_decomposition.rs::tests`
    (4 tests): elision on empty, full render with static profile,
    trigger fire-rate sub-table, em-dash for missing terms.
  - `crates/faultline-types/src/tests.rs` (3 tests): TOML
    roundtrip for the full profile, legacy scenario loads with
    `Faction.utility == None`, each `AdaptiveCondition` variant
    roundtrips through serde.

## Calibration confidence in methodology (Epic N round-two — partial)

R3-3-style polish on Epic N round-one: the `## Methodology &
Confidence` section now surfaces a per-scenario `Calibration
confidence` tag — `[H] Pass`, `[M] Marginal`, `[L] Fail` — when the
scenario declares a `[meta.historical_analogue]` and the run set is
non-empty. The tag mirrors the per-observation roll-up shown in the
standalone Calibration section but lives in the methodology
appendix where the analyst is reading about how to interpret the
numbers — making the calibration claim load-bearing on the same
page as the parameter-defensibility claim.

Wired in `crates/faultline-stats/src/report/methodology.rs`. The
`Methodology` section's render method now reads
`scenario.meta.historical_analogue` + `summary.calibration` to
decide whether to emit the tag; the synthetic-disclaimer / no-runs
paths are still handled by the standalone Calibration section
above. A new prose paragraph in the appendix explains how
calibration confidence relates to the parameter-defensibility tag
in the header — they answer different trust questions and a
scenario can in principle Pass one and Fail the other.

This closes one of the two round-two N items; the other (5–10
cleanly-sourced single-event analogues for the bundled scenario
set) remains deferred per the priority list — single-event analogue
research is per-scenario work, not a framework change.

## Property tests (R3-5)

Determinism + seeded RNG is the substrate property tests are designed
for, so R3-5 added a `proptest`-backed suite covering the four modules
that handle RNG or compute statistical bounds. The tests live alongside
the existing fixed-seed integration tests; they pin *invariants* across
the whole input space rather than checking specific outputs at one
seed.

- `crates/faultline-engine/tests/property_invariants.rs` — for any
  seed: faction `total_strength` ≥ 0 across every snapshot, faction
  morale stays in `[0, 1]`, tension stays in `[0, 1]`, and two engine
  runs at the same seed produce bit-identical `RunResult` JSON
  (the determinism contract `--verify` depends on). Uses the bundled
  `tutorial_symmetric.toml` scenario via `include_str!` to exercise a
  realistic engine path; proptest budget tightened to 16 cases so the
  4 properties × ~100 ticks each finish under a second.
- `crates/faultline-stats/tests/property_uncertainty.rs` — Wilson
  bounds always contain the point estimate, narrow monotonically with
  sample size, and `wilson_from_rate` agrees with the count form.
  Bootstrap CI is bit-identical for the same `(values, seed)` and
  always satisfies `lower ≤ upper`.
- `crates/faultline-stats/tests/property_network_metrics.rs` —
  disrupting nodes or zeroing edges never *increases* max-flow on a
  static topology (the example invariant from `improvement-plan.md`),
  max-flow is always non-negative and finite, and Brandes betweenness
  scores stay in `[0, 1]` with descending-rank output.
- `crates/faultline-stats/tests/property_search.rs` — same
  `search_seed` ⇒ bit-identical `SearchResult` JSON across random
  seeds, every trial's continuous assignments stay in `[low, high]`,
  every grid-mode trial hits one of the enumerated `enumerate_levels`
  values, and the Pareto frontier is strictly ascending and in-bounds.
  Uses an inline minimal scenario fixture (two regions, two factions,
  30 max ticks, `num_runs = 2`) so the engine path stays fast under
  proptest's 24-case budget.

The properties are intentionally lightweight: each test files runs
in well under a second even though several involve full engine runs
per case, because the fixtures are scenario-minimal. Adding a new
property is one new `#[test]` inside an existing `proptest!` block —
no scaffolding required since `proptest` is already a workspace
dev-dependency on every relevant crate.

**Why these properties.** The May 2026 priority refresh promoted
property tests to the top-five priority list specifically because the
seeded-RNG / `BTreeMap`-iteration determinism contract is exactly the
invariant property tests are good at pinning. Fixed-seed integration
tests catch the failure mode "this output is wrong at seed 42"; they
don't catch "an unrelated refactor introduced a `HashMap` somewhere
in the trial pipeline and now run-to-run output is non-deterministic
under most seeds." Property tests do.

## Code Style

- Rust Edition 2024. Formatting enforced by `rustfmt.toml`: 100-char max line width, 4-space indentation, Unix newlines, `Tall` fn params layout.
- Run `cargo fmt --all` before committing. CI rejects unformatted code.
- Clippy warnings treated as errors in CI: `cargo clippy --all-targets -- -D warnings`.
- Workspace-level lints in root `Cargo.toml`: `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented`, `clippy::clone_on_ref_ptr` are warnings; `clippy::unwrap_used` is deny. `unsafe_op_in_unsafe_fn` is a warning.
- **No `unwrap()` anywhere** — including tests. Use `expect("descriptive reason")` instead.
- Edition 2024: `gen` is a keyword — use `r#gen` for random generation calls.

## Workspace Structure

```
crates/
  faultline-types/       # Shared data structures (zero logic, leaf crate)
  faultline-geo/         # Geography, maps, terrain (depends on: types)
  faultline-tech/        # Technology capability cards (depends on: types)
  faultline-politics/    # Political climate, loyalty (depends on: types)
  faultline-events/      # Event system (depends on: types)
  faultline-engine/      # Core simulation loop (depends on: types, geo, tech, politics, events)
  faultline-stats/       # Monte Carlo runner (depends on: engine, types)
  faultline-backend-wasm/# Browser WASM frontend (depends on: engine, stats, types)
  faultline-cli/         # Headless CLI runner (depends on: engine, stats, types)
```

## Architecture

- **Determinism is non-negotiable.** Same config + same seed = identical output on native and WASM. Uses `ChaCha8Rng`. Use `BTreeMap` for deterministic iteration (never `HashMap`).
- **No `unwrap()`.** Workspace-level `clippy::unwrap_used = "deny"`. All error paths must be handled.
- **WASM-compatible engine.** `faultline-engine` must compile to `wasm32-unknown-unknown`. No `std::fs`, no `std::thread`, no `rayon` in the engine crate. Parallelism lives only in `faultline-cli` (rayon) and `faultline-backend-wasm` (web workers).
- All IDs are newtypes wrapping `String` (defined via `define_id!` macro in `faultline-types/src/ids.rs`).
- All config structs derive `Serialize, Deserialize, Clone, Debug`.
- Technology modifiers are "capability cards" — named bundles of statistical effects derived from OSINT.
- Scenarios are TOML files in `scenarios/`. The browser app reads them via `site/scenarios/`, which is a symlink to `../scenarios` so the source of truth lives in one place. The GitHub Pages deploy workflow materializes the symlink (replaces it with a real copy) before uploading the artifact, since the upload only includes `site/`.
- The browser tech-card library at `site/js/app/tech-library.js` records each card's open-source provenance via `source_ref` (a domain-generic descriptor — *not* a citation to any specific publication). Adding a card with a section-level fingerprint to a specific external document will fail the grep-guard CI stage.

## Scenario Data Policy

Faultline models aggregate statistical effects of real-world systems. When writing or reviewing scenarios:

- **All capability parameters must be sourceable from public OSINT** (IISS Military Balance, CRS reports, congressional testimony, published defense analyses, academic literature).
- **Describe effects, not implementations.** A tech card says "detection range 300km against 1m² RCS" (published spec), not "use X-band phased array with Y waveform" (technical data).
- **No classified, CUI, or export-controlled information.** If you can't find it in a public source, don't include it.

## Security Considerations

- No OpenAI/Codex integrations — disabled due to security concerns (government surveillance partnerships).
- No Google/Gemini integrations — same concerns.
- PR reviews use Claude Code (security + quality profiles) and Qwen 3.5 via OpenRouter.

## CI/CD Pipeline

Two GitHub Actions workflows on self-hosted runners:

- **`main-ci.yml`** — Runs on main push and tags. CI stages (fmt, clippy, test, build, cargo-deny), WASM build via wasm-pack, GitHub Pages deployment. Auto-creates GitHub issues on failure.
- **`pr-validation.yml`** — Runs on PRs. CI stages + Claude Code AI review (security + quality profiles) + OpenRouter/Qwen 3.5 general review + automated agent fix iterations (max 5, extendable with `[CONTINUE]` comment). Add `no-auto-fix` label to disable automated fixes.

Agent commit authors: `AI Review Agent`, `AI Pipeline Agent`, `AI Agent Bot`.

## Known Advisory Exemptions

One advisory is currently exempted in `deny.toml`:

- `RUSTSEC-2026-0097` — rand 0.8 unsound only when a custom logger calls `rand::rng()` and `ThreadRng` reseeds inside that logger. Faultline uses `tracing` (not `log`) and never calls rand from a logging context. Upgrading to rand 0.9+ requires coordinated updates across `rand_chacha`, `rand_distr`, `statrs`, and `nalgebra` and is planned for a future release.

`cargo deny check` otherwise passes clean.
