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
      "this is ETRA-candidate"); feeds into an at-a-glance report
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
- [ ] Dashboard: "Pin Results" + side-by-side comparison mode
- [ ] Scenario diff viewer in the TOML editor
- [x] Report: "Policy Implications" and "Countermeasure Analysis"
      sections

**Status:** Epic B **mostly complete on the analysis side**. First PR
landed the three schema extensions (all `#[serde(default)]` for
backwards compatibility), the `--counterfactual` and `--compare`
CLI modes built on an extended `set_param` path layer that now reaches
kill-chain phase parameters, a new `faultline_stats::counterfactual`
module producing a `ComparisonReport` with per-faction win-rate deltas
and per-chain feasibility deltas, and the two new report sections.
The remaining two items are WASM frontend work (pin results,
side-by-side dashboard, scenario diff viewer in the TOML editor) and
will ship as a follow-up UI-focused PR.

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
