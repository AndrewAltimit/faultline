# Faultline Improvement Plan

Living tracker for the comprehensive review work on branch
`review/comprehensive-improvements` (and sub-branches merged into it).
Each epic is independently shippable; PRs should close to `main` as
epics complete, not to this branch.

The plan is derived from a three-angle audit performed in April 2026
(engine analytics, frontend/UX, scenario content — ~190 findings
total). This document only names the **cross-cutting themes and
ordered epics**; individual findings live in the audit reports in
the branch's review conversation.

---

## Cross-cutting themes

Three themes surfaced independently in all three audits and form the
spine of the work:

1. **Uncertainty is implicit, not first-class.** Parameters are point
   estimates, CIs use ad-hoc Wald approximations, reports don't
   explain what `[H]`/`[M]`/`[L]` mean, and scenario authors can't
   flag "this number is a low-confidence expert estimate."
2. **No counterfactual / comparative workflow.** "If the defender
   deployed X, success drops to Y" requires hand-editing TOML and
   re-running. Missing at every layer: schema, engine, UI, report.
3. **Attribution and time dynamics are underdeveloped.** Detection
   accumulates but we have no time-to-first-detection histogram, no
   hazard curves, no IWI/IOC library, no escalation ladder, no
   hysteresis in branch conditions, no de-escalation phase.

---

## Epics

Sequencing favors **highest analytical leverage with lowest visual
risk first** — the tool gets more rigorous before it gets flashier,
so the flash lands on a substrate worth showing.

### Epic A — Uncertainty as a first-class citizen

Foundation for everything else. Without proper CIs and confidence
tagging, downstream comparisons are suspect.

- [x] `Confidence` tags on `PhaseCost`, `CampaignPhase` (serde-optional)
- [x] Replace Wald CI in `analysis.rs::confidence_from_rate` with
      Wilson score interval
- [x] Bootstrap CI helper for continuous metrics (duration,
      casualties, cost asymmetry) — available in
      `faultline_stats::uncertainty::percentile_bootstrap_ci`; not yet
      wired into the report for continuous metrics
- [x] Wilson CI bounds surfaced on `FeasibilityRow`
- [x] Win-rate Wilson CIs in report
- [x] Methodology appendix + confidence legend in `report.md`
- [x] "Low-confidence parameters" section when authors tag any
- [x] Wilson CIs on `PhaseStats` (per-phase success / detection /
      failure / not-reached rates)
- [x] Bootstrap CIs on duration / casualties / cost-asymmetry
      distributions in the report
- [x] Metadata-level `confidence` on scenario `[meta]` (coarse
      whole-scenario tag — "this scenario is a conceptual sketch" vs
      "this is publication-ready"); feeds into an at-a-glance report
      banner

**Status:** Epic A **closed**. Two PRs landed: PR 1 (commit `44d9121`
+ hardening follow-up) shipped Wilson CIs on win rates and feasibility
cells, the confidence legend, and the low-confidence section. PR 2
(branch `epic-a-uncertainty-polish`) shipped the remaining three items
— per-phase Wilson CIs in the phase breakdown table, a Continuous
Metrics section with percentile-bootstrap CIs on the mean of every
scalar distribution (seeded from `scenario.simulation.seed` so the
report stays bit-identical under fixed inputs), and an optional
`[meta].confidence` scenario-level banner. Epic B can now proceed.

### Epic B — Counterfactual & comparative analysis

The core analyst workflow: "what if the defender had X?"

- [x] Schema: `[events.<id>.defender_options]` — cost/effect
      branches the defender can choose
- [x] Schema: `[factions.<id>.escalation_rules]` — doctrine / ROE
      enforcement (declarative; engine enforcement deferred)
- [x] Schema: `[kill_chains.*.phases.*.warning_indicators]` — IWI /
      IOC entries (declarative; does not drive the detection roll yet)
- [x] CLI: `--counterfactual <param>=<value>` mode; also
      `--compare <other.toml>` side-by-side report
- [x] Dashboard: "Pin Results" + side-by-side comparison mode
- [x] Scenario diff viewer in the TOML editor
- [x] Report: "Policy Implications" and "Countermeasure Analysis"
      sections

**Status:** Epic B **closed**. First PR landed the three schema
extensions (all `#[serde(default)]` for backwards compatibility), the
`--counterfactual` and `--compare` CLI modes built on an extended
`set_param` path layer that now reaches kill-chain phase parameters, a
new `faultline_stats::counterfactual` module producing a
`ComparisonReport` with per-faction win-rate deltas and per-chain
feasibility deltas, and the two new report sections. Second PR
(branch `epic-b-comparison-ui`) shipped the three frontend pieces:
`PinnedStore` (localStorage-backed pin manager with quota-aware
trimming), a side-by-side comparison panel that mirrors the Rust
`ComparisonReport` delta shape (win-rate deltas with Wilson CIs,
per-chain success / detection / cost-asymmetry deltas, mean-duration
delta), and a unified-diff modal in the TOML editor that diffs the
current text against the last loaded preset/import or any pinned
scenario.

### Epic C — Time & attribution dynamics

Fills the biggest analytical hole: the tool reports *that* things
happened but not *when* or *how often over time*.

- [x] `time_to_first_detection` histogram per chain
- [x] Per-phase Kaplan-Meier survival / cumulative hazard curves
- [x] Sobol / Morris variance decomposition (replacing pure OAT)
- [x] Correlation matrix (inputs ↔ outputs)
- [x] Escalation-ladder branch condition with hysteresis:
      `EscalationThreshold { metric, threshold, direction, sustained_ticks }`
- [x] Pareto frontier output (cost vs. success vs. detection)
- [x] Defender-reaction-time distribution

**Status:** Epic C **closed**. Single PR (branch
`epic-c-time-attribution`) added a new
`faultline_stats::time_dynamics` post-processing module — three
analytics families (time-to-first-detection per chain with
right-censoring, defender exposure / reaction time per chain, per-phase
Kaplan-Meier survival with cumulative hazard) hang off the existing
`CampaignSummary`, and two cross-run summaries (an output-output
Pearson correlation matrix and a non-dominated Pareto frontier over
attacker cost / success / stealth) hang off `MonteCarloSummary`. All
four are pure functions of already-collected `RunResult` data — no
re-run, no new RNG draws — so determinism is preserved and existing
manifest hashes only change because the summary schema gained new
fields. The Morris elementary-effects screening lives in a separate
`faultline_stats::morris` module: a deterministic, seeded R-trajectory
design that produces `mu_star` / `mu` / `sigma` per parameter against
one of three output metrics (duration, first-faction win rate, mean
chain success). `R(k+1)` MC batches per run versus Sobol's `N(2k+2)`
keeps it inside an interactive analyst budget while still ranking by
variance, not just by visible delta — Sobol remains a future follow-up
for parameters Morris flags. Engine-side, `BranchCondition` gained an
`EscalationThreshold { metric, threshold, direction, sustained_ticks }`
variant with hysteresis: the engine captures an escalation-metric
snapshot at the end of every tick (only when the scenario actually
references the variant — `max_escalation_window` returns `0` for
legacy chains and the snapshot is dropped immediately) and the branch
matcher checks the last `sustained_ticks` snapshots. Two end-to-end
integration tests pin the high-tension and low-tension paths. The
report renderer adds three new sections — Time & Attribution Dynamics
(TTD / reaction / KM tables), Pareto Frontier, and Output Correlation
Matrix — each elided when no chain produces signal. All 9 bundled
scenarios still verify bit-identical via the manifest determinism
contract; cargo deny / clippy / fmt / JS tests / WASM build all
clean.

### Epic D — Engine model depth

Things scenario authors want to express and can't. Pick 2–3, not
all at once — each is substantial.

- [ ] Supply-network graph + interdiction (new `supply_phase`)
- [ ] Multi-front resource contention (campaigns compete for
      defender attention)
- [x] Leadership decapitation + succession penalties
- [ ] Info-op narrative competition (so `MediaEvent` isn't
      fire-and-forget)
- [x] Weather / time-of-day modifiers on terrain
- [ ] Coalition / alliance fracture mechanic (beyond
      `Foreign.is_proxy` flag)
- [ ] Refugee / displacement flows with cross-regional propagation
- [x] `BranchCondition::OrAny` for prerequisite OR logic

**Status:** Epic D **closed** (round one). Single PR (branch
`epic-d-engine-depth`) shipped three of the seven items — the
"pick 2–3" bar this epic was scoped to. (1) `BranchCondition::OrAny`
adds an OR composition over inner conditions with short-circuit
left-to-right evaluation; the engine-side escalation-window walker
recurses through it so an `EscalationThreshold` nested in an `OrAny`
still registers its history requirement, and validation rejects an
empty `conditions` vector at load time so an unfilled author template
fails loudly instead of silently never matching. (2) Weather / time-
of-day modifiers introduce an optional global `EnvironmentSchedule`
whose windows compose multiplicatively — `Activation::Always`,
`TickRange`, and `Cycle` (with safe modular-subtraction arithmetic
that handles `phase >= period`) all serialize cleanly. Per-terrain
factors apply to combat defense; a global `detection_factor` applies
to every kill-chain phase's per-tick detection probability before
saturation gating, which naturally narrows the shadow-detection
window between unattenuated and saturated rolls. (3) Leadership
decapitation adds an optional `LeadershipCadre` on `Faction` plus a
`PhaseOutput::LeadershipDecapitation` variant that advances the rank
index, applies a one-shot morale shock, and caps the target's morale
at the new rank's effectiveness × succession_floor for the recovery
ramp; combat reads `morale` directly so the cap is observable in
Lanchester outcomes. Faction becomes leaderless when the rank index
passes the cadre — morale floors at zero and further strikes saturate
the index without going negative. The remaining four items
(supply-network graph, info-op competition, coalition fracture,
refugee flows) are deferred. All schema additions are
`#[serde(default)]` so legacy scenarios load unchanged; all 10
bundled scenarios still verify bit-identical via the manifest
determinism contract; cargo deny / clippy / fmt / verify-bundled /
verify-migration / grep-guard all clean.

### Epic E — UI identity & analytical density

Move from "generic SaaS dark-mode" to "purpose-built
defense-analysis instrument."

- [ ] Reserve the purple-blue gradient for 3 uses only (logo,
      primary CTA, key stat values) — currently used in ~10 places
- [ ] Distinctive headline font + "fault line" accent motif
      extending the favicon
- [ ] Inset shadow + border on map canvas so it reads as an
      interactive surface
- [ ] Chart polish: gridlines, axis labels, KDE overlays on
      histograms, confidence bands, colorblind-safe palette
- [ ] Radar / parallel-coordinates replacement for the dense
      feasibility table
- [ ] Map: pan/zoom, label-collision avoidance, hover tooltips
      with region stats, strength-proportional unit sizes
- [ ] Kill-chain phase overlays on the map (currently
      kill chains are invisible on the map)
- [ ] Dashboard: progress bar + cancel for long MC runs
- [ ] Export results to PNG / CSV / JSON / PDF
- [ ] Addressable run URLs: `?scenario=…&seed=…&tick=…`
- [ ] Light-mode toggle
- [ ] TOML editor: Monaco/CodeMirror with schema-aware autocomplete,
      inline validation, hover docs

**Status:** deferred — some items depend on Epic A/B/C output.

### Epic F — Scenario library & metadata

Make scenarios self-describing and rebalance the tech library.

- [ ] Extend `[meta]` with `analytical_purpose`, `scenario_type`,
      `confidence`, `osint_sources`, `red_team_profile`,
      `blue_team_posture`, `sensitivity_parameters`,
      `historical_precedent`
- [ ] Backfill all 9 existing scenarios with new metadata
- [ ] Rebalance tech library: current ratio is 29 institutional-
      erosion cards vs. ~2 SIGINT and ~1 supply-chain. Add ~40
      cards across SIGINT/HUMINT, supply-chain, SCADA/ICS,
      healthcare, GPS denial, deepfakes
- [ ] New scenarios: ransomware + drone convergence, Taiwan Strait,
      supply-chain weaponization
- [ ] Metadata form fields in the browser scenario editor

**Status:** deferred.

---

## Round two — engine depth, optimization, optical separation

The Round-One epics (A–F) treat Faultline as a single-shot Monte Carlo
engine over hand-authored scenarios. Round Two pushes three directions:

1. **Optical separation from external research.** Faultline is a
   generic statistical-modeling tool. Any in-repo branding, citation,
   or data field that visibly pairs Faultline with a specific external
   threat-assessment series creates "looks operational when paired"
   exposure that the LEGAL.md posture is built to avoid. Round Two
   opens with the cleanup pass and bakes the rule into all subsequent
   work.
2. **Engine depth, not surface polish.** The current engine is a
   single-pass discrete-tick Lanchester-plus-kill-chains simulator.
   Round Two adds adaptive actors, capacity/queue dynamics, network
   primitives, belief-state tracking, and a strategy-search layer on
   top — moving the engine from "evaluate this fixed scenario" to
   "explore the strategy space around this scenario."
3. **Calibration, reproducibility, and authoring discipline.** The
   tool will only be cited from external research if its outputs can
   be re-derived bit-for-bit from a published manifest, and only
   authored well if the editor catches mistakes the analyst would
   otherwise discover by reading the report.

### Epic G — Reference sanitization

Faultline currently contains ~184 in-repo references to a specific
external threat-assessment series ("ETRA"), including ~150 of them in
`site/js/app/tech-library.js` as `etra_ref` fields citing specific
section numbers, plus prose in `report.rs`, scenario TOML headers,
docs, and CSS comments. The sanitization pass replaces all of them
with generic vocabulary that is field-standard in published threat-
assessment writing (RAND, CSIS, IISS, CRS) and removes any structural
field name (`etra_ref`) that would re-introduce coupling.

- [x] Rename `etra_ref` → `source_ref` (or just `ref`) in the tech-card
      schema; update `tech-library.js`, `tech-cards.js`, and any
      consumer
- [x] Replace per-card `etra_ref` *values* (e.g. "Section 5.1 (Smurfing
      Swarm)") with generic descriptors ("structured threat assessment
      literature", "published OSINT analysis"); remove the unique
      section-number fingerprints
- [x] Replace "Locust ETRA" / "ETRA-2026-WMD-001" / etc. domain
      descriptions with topical labels ("Drone swarms — covert
      sensors, C-UAS", "WMD proliferation")
- [x] Replace "ETRA-style" / "ETRA-grade" / "ETRA-candidate" with
      generic equivalents ("structured threat assessment",
      "publication-grade", "high-confidence") in `report.rs`,
      `scenario.rs`, `campaign.rs`, `counterfactual.rs`,
      `improvement-plan.md`, `ROADMAP.md`, `scenario_schema.md`
- [x] Drop "ETRA Scenario N" labels from scenario TOML headers and
      tutorial strings; reframe as "open-source threat-assessment
      archetype" where the framing is needed at all
- [x] Dedupe `site/scenarios/` against `scenarios/` (currently
      hand-copied byte-identical) — symlink at build time or copy via
      CI step so future drift is impossible
- [x] Add a CI grep guard that fails the build if `\bETRA\b`,
      `etra_ref`, or any `ETRA-YYYY-` document ID re-enters the tree

**Status:** Epic G **closed**. Single PR (branch
`epic-g-reference-sanitization`) renamed the `etra_ref` field to
`source_ref` (132 occurrences across `tech-library.js` and
`tech-cards.js`), replaced 129 per-card section citations with
domain-generic descriptors ("Open-source UAS / counter-UAS literature"
etc.), rewrote the 6 `DOMAINS` descriptions to drop document
identifiers, swept ETRA branding from 4 Rust source files and 3 docs
files, rewrote the headers of 3 scenario TOMLs to remove direct
publication citations, replaced `site/scenarios/` (a byte-identical
hand-copy) with a symlink to `../scenarios`, and added
`tools/ci/grep-guard.sh` wired into both CI workflows after
`cargo-deny` to block re-entry of `\bETRA\b`, `etra_ref`, or any
`ETRA-YYYY-` document ID. The guard whitelists this file (Epic G's
section legitimately describes the patterns it bans) and itself.
Prerequisite for the rest of Round Two; H–Q can now proceed.

### Epic H — Strategy search & adversarial co-evolution

Today every Monte Carlo run uses the scenario's hand-authored faction
parameters as static inputs. Faultline can already *evaluate* a
strategy; it can't *find* one. This epic adds a search layer that
treats faction parameters (force allocations, tech-card selections,
event ROE) as decision variables and searches the joint space.

- [x] `StrategySpace` schema — declare which scenario parameters are
      "decision variables" for which faction, with allowed ranges /
      discrete choices
- [x] Single-side optimization: given a fixed opponent, search over
      one faction's strategy space to maximize a user-specified
      objective (win rate, cost-asymmetry, attribution-difficulty)
      under constraints
- [ ] Adversarial co-evolution: alternating best-response loop until
      both sides' strategies converge (or report cycle / no-equilibrium
      diagnostics)
- [x] Pareto frontier across multi-objective searches (win rate vs.
      detection vs. attacker cost)
- [x] Report section: "best-response strategies under search" with
      stability diagnostics
- [x] Determinism contract: search uses its own seeded sampler
      independent of MC seed so search-then-evaluate is reproducible

**Status:** Epic H **closed** (round one). Single PR (branch
`epic-h-strategy-search`) shipped five of the six items — the
"single-side optimization with Pareto frontier" arc the epic was
scoped against; adversarial co-evolution (the alternating best-
response loop) is the deliberately-deferred sixth item and slots
naturally into a round-two PR alongside Epic I (defender-posture
specialization). What landed:

- A new `StrategySpace` type on `Scenario` (optional, `#[serde(default,
  skip_serializing_if = "StrategySpace::is_empty")]` so legacy
  scenarios stay byte-identical). Each `DecisionVariable` names a
  parameter via the same dotted path layer Epics B/C use for
  `--counterfactual` / `--sensitivity`, plus a `Domain::Continuous {
  low, high, steps }` or `Domain::Discrete { values }` sampling
  declaration. An optional `owner: FactionId` lets reports group
  decisions by side without reading them out of the path string.
- A new `faultline_stats::search` module with `run_search(scenario,
  config)` that samples assignments via `Random` or `Grid` methods,
  evaluates each via `MonteCarloRunner::run`, and returns a
  `SearchResult` with per-trial `objective_values`, the non-dominated
  `pareto_indices` across all declared objectives (direction-aware:
  `MaximizeWinRate` is `>=` while `MinimizeDetection` is `<=`), and
  the `best_by_objective` map (ties resolve by lowest trial index for
  reproducibility).
- A two-seed determinism contract enforced by `SearchConfig`:
  `search_seed` drives the `ChaCha8Rng` that samples assignments and
  is independent of `mc_config.seed`, which drives the inner Monte
  Carlo evaluation. Same `(search_seed, mc_seed)` always reproduces
  the same `output_hash`; changing the inner MC seed changes trial
  *outcomes* but never the trial *assignments* (pinned by the
  `search_seed_independent_of_mc_seed` test in
  `crates/faultline-stats/src/search.rs`).
- Round-one objectives (`MaximizeWinRate { faction }`,
  `MinimizeDetection`, `MinimizeAttackerCost`,
  `MaximizeCostAsymmetry`, `MinimizeDuration`) are pure functions of
  the existing `MonteCarloSummary` / `CampaignSummary` shape — no
  new analytics modules required. Adding a new objective is additive;
  the manifest stores objective *labels* (`label()` strings), not the
  structured enum, so future variants don't break existing manifests.
- `--search` CLI mode with `--search-method`, `--search-trials`,
  `--search-runs`, `--search-seed`, and repeatable
  `--search-objective` flags. CLI objectives override the scenario's
  embedded `[strategy_space].objectives` list when both are present,
  so a pre-canned space can be reused for one-off questions. A new
  `ManifestMode::Search` variant lets `--verify` replay a saved
  search-mode manifest bit-identically (proven on the bundled
  `strategy_search_demo.toml` scenario in the verify-bundled CI
  pipeline).
- Engine-side validation rejects empty paths, duplicate variable
  paths, inverted continuous ranges, zero `steps`, empty discrete
  `values`, unknown `owner` factions, and unknown
  `MaximizeWinRate.faction` references at scenario load time so
  authoring mistakes surface up front instead of mid-search. Path-
  resolution validation (does the dotted path resolve via
  `set_param`?) lives in the search runner itself, since `set_param`
  is in `faultline-stats` and the engine cannot depend on stats
  without creating a crate cycle.
- Bundled `scenarios/strategy_search_demo.toml` exercises the full
  pipeline end-to-end: two continuous decision variables, two
  objectives, both grid and random methods round-trip through
  `--verify` cleanly, and the scenario passes all CI guards
  (fmt, clippy, verify-bundled, verify-migration, grep-guard).

All tests pass (~444 across the workspace including 8 unit + 4
integration + 8 engine-validation tests covering the new surface);
fmt / clippy / cargo-deny / WASM build / JS tests / verify-bundled /
verify-migration / grep-guard all clean.

### Epic I — Defender-posture optimization

Sub-class of Epic H specialized for the most common analyst workflow:
"given this offensive scenario, what's the cost-optimal defender
configuration?" Distinct enough to ship independently.

- [x] Defender decision space: force-placement / posture parameters
      reachable via the `set_param` path layer (now extended to
      `faction.<id>.force.<force_id>.{strength,mobility,upkeep}`).
      Tech-card budgeted-menu selection deferred — the existing
      `tech_access` shape is a flat vec, and a budgeted on/off switch
      is a separate schema design (slot for round two).
- [x] Cost / effectiveness Pareto frontier specifically for defender
      configurations — four new `SearchObjective` variants
      (`MaximizeAttackerCost`, `MaximizeDetection`, `MinimizeDefenderCost`,
      `MinimizeMaxChainSuccess`) compose with the existing
      `MaximizeWinRate` to express defender-aligned multi-objective
      Pareto searches.
- [x] "Counter-recommendation" report section: ranked list of
      Pareto-frontier postures with `(objective_value − baseline)`
      deltas, direction-aware "improvement?" tags, and Wilson 95%
      CIs on rate-valued win-rate objectives. Anchored on a "do
      nothing" baseline trial that the search runner now computes
      alongside the sampled trials.
- [ ] Sensitivity of the optimal posture to assumed attacker strategy
      (robustness analysis — "this defender wins if the attacker is
      Profile A or C, but not Profile B")

**Status:** Epic I **closed** (round one). Single PR (branch
`epic-i-defender-posture`) shipped three of the four items — the
"single-side defender posture optimization with Pareto frontier and
counter-recommendation" arc the epic was scoped against. Robustness
analysis (the attacker-strategy sensitivity sweep) is the deferred
fourth item and slots naturally into a round-two PR alongside the
Epic H adversarial-co-evolution loop, since both involve nested
search across attacker and defender spaces.

What landed:

- Four new defender-aligned `SearchObjective` variants on top of the
  Epic H attacker-aligned set: `MaximizeAttackerCost`,
  `MaximizeDetection`, `MinimizeDefenderCost`, and
  `MinimizeMaxChainSuccess`. All four are pure functions of the
  existing `CampaignSummary` shape — no new analytics modules, no
  new RNG draws. CLI parser, label round-trip, and direction (the
  `maximize()` boolean) all extended; back-compat is total because
  the enum remains additive (older manifests' objective labels
  reparse cleanly).
- Extended the dotted-path layer in `faultline_stats::sensitivity` to
  reach `faction.<id>.force.<force_id>.{strength,mobility,upkeep}`,
  unblocking force-placement and force-readiness as decision
  variables. Error messages name the chain of (faction, force,
  field) so authoring typos surface in the right place.
- Added `compute_baseline: bool` to `SearchConfig` and an optional
  `baseline: Option<SearchTrial>` to `SearchResult`. When enabled
  (default in the CLI), the search runner emits a "do-nothing" trial
  — the scenario evaluated with no decision-variable assignment
  applied — using the sentinel `trial_index = u32::MAX` so renderers
  detect it without a separate type. The `baseline_objective_matches
  _a_zero_assignment_run` test pins that the baseline reuses the
  inner MC seed so it's bit-identical to a standalone MC of the same
  scenario.
- New `render_counter_recommendation` section on the search report.
  Gated on (a) baseline present, (b) at least one decision variable
  carrying an `owner` (so legacy attacker-only spaces stay
  unchanged), (c) non-empty Pareto frontier. Each frontier trial gets
  a posture block, a delta table with direction-aware "improvement?"
  flags, and a Wilson 95% CI panel on rate-valued win-rate objectives
  (other-shape objectives — sums, maxes, durations — would need
  bootstrap CIs and are deferred). The deltas are anchored on the
  baseline so an analyst reads "this posture buys you X over the
  do-nothing case" rather than guessing from absolute values.
- `ManifestMode::Search` gained a `compute_baseline` field
  (`#[serde(default)]` so older manifests at the Epic H shape replay
  cleanly with `false`); the CLI emits the actual setting and the
  `--verify` replay path threads it through into `SearchConfig` so
  hashes match.
- Bundled `scenarios/defender_posture_optimization.toml` exercises
  the full pipeline: three blue-side decision variables (per-phase
  detection probabilities and force readiness), three defender-
  aligned objectives, an 8-cell grid that produces a 2-trial Pareto
  frontier with measurable improvements (e.g. trial #6 reduces max
  chain success from 0.70 to 0.33 and lifts max detection from 0.47
  to 0.87 against the do-nothing baseline). Five integration tests in
  `crates/faultline-stats/tests/epic_i_defender_posture.rs` lock
  search-mode determinism, the baseline-changes-hash invariant,
  Counter-Recommendation rendering gating, and the
  decision-variables-actually-move-objectives sanity check (the trap
  that caught the first draft of this scenario).

All schema additions are `#[serde(default)]` so legacy scenarios
load unchanged; all 12 bundled scenarios still verify bit-identical
via the manifest determinism contract; cargo deny / clippy / fmt /
verify-bundled / verify-migration / grep-guard / JS tests / WASM
build all clean.

### Epic J — Adaptive faction AI

Current `AiProfile` is shallow — a few aggression / risk-tolerance
floats. Real factions adapt to observed opponent behavior. This epic
adds explicit utility functions and Bayesian belief updating so a
faction can *change strategy mid-run* in response to what it has
observed.

- [ ] `Faction.utility` schema — multi-term utility (control,
      casualties, attribution, time-to-objective) with weights
- [ ] Per-tick decision step: faction selects from its action menu
      based on argmax-utility under current belief state
- [ ] Bayesian belief-state over opponent's hidden variables
      (capability cards not yet observed, intent, force disposition)
- [ ] Information events update belief states asymmetrically — a
      detection event raises the defender's confidence about the
      attacker's location; a successful OPSEC raises the attacker's
      confidence that the defender is unaware
- [ ] Determinism: belief updates use scenario seed; same seed and
      same observations always produce the same belief trajectory

**Status:** deferred. Largest engine change in Round Two; partition
into 3+ PRs.

### Epic K — Capacity & queue dynamics

Current engine treats defenders as either-detecting-or-not. Real
defenders have *finite investigative throughput*: alerts queue, leads
go uninvestigated, FOIA requests pile up. This epic adds capacity as
a first-class engine primitive and unlocks scenario classes the kill-
chain primitive can't naturally express ("Process DoS", verification
overload, alert-fatigue suppression of true positives).

- [x] `DefenderCapacity` schema — per-defender-role queue depth, mean
      service rate, and overflow behavior (`DropNew` / `DropOldest` /
      `Backlog`)
- [x] Phase outputs can enqueue defender work (e.g. "this phase
      generates N synthetic tips per tick") — modeled as
      `CampaignPhase.defender_noise` with per-active-tick Poisson
      sampling against the engine RNG
- [x] Queue-depth-dependent detection probability: a saturated queue
      multiplies the per-tick detection roll by the role's
      `saturated_detection_factor`, surfaced via the new
      `gated_by_defender` field on `CampaignPhase`
- [x] Report: per-defender utilization, time-to-saturation
      distribution, "shadow detections" (true positives suppressed by
      saturation — caught at idle, missed under load); per-run shape
      on `RunResult.defender_queue_reports`, cross-run rollup on
      `MonteCarloSummary.defender_capacity`
- [x] Scenario archetypes: alert-fatigue (shipped:
      `scenarios/alert_fatigue_soc.toml`); FOIA volume attack and
      forensic inspection backlog left for follow-up scenarios using
      the same primitives

**Status:** Epic K **closed**. Single PR (branch
`epic-k-capacity-queues`) added the `DefenderCapacity` /
`OverflowPolicy` schema on `Faction`, the `defender_noise` and
`gated_by_defender` fields on `CampaignPhase`, runtime queue state
on `SimulationState.defender_queues`, the per-tick **arrive →
assess → service** ordering in
`crates/faultline-engine/src/campaign.rs::campaign_phase` (enqueue
noise, then roll detection against post-arrival depth, then drain
the queue at end-of-tick), and a deterministic Knuth Poisson
sampler so the noise volume is RNG-deterministic under the same
seed. Saturation gating uses a single uniform draw covering both
the actual detection roll and the shadow-detection bookkeeping —
draws below the unattenuated `dp` but above the saturated `dp`
count as shadow detections, captured per-queue in `shadow_detections`
and aggregated into `mean_shadow_detections` on the cross-run
summary. Validation rejects unknown `(faction, role)` references at
load time so author typos don't silently no-op. The `alert_fatigue_soc`
bundled scenario produces ~85% tier-1 saturation, ~0.4 mean shadow
detections per run, and a measurable red-win-rate uplift over the
no-fatigue baseline — proving the mechanism is observable in the
report. All 10 bundled scenarios still verify bit-identical via the
manifest determinism contract; cargo deny / clippy / fmt /
verify-bundled / verify-migration / grep-guard all clean.

### Epic L — Network & graph primitives

Faultline's only graph is the regional adjacency map. Adding general
network primitives (supply, communications, social, financial) opens
a large class of scenarios that currently can't be expressed without
abusing the regional model.

- [ ] `Network` schema — typed graph with nodes, edges, capacities,
      and per-edge metadata (latency, bandwidth, trust)
- [ ] Network-aware events: interdiction reduces edge capacity,
      disruption removes nodes, infiltration adds attacker visibility
- [ ] Network-aware metrics: connectivity, max-flow / min-cut,
      betweenness centrality (deterministic implementations)
- [ ] Multi-network scenarios: a single faction's supply, comms, and
      social networks tracked simultaneously, with cross-network
      events (a comms outage degrades supply coordination)
- [ ] Report: per-network resilience curves and critical-node ranking

**Status:** deferred.

### Epic M — Information warfare & belief asymmetry

A first-class model of *what each faction knows*, distinct from what
*is true*. Enables modeling deception, false flags, intentional
misperception, and OPSEC as decision-affecting rather than purely
narrative.

- [ ] `BeliefState` per faction — distribution over opponent's hidden
      variables (force disposition, intent, attributed identity)
- [ ] Deception events update opponent belief without changing world
      state ("decoy convoy" raises opponent's confidence about a
      false location)
- [ ] Attribution rolls use the *believed* attribution distribution,
      not the true one — so a successful false-flag operation
      mis-attributes confidently
- [ ] Report: per-faction "what they thought was happening" trace
      alongside the actual world trace; flag divergences
- [ ] Cross-references with Epic J — adaptive AI must act on belief,
      not truth

**Status:** deferred. Pairs naturally with J.

### Epic N — Validation harness & calibration discipline

Faultline currently has no way to disconnect "the math is internally
consistent" from "the parameter ranges are defensible." This epic
adds a back-testing harness that runs scenarios against historical
analogues with known outcomes and reports calibration metrics. Does
not claim prediction; disciplines the parameter library.

- [ ] `historical_analogue` field on scenarios — references a public
      event with documented outcome
- [ ] Calibration metric: how well did MC outcome distribution shape
      the historical observation? (KS distance, log-likelihood)
- [ ] Reference scenario set: 5–10 well-documented historical
      analogues (e.g. Turkey 2016 coup attempt outcome bands,
      published Russo-Ukraine drone-engagement statistics) where
      parameters are constrained by published estimates
- [ ] Report: per-scenario "calibration confidence" surfaced
      alongside the methodology appendix
- [ ] Author guidance: scenarios with no historical analogue tagged
      as "purely synthetic"; analyst is told what that means for
      result interpretation

**Status:** deferred. The hardest epic in Round Two — the
data-availability bottleneck is real and the work is cross-cutting.

### Epic O — Schema versioning & migration

Round Two adds half a dozen new schema sections (StrategySpace,
DefenderCapacity, Network, BeliefState, historical_analogue). Without
a versioning story, the existing scenario library will rot as fields
move. Add a `[meta].schema_version` field and a migrator framework
*before* we start shipping schema changes that need migrations.

- [x] `[meta].schema_version` field with current version constant
- [x] Migrator framework: `fn migrate(scenario: TomlValue, from: u32,
      to: u32) -> TomlValue` chain
- [x] CLI: `faultline-cli migrate <path> [--in-place]`
- [x] Validator: warns when loading a scenario authored against an
      older schema; offers to migrate
- [x] CI gate: every existing bundled scenario loads cleanly under
      the migrator at every shipped version
- [x] Documentation: schema evolution policy ("additive fields land
      with serde-default; breaking changes bump version and ship a
      migrator in the same PR")

**Status:** Epic O **closed**. Single PR (branch
`epic-o-schema-versioning`) added the `schema_version: u32` field to
`ScenarioMeta` (defaulting to 1 via `#[serde(default = ...)]` so all
pre-existing scenarios load unchanged), introduced the
`faultline_types::migration` module with `CURRENT_SCHEMA_VERSION = 1`
and a registry-based chain driver (`apply_chain` lifted out of
`migrate` so synthetic v0→v1→v2 chains can exercise the loop logic
even though the production registry is currently empty), and routed
both CLI and WASM scenario loading through a single `load_scenario_str`
helper that surfaces a stale-fixture warning when source and current
versions disagree. The `--migrate` CLI flag (with `--in-place`) emits
the upgraded TOML via `migrate_scenario_str`; it short-circuits before
validation because a migration's whole job is to make a stale
scenario valid. The new `tools/ci/verify-migration.sh` script is
wired into both CI workflows after `verify-bundled-scenarios.sh`,
running `--migrate` on every bundled scenario and re-validating the
emitted form so future migrations can't silently leave bundled
fixtures behind. All 9 bundled scenarios were backfilled with
`schema_version = 1`; library-level tests cover the chain driver
(synthetic multi-step chain, missing-step error, step-failure
propagation, legacy-fixture without the field, explicit-current
fixture). Schema-evolution policy documented in
`docs/scenario_schema.md`. Prerequisite for J/K/L/M now in place.

### Epic P — Authoring depth: editor, linter, explain

The current TOML editor is a textarea with WASM-side validation only.
For scenarios to be authored reliably as the schema grows, the editor
needs schema-aware autocomplete, inline validation against the engine
type system, and a structured "what does this scenario actually model?"
explainer.

- [ ] Monaco / CodeMirror editor with TOML grammar + JSON-schema-driven
      autocomplete (schema generated from the Rust types)
- [ ] Inline validation panel: surfaces engine-side warnings (unreached
      regions, factions with no objectives, kill chains with
      unreachable phases) without running a sim
- [ ] Hover documentation: field docstrings from the Rust types
      surface as hover tooltips
- [ ] `faultline-cli explain <scenario>` — produces a structured prose
      summary: what factions exist, what their objectives are, what
      kill chains they execute, what the victory conditions are, what
      parameters are tagged low-confidence
- [ ] Editor: "Explain" button that renders the same summary in-app

**Status:** deferred. Enables Epic F (scenario library expansion) to
move faster.

### Epic Q — Reproducibility & artifact provenance

Lets external citers reference exact Faultline runs by manifest. Every
result emits a manifest containing the inputs needed to re-derive it
bit-for-bit, and the CLI can verify a published manifest by re-running
and comparing.

- [x] `RunManifest` struct: scenario hash, engine version, MC config,
      RNG seed, output hash, host platform
- [x] CLI: every `run` emits `manifest.json` alongside `report.md`
- [x] CLI: `faultline-cli verify <manifest>` re-runs and asserts
      bit-identical output, exits non-zero on mismatch
- [x] Engine version pinned in `Cargo.toml` and surfaced via a
      build-script-generated constant
- [x] CI: `verify` runs on every bundled scenario at every release tag
- [x] Report front-matter includes the manifest's content hash so
      analysts can cite "Faultline run 0xabcd…" stably

**Status:** Epic Q **closed**. Single PR (branch
`epic-q-reproducibility`) added the `RunManifest` schema in a new
`faultline_stats::manifest` module — SHA-256 hashes computed over the
canonical JSON form of the parsed `Scenario` (so the input identity is
robust to TOML formatting churn) and the `MonteCarloSummary` /
`ComparisonReport` / `RunResult` / `SensitivityResult` (so the output
identity exactly matches what was emitted). Every CLI run mode
(single-run, monte-carlo, counterfactual, compare, sensitivity)
emits `manifest.json` alongside its existing artifacts and prepends
the manifest hash to the rendered report's front-matter both as a
parseable HTML comment (`<!-- faultline-manifest manifest_hash="…" -->`)
and as a one-line analyst-facing citation. The `--verify <MANIFEST>`
flag loads the saved manifest, hashes the live scenario, refuses
mismatched scenarios early, replays the recorded mode + Monte Carlo
config, and exits non-zero with a structured field-level diff if any
replay-bound field drifts. `host_platform` is recorded for diagnostics
but excluded from the manifest hash so a manifest produced on Linux
verifies cleanly on macOS — the determinism contract requires
identical output across platforms for the same seed. Engine version
is surfaced via `env!("CARGO_PKG_VERSION")` in
`faultline_stats::manifest::FAULTLINE_ENGINE_VERSION` (no separate
build script needed because Cargo populates the env var
automatically). The new `tools/ci/verify-bundled-scenarios.sh` script
is wired into both CI workflows after `cargo-deny` and the
reference-sanitization guard, and runs an emit/verify round-trip on
every TOML in `scenarios/` (currently 9). Library-level tests in
`crates/faultline-stats/src/manifest.rs` and
`crates/faultline-stats/tests/report_integration.rs` lock the
determinism contract — same scenario + seed → same hashes; mutating
a scenario field flips both `scenario_hash` and `output_hash`.

---

## Working notes

- **Scope discipline.** At ~190 findings this branch can sprawl.
  Treat it as a long-lived integration branch and merge completed
  epics back to `main` as they finish.
- **PR granularity.** Each epic is multiple PRs. Epic A alone is
  probably 2–3. Prefer small, focused PRs; don't let an epic become
  a monolith.
- **Determinism.** Anything that touches the engine or stats must
  preserve bit-identical output across native and WASM for the same
  seed. Add a regression test whenever a new RNG consumer appears.
- **Backwards compatibility.** New schema fields must be
  `#[serde(default)]` so existing TOML scenarios load unchanged.
- **This doc is living.** Check a box when a PR lands. When an epic
  closes, leave it in the doc as a record rather than deleting.
