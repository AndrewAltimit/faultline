# Faultline Improvement Plan

Living tracker for cross-cutting improvement work. Individual PR/epic
writeups live in `CLAUDE.md` and the git history; this doc is the
*ordering* of what's left and *why* — the running narrative, not the
archive.

The plan was originally derived from a three-angle audit (engine
analytics, frontend/UX, scenario content — ~190 findings). It has
since been refreshed twice as epics closed and external reviews
landed. **Last refresh: 2026-05-02** — incorporating the May 2026
priority review, the game-middleware reframing, and the Epic N
calibration scaffold landing.

---

## Priorities (May 2026 review)

The five highest-leverage open items, in order:

1. ~~**Epic N — calibration discipline.** The hardest and most
   important. Even one well-documented historical analogue with
   calibration metrics in the report would change what the tool
   *means*. Until calibration exists, every analytical output is
   internally consistent but externally unjustified, and every new
   epic that produces more outputs (J, M, D-round-three) compounds
   the trust gap.~~ **Scaffold shipped May 2026** — see closed-epics
   list. Schema (`[meta.historical_analogue]`), calibration computation
   (`faultline_stats::calibration` with Pass / Marginal / Fail verdict
   ladder per `Winner` / `WinRate` / `DurationTicks` observation), and
   the always-emit `## Calibration` report section all landed. One
   bundled archetype (`scenarios/calibration_demo.toml`). Remaining N
   work: 5–10 cleanly-sourced single-event analogues for the bundled
   scenario set; per-scenario "calibration confidence" surfaced in the
   methodology appendix; deciding whether calibration verdicts should
   gate the `verify-bundled` CI step (currently they don't — output is
   bit-stable but verdicts can be `Fail` without breaking CI).
2. ~~**R3-5 property tests.** Determinism + seeded RNG = ideal
   substrate. "For any seed, no faction strength goes negative" /
   "Wilson CI bounds always contain the point estimate" /
   "post-disruption residual capacity ≤ pre-disruption" are
   high-value invariants the seeded fixture tests miss. Cheap to
   start; compounds with every later epic.~~ **Shipped May 2026** —
   see closed-epics list. All three pinned invariants (engine
   strength non-negative, Wilson bounds contain point estimate,
   post-disruption max-flow ≤ pre-disruption) plus determinism
   properties on engine / search now have `proptest` coverage.
3. ~~**Epic P — `faultline-cli explain` subset.** Cheap, decouples
   from the larger Monaco editor work, and forces every scenario
   to answer "which parameters does this scenario actually move?"
   — the same question R3-2 asks of the engine.~~ **Shipped May
   2026** — see closed-epics list. Remaining Epic P items (Monaco
   editor, hover docs, inline validation panel) stay deferred.
4. **R3-2 round two — finish the unread-parameter audit.** Round
   one shipped the three highest-leverage silent-no-ops; round
   two has been closing the rest opportunistically. As of May 2026:
   `diplomacy` shipped with Epic D round-three item 1; the
   `mobility` + `terrain.movement_modifier` +
   `EnvironmentWindow.movement_factor` triple shipped as a coupled
   "movement rate" wiring (R3-2 round-two item 1; `upkeep` was
   already wired); population-segment activation shipped as a
   coupled "media landscape + activation tracking" wiring
   (R3-2 round-two item 3 — wires `MediaLandscape.fragmentation`,
   `social_media_penetration`, `internet_availability` and adds
   per-segment activation tracking + report). Items still deferred:
   tech-card costs, visualization metadata (`Region.centroid`,
   `Faction.color`), `force_projection`. Closing the gap maintains
   the trust the round-one audit bought.
5. **Defer Epic J (adaptive AI) and Epic M (belief states) until
   N is at least scaffolded.** Both are interesting but produce
   more outputs whose calibration is unknown. They compound the
   trust gap rather than closing it. Moving J/M before N is
   shipping interesting machinery on top of a foundation we
   haven't justified.

R3-3 (decompose `report.rs`) was on the original priority list and
shipped before this refresh — see the closed-epics list below. The
Epic P explain subset shipped after the May 2026 refresh; its slot
in the list above is struck through rather than re-numbered so the
priority context (why this item, in this order, ahead of what)
remains visible to a future reader who wants to see how the list
was reasoned about. R3-5 (property tests) shipped after Epic P
explain — same reasoning for striking through rather than
re-numbering. Epic N scaffold shipped after R3-5; same reasoning
again for striking-through rather than re-numbering. Note that the
N entry remains the *highest* priority despite the strike-through:
the framework is in place, but the value compounds with each
single-event analogue added.

---

## Strategic option — game-middleware pivot

A May 2026 reframing observed that Faultline's engineering
discipline (determinism, replay manifests, schema versioning,
seeded RNG, kill-chain + capacity primitives, network primitives,
strategy search, counterfactual replay) maps almost exactly onto
what good game middleware needs — and that games don't have the
calibration problem that breaks the analyst use case. The same
properties that make Faultline "moderately interesting as a
research tool" make it "actually quite good as game-AI substrate."

**This is a strategic option, not a committed direction.** Recording
it here so the trade-off is explicit when the next epic is chosen,
not buried in a review thread.

**Genre fit, ranked.** Excellent: heist / stealth (kill chains +
Epic K alert fatigue), espionage / political sims, insurgency /
asymmetric warfare, grand strategy / 4X (faction AI + escalation +
leadership cadres), roguelikes with emergent factions. Possible:
tabletop GM tooling, browser strategy. Bad: twitch action,
narrative-first, puzzle, sports.

**How priorities re-rank under the pivot:**

| Epic | Analyst priority | Game-middleware priority |
| --- | --- | --- |
| J — adaptive AI | deferred-large | **critical** (NPCs) |
| M — belief asymmetry | deferred-pairs-with-J | **critical** (deception, fog of war) |
| D round-three (info-op, refugees, supply) | optional depth | **high** (player-visible mechanics) |
| F — scenario library + tech rebalance | optional content | **high** (content is the product) |
| N — calibration | hardest, blocking value | **skip entirely** |
| E — UI polish | required if user-facing | depends — irrelevant as middleware, important as designer tool |
| P — Monaco editor + explain | author tooling | **high** (game designers need this) |

**Engineering gaps for game use:**

1. No streaming API. Engine runs scenarios to completion. Games need
   `step()` / `apply_action()` / `query_state()`. The tick loop is a
   single-pass completion model with no step-isolation boundary;
   wrapping it requires designing a suspension/resume protocol over
   `SimulationState` so a host can interleave external input between
   ticks. Non-trivial — the time cost is "spike, then estimate", not
   a week.
2. No player-agency model. Scenarios assume both factions are
   AI-driven. Need a "player faction" abstraction that consumes
   inputs from a runtime instead of from the scripted strategy
   space.
3. No mid-run save/load. `SimulationState` derives `Serialize` /
   `Deserialize`, so a snapshot can be written today, but the format
   is unstable: no `schema_version` field on the struct (Epic O's
   versioning lives on `Scenario`, not runtime state), and several
   volatile runtime maps (`network_states`, `diplomacy_overrides`,
   `fired_fractures`, `defender_queues`, `metric_history`) were
   added incrementally for one-shot post-run analytics, not for
   round-tripping mid-run. A real save/load needs a stable on-disk
   format with explicit migration support — Epic O's groundwork on
   `Scenario` migration helps here, but the runtime-state schema is
   a separate piece of work.
4. Performance unknown for game budgets. Faultline runs scenarios
   in seconds; games need ms-per-frame. Need benchmarks. Monte
   Carlo is embarrassingly parallel; offline pre-computation
   probably does most of the heavy lifting.
5. Authoring UX is for analysts, not game designers. Visual editor
   need; Epic P helps but doesn't fully close it.

**Sequencing if the pivot is taken:** spike a streaming API as a
separate crate → build one demo game (cleanest fit is stealth, since
kill chains + Epic K's alert fatigue is plug-and-play) → revisit J /
M / D-round-three under the new framing → open-source as middleware.
Epic N is dropped under this branch; the analyst-vs-game decision
needs to be made before committing N's substantial cross-cutting
work.

---

## Status snapshot

**Closed (24):** A (uncertainty), B (counterfactual), C (time +
attribution dynamics), D round-one (engine depth: `OrAny`,
environment schedule, leadership decapitation), D round-two
(coalition fracture), D round-three item 1 (diplomacy behavioral
coupling for combat + AI), D round-three item 2 (supply-network
interdiction phase), G (reference sanitization), H round-one
(strategy search), H round-two (adversarial co-evolution), I
round-one (defender-posture optimization), I round-two (robustness
analysis), K (defender capacity / queue dynamics), L (network
primitives), N round-one (calibration scaffold —
`[meta.historical_analogue]` schema, per-observation Pass / Marginal
/ Fail verdict computation, `## Calibration` report section gating
on synthetic-vs-calibrated, one bundled archetype), O (schema
versioning), P sub-item (`faultline-cli explain` — pure-schema "what
does this scenario actually model?" view), Q (manifest replay), R3-2
round-one (unread-parameter audit, three highest-leverage
parameters), R3-2 round-two item 1 (`ForceUnit.mobility` +
`terrain.movement_modifier` + `EnvironmentWindow.movement_factor`
wired into a per-tick move-accumulator gate; `ForceUnit.upkeep`
turned out to be already wired), R3-2 round-two item 3
(`MediaLandscape.fragmentation` + `social_media_penetration` +
`internet_availability` wired into the political / information
phases as coupled noise / tension multipliers; per-segment
activation events tracked end-to-end and surfaced in a new
`## Civilian Activations` report section), R3-2 round-two item 4
(`TechCard.deployment_cost` / `cost_per_tick` / `coverage_limit`
wired into engine init, attrition, and combat phases respectively;
per-faction tech-cost report added), R3-3 (decompose `report.rs`),
R3-5 (property tests — `proptest` coverage of engine / search /
uncertainty / network_metrics invariants).

**Deferred / open epics:** D round-three (2 remaining items), E (UI
polish), F (scenario library + tech rebalance), J (adaptive AI), M
(belief asymmetry), N (reference scenario set + per-scenario
calibration confidence — round-two; framework round-one shipped),
P (authoring depth).

**Open R3 follow-ups:** R3-1 (test-boilerplate sweep — partial), R3-2
round-two (audit follow-up — items 1 + 2 + 3 + 4 closed; two items
still deferred: visualization metadata, `force_projection`), R3-4
(generalize leadership morale cap), R3-6 (decompose `Scenario`).

Detailed writeups for closed epics live in `CLAUDE.md` (which is the
authoritative description of what currently ships) and in the merged
PR descriptions on `main`. This doc no longer carries them.

---

## Open epics

### Epic D round three — engine model depth (remaining)

Round one shipped `OrAny`, the environment schedule, and leadership
decapitation. Round two added coalition fracture (analytical
accounting only). Round three opens with diplomacy behavioral
coupling for combat and AI; three items remain.

- [x] **Behavioral coupling for diplomacy (combat + AI).** Shipped
      May 2026. Mutually-Allied pairs skip combat; Cooperative
      neighbors are de-rated to 0.3× in AI threat presence and
      attack scoring. Reads `fracture::current_stance` so
      post-fracture and `EventEffect::DiplomacyChange` overrides are
      respected. Closes the round-two "analytical accounting only"
      caveat for combat and AI; victory-check and political phases
      still ignore diplomacy. See the "Diplomatic stance behavioral
      coupling" section in `CLAUDE.md`. Also closes R3-2 round-two
      item 2 (`Faction.diplomacy` unread).
- [x] **Supply-network interdiction phase.** Shipped May 2026.
      Builds on Epic L's network primitives — `kind = "supply"`
      networks with an `owner` now drive per-tick attenuation of
      the owner's `resource_rate` proportional to residual /
      baseline capacity. Validation rejects `kind = "supply"`
      without an `owner` (silent-no-op shape). New
      `## Supply Pressure` report section aggregates per-faction
      mean / min / pressured-tick stats across runs. Bundled
      archetype: `scenarios/supply_interdiction_demo.toml`. See the
      "Supply-network interdiction" section in `CLAUDE.md`.
- [ ] Multi-front resource contention: campaigns compete for defender
      attention beyond the single-queue Epic K already models.
- [ ] Info-op narrative competition so `MediaEvent` isn't
      fire-and-forget; refugee / displacement flows with cross-regional
      propagation. Lower priority — both lean game-design rather than
      analytical.

### Epic E — UI identity & analytical density

Move from "generic SaaS dark-mode" to "purpose-built defense-analysis
instrument." Items: gradient discipline, headline font + faultline
accent motif, map canvas treatment, chart polish (gridlines, KDE
overlays, confidence bands, colorblind-safe palette), radar /
parallel-coordinates replacement for the dense feasibility table,
map pan/zoom + label collision avoidance + kill-chain phase
overlays, dashboard progress + cancel for long MC runs, export
to PNG/CSV/JSON/PDF, addressable run URLs, light-mode toggle,
Monaco/CodeMirror editor (overlaps Epic P).

Some items depend on Epic A/B/C output (now landed); others depend
on Epic P (Monaco editor work). Status: deferred.

### Epic F — Scenario library & metadata

Make scenarios self-describing and rebalance the tech library.
Items: extend `[meta]` with `analytical_purpose`, `scenario_type`,
`osint_sources`, `red_team_profile`, `blue_team_posture`,
`sensitivity_parameters`, `historical_precedent`; backfill all
bundled scenarios; rebalance the tech library (current ratio is
heavily weighted toward institutional-erosion cards, with very few
SIGINT, supply-chain, SCADA/ICS, healthcare, GPS denial, deepfakes
cards); new scenarios (ransomware + drone convergence, Taiwan
Strait, supply-chain weaponization); metadata form fields in the
browser editor.

`historical_precedent` here overlaps `historical_analogue` in Epic
N — same field, different motivations. If N moves first, F
inherits the field for free. Status: deferred.

### Epic J — Adaptive faction AI

Current `AiProfile` is shallow. Real factions adapt to observed
opponent behavior. This epic adds explicit utility functions and
Bayesian belief updating so a faction can change strategy mid-run
in response to what it has observed.

Items: multi-term `Faction.utility` (control / casualties /
attribution / time-to-objective with weights); per-tick decision
step (faction selects from action menu via argmax-utility under
current belief state); Bayesian belief-state over opponent's
hidden variables; information events update belief states
asymmetrically; determinism preserved (belief updates use scenario
seed).

Status: **deferred until N at least scaffolded** (May 2026 review).
Largest engine change in the back half of the plan; partition into
3+ PRs when picked up. Critical for the game-middleware pivot.

### Epic M — Information warfare & belief asymmetry

A first-class model of *what each faction knows*, distinct from what
*is true*. Enables modeling deception, false flags, intentional
misperception, OPSEC as decision-affecting rather than narrative.

Items: `BeliefState` per faction; deception events that update
opponent belief without changing world state; attribution rolls
use the *believed* attribution distribution; per-faction "what
they thought was happening" trace alongside the actual world
trace; cross-references with Epic J.

Status: **deferred until N at least scaffolded** (May 2026 review).
Pairs naturally with J. Critical for the game-middleware pivot
(deception, fog of war = good gameplay).

### Epic N — Validation harness & calibration discipline

The hardest and most important open epic for the analyst use case.
Faultline currently has no way to disconnect "the math is internally
consistent" from "the parameter ranges are defensible." This epic
adds a back-testing harness that runs scenarios against historical
analogues with known outcomes and reports calibration metrics. Does
not claim prediction; disciplines the parameter library.

- [x] **`historical_analogue` field on scenarios.** Shipped May 2026
      as `[meta.historical_analogue]` with three observation variants
      (`Winner`, `WinRate`, `DurationTicks`). See the schema reference
      in `docs/scenario_schema.md`. Overlaps Epic F's
      `historical_precedent`; F inherits the field.
- [x] **Calibration verdict.** Shipped May 2026 as a coarse
      Pass / Marginal / Fail ladder per observation in
      `faultline_stats::calibration`, plus a worst-of roll-up. Not
      KS-distance / log-likelihood — the coarse ladder reflects how
      much confidence the framework can defensibly carry on its own
      thresholds. Tightening to a continuous metric is a follow-up
      once the framework has more bundled analogues to tune against.
- [x] **Synthetic-scenario disclaimer.** Shipped May 2026. Scenarios
      without an analogue render a "purely synthetic" notice in the
      `## Calibration` section explaining what the absence means for
      result interpretation.
- [ ] **Reference scenario set: 5–10 well-documented historical
      analogues where parameters are constrained by published
      estimates.** Round-two work. Round-one shipped one bundled
      archetype (`scenarios/calibration_demo.toml`) using a stylized
      aggregate analogue rather than a single named event. Each
      single-event addition is per-scenario research work, not a
      framework change.
- [ ] **Per-scenario "calibration confidence" surfaced alongside the
      methodology appendix.** Round-two work. The current `##
      Calibration` section surfaces per-observation source confidence
      and a per-scenario verdict; rolling the verdict into a
      methodology-section "calibration confidence" tag (alongside the
      existing scenario-level `meta.confidence` parameter-defensibility
      tag) is the next iteration.

**Why hardest, retrospectively.** Round one was tractable because the
framework only requires the schema + the verdict computation + the
report section — none of which depended on actually finding clean
single-event data. Round two is the data-availability work the
original "hardest" framing was about: finding even one analogue with
cleanly published outcome distributions plus parameter constraints is
real work.

**Why still most important.** Without filling in single-event
analogues, every output remains internally consistent but externally
unjustified for the bundled scenarios. Round one closed the framework
gap; round two closes the trust gap.

**Skip if game-middleware pivot is taken.** Calibration is the
analyst-use-case payoff; it's irrelevant for game middleware. The
framework that round one shipped is cheap to leave in place
regardless — `historical_analogue` is opt-in per scenario; game
scenarios just wouldn't declare one.

Status: round one shipped; round two deferred per priority list above
(framework now exists, data work proceeds opportunistically).

### Epic P — Authoring depth: editor, linter, explain

The current TOML editor is a textarea with WASM-side validation only.
For scenarios to be authored reliably as the schema grows, the editor
needs schema-aware autocomplete, inline validation against the engine
type system, and a structured "what does this scenario actually
model?" explainer.

- [x] **`faultline-cli explain <scenario>`** — produces a structured
      summary: factions, objectives, kill chains, victory conditions,
      decision-variable surface, low-confidence parameters. Shipped
      May 2026 as `faultline_stats::explain` + `--explain` /
      `--explain-format` CLI flags. Markdown to stdout by default;
      JSON for tooling.
- [ ] Monaco / CodeMirror editor with TOML grammar + JSON-schema-driven
      autocomplete (schema generated from the Rust types)
- [ ] Inline validation panel: surfaces engine-side warnings (unreached
      regions, factions with no objectives, kill chains with
      unreachable phases) without running a sim
- [ ] Hover documentation: field docstrings from the Rust types
      surface as hover tooltips
- [ ] Editor "Explain" button that renders the same Markdown in-app
      (the `ExplainReport` struct is the substrate — both producer and
      renderer live in `faultline-stats` so the WASM frontend can call
      them directly without forking)

Status: explain subset shipped; remaining editor work (Monaco,
hover docs, inline validation, browser-side Explain button) still
deferred. Enables Epic F to move faster once the editor work lands.

---

## Round-three follow-ups (codebase health)

Surface review of the round-two epics flagged six structural items
that aren't blocking but compound as later epics layer in. Each is
small enough to ship as a single PR; most can land opportunistically
alongside the next epic that touches the affected area. Two have
since closed (R3-2 round-one, R3-3); the rest are tracked here.

- **R3-1: test-boilerplate sweep (partial).** `Default` impls landed
  on `Scenario`, `Faction`, and supporting types pre-Epic-L so adding
  a top-level field is `..Default::default()` cheap *for new tests*.
  ~30 existing struct-literal call sites were not migrated; an
  opportunistic sweep would make Epic M / N field additions
  essentially free. Acceptance: every existing `Scenario { ... }`
  literal in `crates/**/tests*` migrates to the spread form.
- **R3-2 round two: unread-parameter audit follow-up.** Round one
  wired `command_resilience`, `morale_modifier`, and
  `defender_budget`. Items in priority order:
  1. ~~`ForceUnit.upkeep` and `ForceUnit.mobility`. Both authored in
     every bundled scenario, both unread. `upkeep` pairs with
     per-tick resource drain; `mobility` would gate `movement_phase`
     rate (currently every unit moves 1 region/tick). Movement-rate
     semantics need a policy decision (move accumulator vs. tick-rate
     gate) that's bigger than the audit scope.~~ **Shipped May 2026.**
     `ForceUnit.upkeep` turned out to already be wired (sums per-tick
     over `fs.forces` and deducts from `resources` in
     `tick::attrition_phase`); the original audit missed it. The
     R3-2-round-two PR landed `ForceUnit.mobility` together with
     `TerrainModifier.movement_modifier` and
     `EnvironmentWindow.movement_factor` as a single coupled
     "movement rate" wiring — a per-attempt
     `effective_mobility = mobility × terrain_modifier × env_factor`
     drives a `move_progress` accumulator on the unit, capped at
     `1.0`, with the move firing only when the accumulator reaches
     the threshold. Default authoring (`mobility = 1.0`, terrain
     modifier 1.0, no env windows) preserves legacy
     "moves every tick when queued" behavior. Validation rejects
     three silent-no-op shapes: non-finite or negative mobility,
     non-finite or negative `terrain.movement_modifier`, and
     `EnvironmentWindow.movement_factor` (already covered).
     See the "Unread-parameter audit (R3-2 round two — movement
     rate)" section in `CLAUDE.md`.
  2. ~~`Faction.diplomacy`. Declared by 32 scenarios, mostly empty.
     Wiring is non-trivial (alliance dynamics affect combat
     targeting and political phase). Closes the round-two coalition-
     fracture caveat in Epic D.~~ **Shipped May 2026** as part of
     Epic D round-three item 1 (combat + AI behavioral coupling for
     `Diplomacy::Allied` and `Diplomacy::Cooperative`). Political-
     phase and victory-check coupling remain deferred — open whenever
     a use case appears.
  3. ~~`MediaLandscape.{fragmentation, social_media_penetration,
     internet_availability}` and `PopulationSegment.{activation_threshold,
     activation_actions, volatility}`. The population-segment
     activation mechanic is half-built; finishing it is a small
     epic in its own right.~~ **Shipped May 2026.** The three
     unread `MediaLandscape` fields are now load-bearing on
     `update_civilian_segments` (noise amplification + tension-pull
     dampening) and `information_phase` (disinfo-amplification). The
     other listed `PopulationSegment` fields turned out to be already
     wired (`activation_threshold` / `activation_actions` / `volatility`
     all read in the latch and post-activation processor). The
     "half-built" gap was the missing reporting layer: each activation
     is now logged on `RunResult.civilian_activations`, aggregated
     across runs by
     `MonteCarloSummary.civilian_activation_summaries`, and surfaced
     in a new `## Civilian Activations` report section. Validation
     rejects out-of-range / non-finite media-landscape and segment
     fields. See the "Unread-parameter audit (R3-2 round two —
     population-segment activation)" section in `CLAUDE.md`.
  4. ~~`TechCard.{cost_per_tick, deployment_cost, coverage_limit}`.
     Depend on broadening budget enforcement (covered indirectly
     by `defender_budget` wiring; missing piece is enforcing tech
     activation cost against per-faction running spend).~~ **Shipped
     May 2026.** `deployment_cost` is deducted at engine init in
     `tech_access` declaration order, with cards the faction can't
     afford recorded as denied (skipped, not deployed); `cost_per_tick`
     is deducted in the attrition phase per-tech, with cards whose
     maintenance can't be paid decommissioned for the rest of the run;
     `coverage_limit` (when `Some(n)`) caps the per-tick number of
     (region, opponent) pairs the card contributes to during combat.
     Per-faction `RunResult.tech_costs` records the activity, rolled
     up cross-run by `MonteCarloSummary.tech_cost_summaries` and
     surfaced in a new `## Tech-Card Costs` report section. Validation
     rejects three silent-no-op shapes: non-finite or negative cost
     fields, and `coverage_limit = Some(0)`. See the "Unread-parameter
     audit (R3-2 round two — tech-card costs)" section in `CLAUDE.md`.
  5. `Region.centroid`, `Faction.color`. Visualization metadata
     (used by the WASM frontend). Document as such; not silent
     no-ops by the engine's standards.
  6. `ForceUnit.force_projection`. Declared but zero scenarios set
     it. Drop-or-wire decision; lean towards drop unless an epic
     calls for it.
- **R3-4: generalize the leadership morale cap.** The Epic-D
  leadership cadre couples decapitation to morale via a separate
  per-tick clamp step in `tick::apply_leadership_caps`. A
  `command_effectiveness` multiplier read directly by combat
  (alongside `morale`) would generalize cleanly when round-three
  Epic D adds more command-degrading effects. Worth refactoring
  before the next D stack lands.
- **R3-5: property tests.** ~~Every test today is integration-against-
  fixed-seed. Determinism plus the workspace's seeded RNG policy
  makes property invariants high-value and low-friction with
  `proptest` or `quickcheck`. Acceptance: a `proptest` dev-dep, at
  least one property per module that handles RNG (engine, search,
  network_metrics). Examples worth pinning: "for any seed, no
  faction strength goes negative", "Wilson CI bounds always
  contain the point estimate", "post-disruption network samples
  never have a larger residual capacity than pre-disruption ones".~~
  **Shipped May 2026.** All three pinned invariants plus determinism
  / bounds properties now run against the engine, search, uncertainty,
  and network_metrics modules. See the "Property tests (R3-5)"
  section in `CLAUDE.md` for module layout. Tests in
  `crates/faultline-{engine,stats}/tests/property_*.rs`.
- **R3-6: decompose `Scenario`.** With Epic L landed, `Scenario` has
  14 top-level fields and is approaching the "hard to reason about"
  ceiling. Sub-modules or grouped extension blocks
  (`Scenario.analytics`, `Scenario.adversarial`; `Scenario.networks`
  already exists) would help. Should be designed once enough is
  known to pick the right grouping (probably after Epic M lands
  `BeliefState`).

---

## Working notes

- **Determinism is non-negotiable.** Anything that touches the
  engine or stats must preserve bit-identical output across native
  and WASM for the same seed. Add a regression test whenever a new
  RNG consumer appears. The `verify-bundled` and
  `verify-robustness` CI stages catch drift.
- **Backwards compatibility.** New schema fields must be
  `#[serde(default)]` so existing TOML scenarios load unchanged.
  Schema-breaking changes must ship a migrator in the same PR (Epic
  O policy).
- **Reference sanitization.** The grep guard
  (`tools/ci/grep-guard.sh`) blocks re-introduction of references
  coupling Faultline to a specific external threat-assessment
  publication series. New content uses the field-standard
  vocabulary documented inline in the script.
- **PR granularity.** Each open epic is multiple PRs. Prefer small,
  focused PRs; don't let an epic become a monolith. J and M each
  warrant 3+.
- **Doc maintenance.** This doc is the *running narrative*, not the
  archive. When an epic closes, drop its detailed writeup and leave
  a one-line entry in the closed-epics list. The detailed writeup
  belongs in `CLAUDE.md` (the authoritative description of what
  ships) or in the merged PR description on `main`. Keep this file
  short enough that an analyst can read it in a sitting.
