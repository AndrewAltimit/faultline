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
# Note: alliance fracture is currently *analytical accounting* — the
# engine does not yet consume the post-fracture stance for combat /
# AI / victory dynamics. The fracture is observable in the report and
# in `RunResult.fracture_events`, not in tick-level run behavior.
cargo run -p faultline-cli -- scenarios/coalition_fracture_demo.toml -n 32

# Replay a saved manifest and assert bit-identical output (Epic Q)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml \
    --verify ./output/manifest.json

# Migrate a scenario forward to the current schema version (Epic O)
# Prints to stdout by default; --in-place rewrites the source file.
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate --in-place

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

**Scope caveat: analytical accounting only.** The current engine
does not consume `Diplomacy` stance for downstream effects — combat
targeting (`tick::find_contested_regions`) treats every co-located
faction as a combatant regardless of stance, and the AI / victory-
check / political phases never consult diplomacy. A fracture is
observable in `RunResult.fracture_events`, in the cross-run rollup
on `MonteCarloSummary.alliance_dynamics`, and in the rendered
report's `## Alliance Dynamics` section, but not in tick-level run
dynamics. Treat fire rates as scenario-design diagnostics rather
than live behavioral predictions. A future round will add behavioral
coupling (combat targeting respecting `Diplomacy::Allied`, AI
de-prioritizing Cooperative neighbors).

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
