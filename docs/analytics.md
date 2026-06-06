# Faultline Analytics Reference

This document describes what Faultline computes *across* runs and how
results are surfaced: the Monte Carlo report sections, the cross-run
analytics producers, and the higher-order analysis modes (strategy
search, defender-posture optimization, robustness, adversarial
co-evolution, calibration, and scenario explain).

For command invocations see `docs/cli.md`. For scenario schema see
`docs/scenario_schema.md`.

---

## Monte Carlo report sections

Every `cargo run -p faultline-cli -- <scenario> -n <N>` run produces a
`report.md` in the output directory. The report contains exactly 28
sections rendered in this fixed order:

1. **Header** — scenario name, author, version, schema version, tags,
   author confidence, and prose description. Always emits.
2. **Win Rates** — per-faction win rate with Wilson 95% confidence
   intervals. Elides when `total_runs == 0`.
3. **Continuous Metrics** — mean/stdev/min/max of scalar run outputs
   (duration, casualties, spend). Elides when empty.
4. **Feasibility** — kill-chain phase feasibility matrix (completion
   probability × detection probability per phase). Elides when no
   kill chains are declared.
5. **Phase Breakdown** — per-phase firing counts, success rates,
   attribution and cost distributions. Elides when empty.
6. **Time Dynamics** — per-chain time-to-first-detection
   (right-censored when never detected), defender-reaction-time
   distribution (gap from first detection to run end), and per-phase
   Kaplan-Meier survival curves with cumulative hazard. Elides when
   the chain produces no signal.
7. **Pareto Frontier** — non-dominated runs across (attacker cost,
   success, stealth = `1 - max chain detection`). Surfaces the
   achievable trade-off envelope before reaching for a sweep. Elides
   when fewer than two runs exist or all runs are dominated.
8. **Correlation Matrix** — Pearson correlations across the six
   built-in per-run scalars: duration, casualties, attacker spend,
   defender spend, mean attribution, max detection. Constant series
   show as `—` (correlation undefined; deliberately not zero). Elides
   when the matrix is empty.
9. **Defender Capacity** — per-role queue utilization, time-to-
   saturation, mean shadow detections, and (when present)
   cross-role escalation spillover in/out counts. Elides when no
   faction declares `defender_capacities`.
10. **Network Resilience** — per-network mean/max disrupted-node and
    component counts, fragmentation rate, and the top-N critical-node
    ranking by Brandes betweenness centrality on the static topology.
    Elides when no `[networks.*]` are declared.
11. **Supply Pressure** — per-faction mean/min supply pressure and
    pressured-tick counts when at least one `kind = "supply"` network
    with an `owner` is declared. Elides otherwise.
12. **Tech Costs** — per-faction deployment spend, maintenance spend,
    denied cards, and decommissioned cards. Elides when no faction
    engages the cost mechanic.
13. **Seam Analysis** — cross-chain and cross-phase seam detection
    (timing gaps, sequencing anomalies). Elides when empty.
14. **Regional Control** — per-region per-faction control distribution
    across runs. Elides when empty.
15. **Low Confidence** — every author-flagged `ConfidenceLevel::Low`
    cell (scenario-level confidence, per-phase `parameter_confidence`,
    per-phase-cost `confidence`) gathered in one place. Elides when no
    low-confidence flags are set.
16. **Policy Implications** — scenario-authored policy recommendation
    text. Elides when absent.
17. **Countermeasure** — defender-side countermeasure effectiveness
    summary. Elides when empty.
18. **Environment Schedule** — summary of `[[environment.windows]]`
    schedules and their per-tick coverage. Elides when no windows are
    declared.
19. **Leadership Disruption** — per-faction decapitation strike
    statistics and command-effectiveness degradation trajectories.
    Elides when no faction declares a `leadership` cadre.
20. **Alliance Dynamics** — per-rule fracture fire rate, mean fire
    tick, and terminal-stance distribution. Elides when no faction
    declares `alliance_fracture`.
21. **Narrative Dynamics** — per-faction information dominance
    (mean/max dominance ticks, mean peak score) and per-narrative
    trajectory (firing rate, mean peak strength, modal favored
    faction). Elides when no `MediaEvent` effects fire across the run
    set.
22. **Civilian Activations** — per-segment activation rate, mean
    activation tick, and per-action firing counts. Elides when no
    `population_segments` are declared. A segment declared but never
    activated still appears ("declared but never tripped").
23. **Displacement** — per-region peak/mean/terminal displaced
    fraction and total inflow/outflow. Elides when no `Displacement`
    effects or `Flee` actions produce displacement.
24. **Utility Decomposition** — per-faction mean per-term utility
    contribution averaged across selected actions, and per-trigger
    fire rates. Elides when no faction declares a `[utility]` block.
25. **Belief Asymmetry** — per-faction mean force-strength error,
    deception event counts, and terminal-deceived belief counts.
    Elides when the scenario does not opt into `belief_model`. A
    round-two **Belief fidelity** sub-section (mean belief confidence,
    ambient-intel pickups, terminal `Inferred` belief counts) appears
    only when there is round-two activity (any `AmbientIntel` pickup or
    `Inferred` belief), keeping round-one scenarios' output unchanged.
26. **Attribution Fidelity** — believed-attribution analytics (Epic M
    round-two): the cross-run misattribution rate (fraction of
    detection-time attribution rolls where the defender fingered the
    wrong faction), the deception-driven rate (how much a planted
    false flag drove it), the fracture-misattribution count
    (misattributions that broke an alliance against an innocent ally),
    and the `true → believed` confusion-pair table. Elides unless the
    scenario opts into `belief_model.believed_attribution` and at least
    one detection fired a roll, so non-opted-in scenarios are unchanged.
27. **Calibration** — back-testing verdict against a declared
    `[meta.historical_analogue]` (Pass/Marginal/Fail per observation
    plus a roll-up), or a "purely synthetic" disclaimer when no
    analogue is declared. **Always emits.**
28. **Methodology & Confidence** — explanation of statistical methods
    (Wilson score intervals, bootstrap CIs) and a per-scenario
    calibration-confidence tag (`[H] Pass`, `[M] Marginal`, `[L]
    Fail`) when an analogue is declared and runs are available.
    **Always emits.**

### Report module layout

The Markdown renderer lives in `crates/faultline-stats/src/report/`.
It is decomposed one file per section:

```
crates/faultline-stats/src/report/
  mod.rs                 — public API, ReportSection trait, monte_carlo_sections() array
  <section>.rs           — one file per Monte Carlo section (27 total)
  coevolve.rs            — render_coevolve_markdown (co-evolution report type)
  comparison.rs          — render_comparison_markdown (counterfactual/comparison)
  robustness.rs          — render_robustness_markdown (robustness report type)
  search.rs              — render_search_markdown (search/optimization report type)
  util.rs                — shared helpers: escape_md_cell, fmt_scalar, confidence_word
  test_support.rs        — empty_summary / minimal_scenario fixtures (cfg(test) only)
```

The `ReportSection` trait is:

```rust
pub trait ReportSection {
    fn render(&self, summary: &MonteCarloSummary, scenario: &Scenario, out: &mut String);
}
```

Each section struct implements the trait and owns its own elision
logic. The composer (`render_markdown`) simply iterates
`monte_carlo_sections()` — a `[&'static dyn ReportSection; 28]` array
— and calls `render` on each entry. The composer never grows
conditional chains.

**To add a new section:** create one file in `report/` with a unit
struct implementing `ReportSection`, declare it in `mod.rs`, and add
one entry at the desired position in the `monte_carlo_sections()`
array.

**Determinism contract:** the rendered Markdown is part of the
manifest content hash (via `--verify`). Changing section ordering,
adding any unconditional output to an existing data section, or
inserting a new unconditional section all change every bundled
scenario's `output_hash` and break `--verify`. The `verify-bundled`
CI step catches this automatically. Only `Header`, `Calibration`, and
`Methodology` are permitted to emit for empty inputs.

---

## Built-in cross-run analytics

All producers live in `crates/faultline-stats/src/`. They are pure
functions of `RunResult` data — they never re-run the engine.
Schemas for output types live on `MonteCarloSummary` / `CampaignSummary`
in `crates/faultline-types/src/stats.rs`.

### Time & Attribution Dynamics

Producer: `crates/faultline-stats/src/time_dynamics.rs`

- **Time-to-first-detection** — per kill-chain, right-censored when the
  chain was never detected across the run. The censoring flag is
  explicit so downstream consumers can distinguish "was always stealthy"
  from "data not yet collected."
- **Defender reaction time** — distribution of the gap between the first
  detection tick and the run-end tick. Measures how much response time
  the defender had after attribution.
- **Kaplan-Meier survival curves** — per-phase, with cumulative hazard.
  Surfaces which phases are the "kill points" across the MC distribution
  rather than at a single point estimate.

All three elide at the section level when the chain produces no signal.

### Pareto Frontier

Producer: `crates/faultline-stats/src/time_dynamics.rs` (assembled in
the MC aggregator in `lib.rs`)

Non-dominated runs across three objectives:

- **Attacker cost** (minimize)
- **Success** (maximize, binary per run)
- **Stealth** = `1 − max chain detection` (maximize)

The frontier surfaces the achievable trade-off envelope — a run that is
strictly worse on all three objectives than some other run is dominated
and excluded. Use `--search` to actively optimize the space rather than
just observe the natural frontier.

### Output Correlation Matrix

Producer: `crates/faultline-stats/src/time_dynamics.rs`

Pearson correlations across six per-run scalars: run duration,
total casualties, attacker spend, defender spend, mean attribution, max
detection. Constant series (variance zero across the run set) show as
`—` — correlation is undefined for a constant, not zero. This prevents
the matrix from implying spurious independence.

### Morris Elementary-Effects Screening

Producer: `crates/faultline-stats/src/morris.rs`

An implementation of Morris (1991) variance-decomposition screening.
For each trajectory through the parameter space, every parameter is
perturbed once by a fixed step `Δ` and the resulting elementary effect
`EE_i = (y(x + Δ e_i) − y(x)) / Δ` is recorded. With `R` trajectories
the summary statistics are:

- `mu_star` — mean of absolute elementary effects (first-order importance
  ranking; high = parameter moves output a lot on average)
- `sigma` — stdev of elementary effects (high = non-linear or interacting;
  low with high `mu_star` = roughly additive)

Total simulation cost is `R × (k + 1)` runs (where `k` = number of
parameters), the same order as `k` separate sensitivity sweeps but the
output is variance-decomposable. Morris is the standard screening stage
before Sobol (which requires `N(2k + 2)` runs).

Morris screening is **not currently CLI-exposed**; it is callable by
library consumers via `MorrisConfig` + `run_morris`. The trajectory
layout is seeded from a caller-supplied seed via `ChaCha8Rng` for
determinism.

### Uncertainty: Wilson Score and Bootstrap CIs

Producer: `crates/faultline-stats/src/uncertainty.rs`

Two CI methods:

- **Wilson score intervals** — closed-form 95% CIs on rate-valued
  quantities (win rates, detection rates). Robust at low sample counts;
  always contain the point estimate; narrow monotonically with sample
  size.
- **Bootstrap CIs** — for scalar distributions where the Wilson closed
  form doesn't apply. Bit-identical for the same `(values, seed)`.

### Network Metrics

Producer: `crates/faultline-stats/src/network_metrics.rs`

Cross-run aggregation of per-tick `NetworkSample` data:

- Mean/max disrupted-node and component counts across the run set
- Fragmentation rate (fraction of runs where the network split)
- **Critical-node ranking** — top-N nodes by Brandes betweenness
  centrality on the *static* (initial) topology, treating the graph as
  undirected. Removing the most-central node is the highest-leverage
  interdiction regardless of directionality.
- **`max_flow`** — Edmonds-Karp max-flow between arbitrary node pairs,
  deterministic via `BTreeMap`-ordered BFS.
- **`mean_infiltration_per_faction`** — helper for scenarios with
  `NetworkInfiltrate` event effects.

---

## Strategy search

Source: `crates/faultline-stats/src/search.rs`,
`crates/faultline-types/src/strategy_space.rs`

Scenarios that declare a `[strategy_space]` block can be evaluated
under `--search` mode. The runner samples assignments to decision
variables (continuous or discrete) using one of two methods:

- `random` — uniform random sampling from each variable's domain.
  Trial count equals `--search-trials`.
- `grid` — Cartesian-product grid. Continuous variables expand into
  `steps` evenly-spaced values (inclusive endpoints); discrete
  variables enumerate their `values`. The first `--search-trials`
  cells of the product are evaluated in odometer order (last variable
  cycles fastest).

Each trial assignment is applied to the scenario via the `set_param`
path grammar, then evaluated with a full MC run at `--search-runs`
iterations.

**Seeding model.** Two seeds are deliberately separated:

- `--search-seed` drives sampling of decision-variable assignments.
  Same seed + space + method always produces the same trial list.
- `--seed` (inner MC seed) is **identical across all trials**, so
  trial-to-trial deltas are pure parameter-change effects, not
  sampling noise.

**Objectives.** Round-one objectives are derived from existing
`MonteCarloSummary` / `CampaignSummary` shape. Manifests record
objective *labels* (not the structured enum) so adding new variants
stays additive. See `docs/scenario_schema.md` under `[strategy_space]`
for the full objective list.

**Outputs.** For each objective, the runner reports the best-scoring
trial. Across all objectives, it reports the non-dominated
(Pareto-optimal) frontier.

Bundled archetype: `scenarios/strategy_search_demo.toml`.

---

## Defender-posture optimization

Source: `crates/faultline-stats/src/search.rs` (same runner as search)

Builds on strategy search by adding defender-aligned `SearchObjective`
variants:

- `MaximizeAttackerCost`
- `MaximizeDetection`
- `MinimizeDefenderCost`
- `MinimizeMaxChainSuccess`

These compose with the existing attacker-aligned objectives so a single
`[strategy_space]` declaration can express either side's optimization.

The `set_param` path layer is extended to reach
`faction.<id>.force.<force_id>.{strength,mobility,upkeep}` so force
posture is a first-class decision variable.

**Baseline trial.** Search runs compute an optional "do-nothing"
baseline trial alongside sampled trials (controlled by
`SearchConfig.compute_baseline`, default `true` in the CLI). The
**Counter-Recommendation** report section ranks Pareto-frontier trials
by per-objective improvement against the baseline, with
direction-aware "improvement?" tags and Wilson 95% CIs on rate-valued
objectives. The section gates on `baseline + at least one decision
variable with owner set`, so legacy attacker-only spaces are
unaffected.

`ManifestMode::Search` records `compute_baseline` so `--verify`
replays match.

Bundled archetype: `scenarios/defender_posture_optimization.toml`.

---

## Defender-posture robustness

Source: `crates/faultline-stats/src/robustness.rs`

Given a set of defender postures (typically the Pareto frontier of a
prior `--search` run) and a library of named attacker profiles declared
in `[strategy_space.attacker_profiles]`, the robustness runner
evaluates every **(posture × profile)** cell via Monte Carlo and
surfaces per-posture rollups:

- **Worst** — the profile under which this posture performs worst
  (direction-aware: for `MinimizeMaxChainSuccess`, "worst" is the
  largest cell value)
- **Best** — the profile under which this posture performs best
- **Mean** and **stdev** across all profiles

**Expected analyst workflow:** search → robustness. First identify
Pareto-optimal postures against a single attacker baseline, then
re-rank them by worst-case profile to surface which postures are
fragile to which attacker strategies.

**Determinism.** The robustness runner has no RNG of its own. Postures
are iterated in caller-supplied order; profiles are iterated in
scenario declaration order (`BTreeMap`). Every cell reuses the same
inner MC seed — cell-to-cell deltas reflect parameter changes only.

**Manifest integrity.** `ManifestMode::Robustness` records the full
posture list inline plus the SHA-256 of the source `search.json` (when
one was supplied via `--robustness-from-search`). `--verify` refuses a
stale source file, so the search phase and the robustness phase are
cryptographically linked.

Bundled archetype: `scenarios/defender_robustness_demo.toml`.

---

## Adversarial co-evolution

Source: `crates/faultline-stats/src/coevolve.rs`,
report renderer: `crates/faultline-stats/src/report/coevolve.rs`

Layers an alternating best-response loop on top of `run_search`. Each
round, one side ("mover") re-optimizes only the variables it owns
against the opponent's currently-frozen assignment via a sub-search.

All `[strategy_space]` variables must declare `owner = "<faction>"`
matching either `--coevolve-attacker` or `--coevolve-defender`;
un-owned or mis-owned variables are rejected at validation.

**Termination conditions:**

- **Nash equilibrium** — the joint `(attacker, defender)` assignment
  matches the prior round (convergence in pure strategies on the
  discrete strategy space the search visits). Distance-1 match.
- **Cycle** — the joint state recurs at any prior position at distance
  `period >= 2`. The reported `period` is the shortest matching
  distance. In alternating-mover play, the smallest realistic cycle
  period is 4.
- **`NoEquilibrium`** — `--coevolve-rounds` reached without either
  signal.

**Triple-seeding model:**

- `--coevolve-seed` drives per-round sub-search sampling via
  `coevolve_seed.wrapping_add(round_index)`, so each round's sampler
  is independent of the next but reproducible from one seed.
- Inner MC seed (from `--seed`) is identical across rounds and across
  trials — round-to-round deltas are pure parameter-change effects.
- The per-round `SearchConfig.search_seed` is derived deterministically
  from the coevolve seed; it is never user-supplied directly.

**CI integration.** A `COEVOLVE <status> rounds=N manifest_hash=...`
line is printed to stdout for CI scripts. `ManifestMode::Coevolve`
records all per-side knobs so `--verify` replays bit-identically.

Bundled archetype: `scenarios/coevolution_demo.toml`.

---

## Calibration

Source: `crates/faultline-stats/src/calibration.rs`,
report section: `crates/faultline-stats/src/report/calibration.rs`

### Historical-analogue back-testing

Scenarios may declare a `[meta.historical_analogue]` block with:

- `name`, `description`, `period` (free-form date label)
- `sources` — required non-empty open-source citations
- `confidence` — author confidence in the analogue *fit*
- One or more `observations` (see below)

The calibration module computes a verdict for each observation by
comparing MC output distributions against declared thresholds.

**`HistoricalMetric` variants and verdict ladder:**

| Metric | Pass | Marginal | Fail |
|---|---|---|---|
| `Winner { faction }` | observed faction is MC modal *and* MC mass ≥ 0.50 | observed faction is MC modal *or* MC mass ≥ 0.25 | otherwise |
| `WinRate { faction, low, high }` | MC point estimate ∈ `[low, high]` | Wilson 95% CI overlaps `[low, high]` | otherwise |
| `DurationTicks { low, high }` | ≥ 50% of MC `final_tick` values in interval | ≥ 25% in interval | otherwise |

Overall verdict = worst per-observation verdict. Calibration claims
compose as ANDs, not ORs.

`compute_calibration` is a pure function of `(scenario, runs,
win_rates)` in `crates/faultline-stats/src/calibration.rs`. Output
lives on `MonteCarloSummary.calibration: Option<CalibrationReport>`.
Serialization is skipped when `None` so legacy-scenario manifest
hashes are unaffected.

**Report section behavior.** The `Calibration` section always emits:

- Scenario has analogue + runs available: analogue header (name,
  period, description, sources) + per-observation table + roll-up.
- Scenario has analogue but no MC runs: analogue header +
  "no MC runs available" disclaimer.
- Scenario has no analogue: synthetic-scenario disclaimer explaining
  what absence means for result interpretation.

### Calibration confidence in `## Methodology`

The `## Methodology & Confidence` section surfaces a
**Calibration confidence** tag — `[H] Pass`, `[M] Marginal`,
`[L] Fail` — when the scenario declares a `historical_analogue` and
the run set is non-empty. The tag mirrors the per-observation roll-up
in the standalone Calibration section but lives in the methodology
appendix where the analyst is reading about how to interpret the
numbers.

A prose paragraph in the appendix explains how calibration confidence
relates to the parameter-defensibility tag in the header — they answer
different trust questions and a scenario can in principle Pass one and
Fail the other.

Wired in `crates/faultline-stats/src/report/methodology.rs`.

Bundled archetype: `scenarios/calibration_demo.toml`.

---

## Scenario explain

Source: `crates/faultline-stats/src/explain.rs`

`--explain` produces a structured "what does this scenario actually
model?" summary without running the engine. Pure schema view — no RNG,
no simulation, no I/O beyond reading the scenario file.

**`ExplainReport` structure:**

```rust
pub struct ExplainReport {
    pub meta: ExplainMeta,         // name, author, version, schema version, tags, confidence
    pub scale: ExplainScale,       // counts: factions, regions, kill chains, phases, networks
    pub factions: Vec<ExplainFaction>,
    pub kill_chains: Vec<ExplainKillChain>,
    pub victory_conditions: Vec<ExplainVictory>,
    pub networks: Vec<ExplainNetwork>,
    pub strategy_space: ExplainStrategySpace,  // decision-variable surface
    pub low_confidence: Vec<ExplainLowConfidence>,
}
```

**Rendered Markdown section sequence:**

1. Header (name, author, version, schema version, tags, author
   confidence, prose description)
2. Scale (faction / region / kill-chain / phase / network counts)
3. Factions
4. Kill chains
5. Victory conditions
6. Networks
7. Decision-variable surface — answers "which parameters does this
   scenario move under `--search` / `--coevolve` / `--robustness`?"
8. Low-confidence parameters — every author-flagged `Low` cell gathered
   in one place so the analyst sees up-front which assumptions a
   counterfactual would have to push on

**API.** Two public functions in `explain.rs`:

```rust
pub fn explain(scenario: &Scenario) -> ExplainReport
pub fn render_markdown(report: &ExplainReport) -> String
```

Both are reusable by tooling beyond the CLI without dragging in the
simulation engine. The browser editor's **Explain** button (Epic P)
calls them directly through the WASM export `explain_scenario_wasm`,
which returns `{ markdown, report }` so the in-app panel renders the
same Markdown the CLI emits.

Output format is Markdown by default; pass `--explain-format json` for
the structured `ExplainReport` serialization.

## Advisory warnings

Source: `crates/faultline-stats/src/warnings.rs`

`collect_warnings(&Scenario) -> WarningReport` runs a set of *non-fatal*
advisory checks, distinct from `faultline_engine::validate_scenario`
(which returns hard, load-blocking errors). A scenario that trips an
advisory check still loads and runs — the finding flags a likely
modelling mistake the author may want to fix. Pure function over
`Scenario`: no RNG, no engine, no I/O, and deliberately **not** injected
into the deterministic Markdown report (so it never affects a bundled
scenario's `output_hash`).

Checks (each a `WarningKind`):

1. `FactionNoObjective` — a faction named by no victory condition has no
   modelled path to win.
2. `UnreferencedRegion` — a region declared on the map that no force,
   victory condition, infrastructure node, terrain modifier, kill-chain
   output, or neighbour `borders` list references.
3. `UnreachablePhase` — a kill-chain phase unreachable from the chain's
   `entry_phase` via the branch graph (a dangling `entry_phase` is left
   to the hard validator and does not flag every phase).

The browser editor surfaces these in an inline advisory panel (Epic P)
via the WASM export `scenario_warnings_wasm`, which serializes the
`WarningReport` (`{ warnings: [ { kind, subject, message } ] }`). The
check logic lives in `faultline-stats` so it is testable in Rust and
reusable by the CLI.
