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

- [ ] `time_to_first_detection` histogram per chain
- [ ] Per-phase Kaplan-Meier survival / cumulative hazard curves
- [ ] Sobol / Morris variance decomposition (replacing pure OAT)
- [ ] Correlation matrix (inputs ↔ outputs)
- [ ] Escalation-ladder branch condition with hysteresis:
      `EscalationThreshold { from, to, duration_ticks }`
- [ ] Pareto frontier output (cost vs. success vs. detection)
- [ ] Defender-reaction-time distribution

**Status:** deferred.

### Epic D — Engine model depth

Things scenario authors want to express and can't. Pick 2–3, not
all at once — each is substantial.

- [ ] Supply-network graph + interdiction (new `supply_phase`)
- [ ] Multi-front resource contention (campaigns compete for
      defender attention)
- [ ] Leadership decapitation + succession penalties
- [ ] Info-op narrative competition (so `MediaEvent` isn't
      fire-and-forget)
- [ ] Weather / time-of-day modifiers on terrain
- [ ] Coalition / alliance fracture mechanic (beyond
      `Foreign.is_proxy` flag)
- [ ] Refugee / displacement flows with cross-regional propagation
- [ ] `BranchCondition::OrAny` for prerequisite OR logic

**Status:** deferred — select on entry.

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

- [ ] `StrategySpace` schema — declare which scenario parameters are
      "decision variables" for which faction, with allowed ranges /
      discrete choices
- [ ] Single-side optimization: given a fixed opponent, search over
      one faction's strategy space to maximize a user-specified
      objective (win rate, cost-asymmetry, attribution-difficulty)
      under constraints
- [ ] Adversarial co-evolution: alternating best-response loop until
      both sides' strategies converge (or report cycle / no-equilibrium
      diagnostics)
- [ ] Pareto frontier across multi-objective searches (win rate vs.
      detection vs. attacker cost)
- [ ] Report section: "best-response strategies under search" with
      stability diagnostics
- [ ] Determinism contract: search uses its own seeded sampler
      independent of MC seed so search-then-evaluate is reproducible

**Status:** deferred. Depends on Epic C (escalation thresholds) for
multi-objective stability and Epic G (no co-branding leakage into
search outputs).

### Epic I — Defender-posture optimization

Sub-class of Epic H specialized for the most common analyst workflow:
"given this offensive scenario, what's the cost-optimal defender
configuration?" Distinct enough to ship independently.

- [ ] Defender decision space: tech-card selection from a budgeted
      menu, force-placement across regions, event-ROE choices
- [ ] Cost / effectiveness Pareto frontier specifically for defender
      configurations
- [ ] "Counter-recommendation" report section: ranked list of defender
      changes, each with `(cost_delta, success_delta, detection_delta,
      attribution_delta)` and Wilson CIs
- [ ] Sensitivity of the optimal posture to assumed attacker strategy
      (robustness analysis — "this defender wins if the attacker is
      Profile A or C, but not Profile B")

**Status:** deferred — slot after H.

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

- [ ] `DefenderCapacity` schema — per-defender-role queue depth, mean
      service rate, and overflow behavior (drop-oldest / drop-random /
      backlog)
- [ ] Phase outputs can enqueue defender work (e.g. "this phase
      generates N synthetic tips per tick")
- [ ] Queue-depth-dependent detection probability: a saturated queue
      drops detection rate
- [ ] Report: per-defender utilization curves, time-to-saturation
      histograms, "shadow detections" (true positives that arrived
      while the queue was full and got dropped)
- [ ] Scenario archetypes: alert-fatigue, FOIA volume attack, forensic
      inspection backlog under mass-incident response

**Status:** deferred. Bridges the engine into non-kinetic threat
classes that the projection-style assessments analyze.

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

- [ ] `[meta].schema_version` field with current version constant
- [ ] Migrator framework: `fn migrate(scenario: TomlValue, from: u32,
      to: u32) -> TomlValue` chain
- [ ] CLI: `faultline-cli migrate <path> [--in-place]`
- [ ] Validator: warns when loading a scenario authored against an
      older schema; offers to migrate
- [ ] CI gate: every existing bundled scenario loads cleanly under
      the migrator at every shipped version
- [ ] Documentation: schema evolution policy ("additive fields land
      with serde-default; breaking changes bump version and ship a
      migrator in the same PR")

**Status:** deferred. Should land before J/K/L/M to keep the existing
scenarios usable.

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

- [ ] `RunManifest` struct: scenario hash, engine version, MC config,
      RNG seed, output hash, host platform
- [ ] CLI: every `run` emits `manifest.json` alongside `report.md`
- [ ] CLI: `faultline-cli verify <manifest>` re-runs and asserts
      bit-identical output, exits non-zero on mismatch
- [ ] Engine version pinned in `Cargo.toml` and surfaced via a
      build-script-generated constant
- [ ] CI: `verify` runs on every bundled scenario at every release tag
- [ ] Report front-matter includes the manifest's content hash so
      analysts can cite "Faultline run 0xabcd…" stably

**Status:** deferred. Enables external citation of Faultline outputs
without coupling Faultline to any specific external publication.

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
