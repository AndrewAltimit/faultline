# Faultline Roadmap

Current state: Phases 1-6 complete. Phase 7 (scenario library) is ongoing.

---

## Phase 1: Foundation — COMPLETE

- [x] Workspace with all 9 crate skeletons
- [x] All types in `faultline-types` with serde derives
- [x] TOML parsing: `Scenario` loads from `.toml` file
- [x] Basic tick loop in `faultline-engine` (all 8 phases)
- [x] Lanchester attrition model (linear, square, hybrid, stochastic)
- [x] `faultline-cli` with `--single-run` and Monte Carlo modes
- [x] `tutorial_symmetric.toml` scenario
- [x] 116 unit tests across all crates
- [x] CI/CD pipeline (fmt, clippy, test, build, cargo-deny)
- [x] PR validation with Claude Code (security + quality) and OpenRouter/Qwen reviews
- [x] GitHub Pages site

---

## Phase 2: Intelligence — COMPLETE

**Goal:** Faction AI makes non-trivial decisions, fog of war works, events fire reliably.

- [x] Faction AI doctrine variants — distinct weight profiles for Conventional, Guerrilla, Defensive, Disruption, CounterInsurgency, Blitzkrieg, Adaptive; morale-based secondary adjustments
- [x] Fog of war — `FactionWorldView` built per faction with visibility based on controlled regions, force positions, adjacency, and Recon capabilities; `evaluate_actions_fog()` AI path uses partial information
- [x] Event chains — cycle detection via DFS in `EventEvaluator::new()`; chain firing in `event_phase()` with max depth limit
- [x] Technology card terrain modifiers — `apply_tech_effects()` integrated into `combat_phase()`; `CombatModifier` effects extracted per faction with terrain scaling and counter-tech checking
- [x] Civilian segment activation — `update_civilian_segments()` checks activation threshold, returns `ActivationResult`; political phase processes ArmedResistance (militia spawn), Sabotage (infra damage), MaterialSupport, Protest, Flee
- [x] `tutorial_asymmetric.toml` scenario (6 regions, government CounterInsurgency vs insurgent Guerrilla, tech cards, population segments, event chains, fog of war)
- [x] Integration tests: 7 tests covering doctrine weights, event chains + cycle detection, tech-terrain modifiers, civilian activation, fog of war visibility, asymmetric scenario loading

---

## Phase 3: Monte Carlo — COMPLETE

**Goal:** Batch simulation with full statistical output and sensitivity analysis.

- [x] Sensitivity analysis — `--sensitivity` with `--sensitivity-param`, `--sensitivity-range`, `--sensitivity-runs` flags; parameter sweep varies one input across a range, runs Monte Carlo per step, outputs sensitivity.json and sensitivity.csv
- [x] `MonteCarloSummary` regional control — `regional_control` field populated with per-region faction control probabilities from final state
- [x] `MonteCarloSummary` event probabilities — `event_probabilities` computed from complete per-run event logs
- [x] Additional metric distributions — `TotalCasualties`, `InfrastructureDamage`, `ResourcesExpended` computed from final_state vs initial scenario
- [x] Snapshot delta encoding — `DeltaSnapshot` and `DeltaEncodedRun` types with encode/decode roundtrip; only changed fields stored between consecutive snapshots
- [x] `us_institutional_fracture.toml` scenario (4-faction US institutional breakdown: federal government, state coalition, militia movement, foreign influence; 8 macro-regions, 5 infrastructure nodes, 5 tech cards, 4 population segments, 7 events with chains)
- [x] Benchmark: 1000 runs of 365-tick scenario in ~2.2s on modern hardware (well under 60s target)
- [x] CSV event log output (`event_log.csv` — one row per event firing with run_index, tick, event_id)
- [x] Per-run event log — `RunResult.event_log` captures every event firing across all ticks (not just snapshot intervals)
- [x] Final state capture — `RunResult.final_state` always contains terminal `StateSnapshot` regardless of snapshot_interval
- [x] Infrastructure status in snapshots — `StateSnapshot.infra_status` tracks infrastructure health for damage computation
- [x] cargo-deny advisory fix — RUSTSEC-2026-0097 (rand 0.8) exempted with documentation; unused license allowances cleaned up

---

## Phase 4: Browser — COMPLETE

**Goal:** WASM app on GitHub Pages with map visualization and scenario editor.

- [x] WASM compilation pipeline (wasm-pack build in CI, output to `site/pkg/`)
- [x] Canvas/WebGL map renderer with region coloring by controlling faction
- [x] Force unit icons with strength indicators on map
- [x] Scenario editor — TOML text editor with syntax highlighting (CodeMirror/Monaco from CDN)
- [x] Visual faction builder (form-based alternative to raw TOML)
- [x] Map region selector (click regions to configure)
- [x] Scenario import/export via browser file API
- [x] Preset library (bundled example scenarios)
- [x] Simulation controls: play / pause / step / speed slider
- [x] Timeline scrubber (jump to any snapshotted tick)
- [x] Results dashboard: win probability bar chart, duration histogram
- [x] Single-run replay with event log sidebar
- [x] GitHub Pages deployment of WASM artifacts
- [x] Persistent WasmEngine with tick-stepping API for play/pause/step controls
- [x] Monte Carlo batch execution via WASM (`run_monte_carlo` export)
- [x] Three-panel app layout (left sidebar, center canvas, right sidebar)
- [x] Event bus architecture for inter-module communication
- [x] Simulator nav link added to all site pages

---

## Phase 5: Polish — COMPLETE

**Goal:** Production-quality UX, scenario library, documentation.

- [x] Monte Carlo execution in WASM via web workers — `site/js/app/mc-worker.js` runs MC off the UI thread; dashboard speaks a request/response protocol with run-id correlation, with main-thread fallback if `Worker` is unavailable
- [x] Scenario sharing (URL-encoded configs) — gzip + base64url encoded TOML in `#scenario=…` hash; share button copies URL to clipboard, bootstrap auto-loads on page open
- [ ] Interactive tutorial scenario (guided walkthrough)
- [x] Comprehensive `scenario_schema.md` documentation — full schema reference at `docs/scenario_schema.md` covering meta, map, factions, tech, events, politics, simulation, victory
- [x] Sensitivity tornado chart in browser UI — `run_sensitivity_wasm` WASM export plus dashboard sweep controls; chart shows per-faction win-rate ranges sorted by sensitivity
- [x] Regional control heatmap at configurable time slices — MC worker now collects snapshots; dashboard aggregates plurality control per snapshot tick into a faction-colored, alpha-scaled heatmap
- [x] Interactive tutorial walkthrough — step-based overlay tour triggered from sidebar button, highlights key UI elements, persists completion in `localStorage`
- [x] Additional built-in maps — bundled `map-library.js` with US, Europe, East Asia, Middle East geographies; map renderer auto-detects from scenario region IDs
- [~] Map editor — dropped in favor of bundled map library per user feedback
- [ ] Performance optimization (SIMD where available, memory pooling)

---

## Phase 6: Analytical Depth — COMPLETE

**Goal:** Simulation engine produces ETRA-grade analysis — multi-phase kill chains, cost asymmetry modeling, detection/attribution scoring, and defensive gap identification.

### 6.1 — Multi-Phase Campaign Model — COMPLETE

- [x] `CampaignPhase` type — defined in `faultline-types::campaign` with prerequisites, base_success_probability, duration range, detection probability, prerequisite_success_boost, attribution_difficulty, costs, targets_domains, outputs, and branches
- [x] `KillChain` type — `BTreeMap<PhaseId, CampaignPhase>` with `entry_phase` and per-phase `branches` (OnSuccess, OnFailure, OnDetection, Probability, Always)
- [x] Phase prerequisite resolution — `campaign::campaign_phase` activates entry phase, rolls detection each tick, resolves branches on completion; successful prerequisites apply `prerequisite_success_boost` to dependent phases
- [x] Campaign-level Monte Carlo — `MonteCarloSummary.campaign_summaries` aggregates per-phase success/failure/detection/not-reached rates and overall chain success with mean completion ticks
- [x] Compound kill chain visualization in browser — dashboard renders a left-to-right phase flow with success/detection/not-reached bars per phase

### 6.2 — Cost Asymmetry Analysis — COMPLETE (core)

- [x] `AttackerBudget` and `DefenderBudget` tracking — `Scenario.attacker_budget` / `Scenario.defender_budget` caps; `CampaignState.attacker_spend` / `defender_spend` accumulate per-phase dollar costs
- [x] Cost-per-capability annotations — `PhaseCost` struct on each `CampaignPhase` with `attacker_dollars`, `defender_dollars`, `attacker_resources`
- [x] Asymmetry ratio output — `CampaignSummary.cost_asymmetry_ratio` = mean defender spend / mean attacker spend; surfaced in dashboard and Markdown report
- [x] Budget constraint mode — phases cannot activate if attacker budget cap would be exceeded; blocked phases are marked `Failed`
- [~] Cost-effectiveness frontier — basic asymmetry column in feasibility matrix; full frontier chart deferred

### 6.3 — Detection and Attribution Modeling — COMPLETE (core)

- [x] `DetectionProbability` per operation phase — `CampaignPhase.detection_probability_per_tick`; `CampaignState.detection_accumulation` tracks cumulative `1 - product(1 - p_i)`
- [x] `AttributionDifficulty` scoring — `CampaignPhase.attribution_difficulty` in [0,1]; on detection the defender's attribution confidence is set to `1 - attribution_difficulty`
- [x] Detection triggers — detected phase transitions to `Detected` status, `defender_alerted` flag set, tension increases, branches resolve under `OnDetection` condition
- [~] False positive modeling — deferred; defender doesn't model classification error rates
- [x] Attribution confidence output — `CampaignSummary.mean_attribution_confidence` and per-run `CampaignReport.attribution_confidence`

### 6.4 — Doctrinal Seam Analysis — COMPLETE (core)

- [x] `DefensiveDomain` type — enum in `faultline-types::campaign` with PhysicalSecurity, NetworkSecurity, CounterUAS, ExecutiveProtection, CivilianEmergency, SignalsIntelligence, InsiderThreat, SupplyChainSecurity, Custom
- [x] Seam identification — each `CampaignPhase.targets_domains` declares which defensive domains it exploits; phases with ≥2 domains count as cross-domain
- [x] Seam exploitation scoring — `MonteCarloSummary.seam_scores` reports cross-domain phase counts, mean domains/phase, and the share of successful phases that were cross-domain
- [~] Cross-domain response friction and organizational friction — deferred as explicit models; seam exploitation share captures the outcome

### 6.5 — Feasibility Matrix Output — COMPLETE (core)

- [x] Per-scenario feasibility matrix — `MonteCarloSummary.feasibility_matrix` with technology readiness, operational complexity, detection probability, success probability, consequence severity, attribution difficulty, cost asymmetry ratio
- [x] Confidence ratings — `FeasibilityConfidence` with High/Medium/Low derived from MC variance (Wald CI + coefficient of variation on phase success rates)
- [x] Markdown report generation — `faultline_stats::report::render_markdown` produces an ETRA-style document; CLI auto-emits `report.md` alongside JSON summaries
- [~] Sensitivity to assumptions — existing sensitivity sweep works against any parameter; not yet cross-referenced with feasibility columns
- [~] Comparative scenario matrix — structure supports it; UI for comparing multiple scenarios deferred

### 6.6 — Non-Kinetic Outcome Modeling — COMPLETE (core)

- [x] `InformationDominance` metric — `PhaseOutput::InformationDominance` accumulates in `CampaignState` and `SimulationState.non_kinetic`
- [x] `InstitutionalErosion` metric — parallel tracking, also erodes `institution_loyalty` entries proportionally
- [x] `CoercionPressure` metric — same pattern
- [x] `PoliticalCost` metric — same pattern
- [x] Victory conditions based on non-kinetic metrics — new `VictoryType::NonKineticThreshold { metric, threshold }` variant checked against `state.non_kinetic`; Europe Eastern Flank scenario demonstrates with a `CoercionPressure ≥ 0.6` victory for the Russian faction

---

## Phase 7: Scenario Library

**Goal:** Comprehensive library of ETRA-grade scenarios covering major threat archetypes.

- [x] Drone threat capability library — `site/js/app/tech-library.js` bundles ~17 ETRA-derived tech cards (offensive swarms, covert sensors, rogue-AP perching, EW jamming, multi-phenomenology C-UAS, HPM, WIDS fusion, rooftop inspection, MDM, satellite backup). Cards are injectable into any scenario via the browser's Tech Cards panel, with per-faction `tech_access` grant. `scenarios/capabilities_demo.toml` exercises the full library.
- [ ] Drone-assisted coup facilitation (ETRA Scenario 2)
- [ ] Revolutionary infrastructure seizure with drone ISR (ETRA Scenario 3)
- [ ] Asymmetric coercion campaign — proof-of-capability escalation (ETRA Scenario 4)
- [ ] Persistent covert surveillance network (ETRA Scenario 5)
- [ ] Cyber-physical network exploitation via drone-delivered rogue APs (ETRA Scenario 6)
- [ ] Persistent covert sensor emplacement with solar-sustained nodes (ETRA Scenario 7)
- [ ] Compound kill chains (ETRA Appendix D: Alpha, Bravo, Charlie)
- [ ] Taiwan Strait crisis — multi-domain great power competition
- [ ] European energy infrastructure sabotage
- [ ] Arctic sovereignty disputes with drone swarm force projection
- [ ] Domestic critical infrastructure ransomware + physical drone attack convergence

---

## Ongoing

- [ ] Scenario validation improvements — check referential integrity (all IDs resolve), no dangling faction references, probability bounds [0,1], event chain cycle detection
- [ ] Property tests — determinism across platforms (native vs WASM), conservation laws (no strength created from nothing), monotonic clocks
- [ ] Additional scenarios covering different conflict archetypes
- [ ] Playwright screenshot regression tests for all UI features
