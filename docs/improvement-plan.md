# Faultline Improvement Plan

The running narrative of what's left and *why* — the ordering of open
work, not the archive. Detailed writeups of shipped work live in the
subsystem docs under [`docs/`](.) (engine-model, analytics,
parameter-audit, testing-and-ci) and in the git history; this file
deliberately does **not** duplicate them. Keep it short enough to read
in a sitting.

**Last refresh: 2026-06-06 (afternoon).** A four-stream parallel
landing:

- **Schema-aware hover documentation** in the browser scenario editor —
  hovering (or caret-resting on) a TOML key surfaces its type, range,
  default, and an "engine effect vs. descriptive only" badge, sourced
  from a field-doc catalog covering ~115 distinct keys across the whole
  schema.
- **Three more single-event calibration analogues** — a 1999 coercive
  air campaign, a 2007 nation-scale DDoS availability campaign, and a
  2012 sector-targeted destructive-wiper incident, each constrained by
  published OSINT and calibrating *Pass* on winner / win-rate /
  duration. The bundled analogue set is now **6**, inside the 5–10
  target band.
- **Self-describing scenario metadata** — the `[meta]` block gained
  `analytical_purpose`, `scenario_type`, `osint_sources`,
  `red_team_profile`, `blue_team_posture`, and `sensitivity_parameters`,
  all optional and fail-loud-validated (present-but-empty shapes are
  rejected at load). Backfilled across the bundled set, surfaced in the
  `explain` view, and kept out of the deterministic Monte Carlo report
  so no bundled output shifted.
- **Standoff-strike force projection** — `ForceUnit.force_projection`
  (declared but previously dead) is now wired: a unit with a
  `StandoffStrike { range, damage }` projects attrition into hostile
  regions within a graph-hop reach of its own region without moving,
  respecting diplomacy. Gated on `force_projection.is_some()`, so every
  legacy scenario stays bit-identical. New gated `## Force Projection`
  report section brings the Monte Carlo report to 29 sections.

Prior refresh (**2026-06-06 morning**): believed-attribution rolls
closed Epic M; the historical-analogue framework gained its first three
single-event analogues; the browser Explain button and inline
validation panel shipped; the tech-card library was rebalanced.

---

## Open priorities

The highest-leverage open work, in order. Two strategic facts frame the
whole list and are covered in the next section: (a) the calibration
foundation the analyst use case needs is now in place with a real
analogue set, and (b) an analyst-vs-game-middleware decision is pending
that re-ranks several of these items.

1. **Make the strategic call: analyst tool or game middleware.** This
   is the highest-leverage *decision*, not a feature. It re-ranks
   everything below (UI vs. streaming API, calibration vs. player
   agency). The calibration foundation is now solid enough that the
   analyst path is viable; the engine primitives are now rich enough
   (belief asymmetry, kill chains, force projection, adaptive utility)
   that the game-middleware path is viable too. Deciding unblocks a
   coherent next epic instead of hedged half-steps. See the next
   section.

2. **Epic E — UI identity & analytical density.** The largest remaining
   user-facing gap on the analyst path. The engine now produces far
   more than the UI surfaces well (belief asymmetry, force projection,
   narrative/displacement, calibration verdicts). Highest-value
   sub-items: chart polish (KDE overlays, confidence bands,
   colorblind-safe palette), a map-canvas treatment that renders real
   geography rather than abstract grids, and a denser feasibility view
   (radar / parallel-coordinates) replacing the wide table. Several
   sub-items depend on the editor work in Epic P.

3. **Epic P — schema-aware editor.** Authoring depth's last open piece.
   The explain view, browser Explain button, inline validation panel,
   and (this refresh) hover documentation have all shipped. What
   remains is the Monaco/CodeMirror editor with TOML grammar +
   JSON-schema-driven autocomplete (schema generated from the Rust
   types). This is the single biggest authoring-reliability win as the
   schema keeps growing, and it directly enables Epic F content work.

4. **Epic F — remaining scenario-library content.** The self-describing
   `[meta]` fields and the tech-card rebalance have shipped. What
   remains is net-new content: a healthcare/critical-infrastructure
   capability-card set (still under-represented), browser metadata
   form-fields for the new `[meta]` block, and a few net-new flagship
   scenarios (ransomware + drone convergence; a Strait crisis;
   supply-chain weaponization). Content is cheap to add and compounds
   the demo surface.

5. **Codebase health — `Scenario` decomposition (R3-6) and the
   test-boilerplate sweep (R3-1).** `Scenario` now carries 14
   top-level fields and is near the "hard to reason about" ceiling;
   grouped extension blocks (`Scenario.analytics`,
   `Scenario.adversarial`) would help, and the right grouping is now
   knowable since the belief/projection fields have landed. Separately,
   ~30 existing struct-literal call sites in tests still use the
   explicit form; migrating them to the spread/`Default` form makes
   future field additions free (this refresh's metadata + projection
   work each had to touch a spread of fixtures by hand — exactly the
   tax R3-1 removes).

---

## Strategic decision pending — analyst tool vs. game middleware

A standing observation: Faultline's engineering discipline
(determinism, replay manifests, schema versioning, seeded RNG, kill-
chain + capacity + network primitives, belief asymmetry, adaptive
utility, strategy search, counterfactual replay) maps almost exactly
onto what good game middleware needs — and games don't have the
calibration problem that constrains the analyst use case. The same
properties that make Faultline a credible research tool make it a
strong game-AI substrate.

**This is a real fork, not a committed direction.** Recording it so the
trade-off is explicit when the next epic is chosen.

**Genre fit, ranked.** Excellent: heist / stealth (kill chains + alert
fatigue), espionage / political sims, insurgency / asymmetric warfare,
grand strategy / 4X (faction AI + escalation + leadership cadres),
emergent-faction roguelikes. Possible: tabletop GM tooling, browser
strategy. Bad: twitch action, narrative-first, puzzle, sports.

**How the open priorities re-rank under each branch:**

| Open item | Analyst path | Game-middleware path |
| --- | --- | --- |
| Epic E — UI density | **required** (the product is the report) | designer tooling only |
| Epic P — schema editor | author convenience | **high** (designers need it) |
| Epic F — content | optional depth | **high** (content is the product) |
| More calibration analogues | **the trust payoff** | skip |
| Streaming `step()` API | not needed | **critical** (new work) |
| Player-agency model | not needed | **critical** (new work) |

**Engineering gaps if the pivot is taken** (none of these exist yet):

1. **No streaming API.** The engine runs scenarios to completion; games
   need `step()` / `apply_action()` / `query_state()`. The tick loop is
   single-pass with no step-isolation boundary; wrapping it means
   designing a suspension/resume protocol over `SimulationState`.
   Spike-then-estimate, not a week.
2. **No player-agency model.** Scenarios assume both factions are
   AI-driven. A "player faction" abstraction would consume runtime
   inputs instead of the scripted strategy space.
3. **No stable mid-run save/load.** `SimulationState` derives
   `Serialize`, but the runtime maps (`network_states`,
   `diplomacy_overrides`, `fired_fractures`, `defender_queues`,
   `metric_history`, the belief stores) were added for one-shot
   post-run analytics, not round-tripping. A real save/load needs a
   versioned on-disk runtime-state format (the scenario-migration
   groundwork helps but doesn't cover runtime state).
4. **Performance unknown for frame budgets.** Faultline runs scenarios
   in seconds; games need ms/frame. Needs benchmarks; offline
   pre-computation probably carries most of the load.
5. **Authoring UX is for analysts.** Visual editor need; Epic P helps
   but doesn't fully close it.

**Sequencing if the pivot is taken:** spike a streaming API as a
separate crate → build one demo game (stealth is the cleanest fit —
kill chains + alert fatigue are plug-and-play) → revisit the adaptive-AI
and belief machinery under the new framing → open-source as middleware.
Further calibration work is dropped under this branch.

---

## Open epics & follow-ups (detail)

### Epic E — UI identity & analytical density

Move from "generic SaaS dark-mode" to "purpose-built defense-analysis
instrument." Items: gradient discipline, headline font + faultline
accent motif, map-canvas treatment (real geography, not grids), chart
polish (gridlines, KDE overlays, confidence bands, colorblind-safe
palette), radar / parallel-coordinates replacement for the dense
feasibility table, map pan/zoom + label collision avoidance + kill-chain
phase overlays, dashboard progress + cancel for long Monte Carlo runs,
export to PNG/CSV/JSON/PDF, addressable run URLs, light-mode toggle. The
editor-overlap items depend on Epic P. Status: open; the largest
user-facing surface left on the analyst path.

### Epic P — authoring depth: schema-aware editor

Shipped: the `explain` CLI subset, the browser Explain button, the
inline validation panel (`scenario_warnings_wasm`), and schema-aware
hover documentation. Remaining: a Monaco / CodeMirror editor with TOML
grammar and JSON-schema-driven autocomplete (schema generated from the
Rust types), so fields are completed and validated as the schema grows.
Hover docs already proved out a field-doc catalog the autocomplete can
reuse. Status: one item left.

### Epic F — scenario library & content

Shipped: the self-describing `[meta]` fields with validation and a
full bundled backfill; the tech-card rebalance (SIGINT, supply-chain,
SCADA/ICS, GPS-denial, deepfake cards). Remaining: a healthcare /
critical-infrastructure capability-card set; browser metadata
form-fields that author the new `[meta]` block; net-new flagship
scenarios (ransomware + drone convergence, a Strait crisis,
supply-chain weaponization). Status: open content work, low risk, high
demo value.

### Epic N — calibration discipline

The framework (schema, Pass/Marginal/Fail verdict ladder, always-emit
`## Calibration` section, synthetic-scenario disclaimer,
methodology-appendix confidence tag) is complete, and the reference set
now holds **6** single-event analogues spanning kinetic coercion, cyber
availability, infrastructure attack, supply-chain compromise, and
destructive malware — inside the 5–10 target.

**Modeling note for future analogue authors:** the combat model resolves
attrition only where opposing forces co-locate and the AI won't assault
a defended region, so a clean kinetic "Winner-by-conquest" verdict is
not reliably reachable at realistic force ratios over a short horizon.
The reliable path to a calibratable `Winner` + `DurationTicks` outcome
is a **kill chain driving a non-kinetic accumulator (`CoercionPressure`)
past a `NonKineticThreshold` victory condition** — every bundled
analogue uses this pattern.

Remaining (optional, polish-grade): 2–4 more analogues for richer
coverage; the fabricated-narrative-integration tie-in; deciding whether
calibration verdicts should gate the `verify-bundled` CI step (today
they don't — output is bit-stable but a verdict can be `Fail` without
breaking CI). Under the game-middleware pivot, none of this is needed.

### Codebase health

- **Decompose `Scenario` (R3-6).** 14 top-level fields, near the
  reasoning ceiling. Grouped extension blocks (`Scenario.analytics`,
  `Scenario.adversarial`; `Scenario.networks` already exists) would
  help. The right grouping is now knowable — the belief, utility, and
  projection fields that were the unknowns have all landed.
- **Test-boilerplate sweep (R3-1).** `Default` impls exist on the major
  config structs, but ~30 existing `Scenario { … }` / `ScenarioMeta { … }`
  / `ForceUnit { … }` literals in tests still use the explicit form.
  Migrating them to the spread form makes every future field addition
  free. Acceptance: every existing struct literal in `crates/**/tests*`
  uses `..Default::default()`.

---

## What's shipped (compact)

Faultline has closed ~38 epics/round-items across uncertainty
quantification, counterfactual + comparison analysis, time/attribution
dynamics, engine-model depth (coalition fracture, supply interdiction,
multi-front contention, narrative + displacement), strategy search +
adversarial co-evolution, defender-posture optimization + robustness,
adaptive multi-term utility AI, information warfare + belief asymmetry
(including believed-attribution rolls), defender-capacity queue
dynamics, network primitives, the calibration framework, schema
versioning + replay manifests, the unread-parameter audit (now
including `force_projection`), the leadership/command-effectiveness
split, property-test coverage, the `report.rs` decomposition, and the
authoring/editor surface (explain, validation panel, hover docs).

The authoritative description of *what currently ships* lives in the
subsystem docs — [engine-model.md](engine-model.md),
[analytics.md](analytics.md), [parameter-audit.md](parameter-audit.md),
[testing-and-ci.md](testing-and-ci.md), and
[scenario_schema.md](scenario_schema.md) — plus the merged PR
descriptions on `main`. This plan no longer carries the per-epic
closeout manifests; the git history is the archive.

---

## Working notes

- **Determinism is non-negotiable.** Anything touching the engine or
  stats must preserve bit-identical output across native and WASM for
  the same seed. New RNG consumers must be gated behind their opt-in
  flag so legacy scenarios consume RNG in the exact legacy order. Add a
  regression test whenever a new RNG consumer appears; the
  `verify-bundled` and `verify-robustness` CI stages catch drift. The
  rendered Markdown report is part of the content hash, so a new
  *unconditional* report section flips every bundled scenario's output
  — keep new sections gated on data presence.
- **Backwards compatibility.** New schema fields must be
  `#[serde(default)]` so existing TOML loads unchanged. Schema-breaking
  changes ship a migrator in the same PR.
- **Fail loud at load.** Reject silent-no-op shapes in validation
  rather than silently no-op at tick N. Every field added this way has
  accept + reject unit tests.
- **Reference sanitization.** The grep guard (`tools/ci/grep-guard.sh`)
  blocks re-introduction of references coupling Faultline to a specific
  external threat-assessment publication series. New content uses the
  field-standard vocabulary documented inline in the script.
- **PR granularity.** Prefer small, focused PRs; don't let an epic
  become a monolith. When parallel streams touch a shared type, expect
  a one-line fixup where their literals meet — cheaper than serializing
  the work.
- **Doc maintenance.** This file is the running narrative, not the
  archive. When an epic closes, drop its detail and leave it to the
  subsystem docs and git history. Resist the urge to grow a closeout
  manifest here — that is exactly the lopsidedness this refresh removed.
