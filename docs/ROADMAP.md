# Faultline Roadmap

Current state: Phases 1-4 complete.

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

## Phase 5: Polish

**Goal:** Production-quality UX, scenario library, documentation.

- [ ] Monte Carlo execution in WASM via web workers
- [ ] Scenario sharing (URL-encoded configs or gist integration)
- [ ] Interactive tutorial scenario (guided walkthrough)
- [ ] Comprehensive `scenario_schema.md` documentation
- [ ] Map editor (define custom geographies in browser)
- [ ] Additional built-in maps (individual US states, abstract grids)
- [ ] Performance optimization (SIMD where available, memory pooling)
- [ ] Sensitivity tornado chart in browser UI
- [ ] Regional control heatmap at configurable time slices

---

## Phase 6: Analytical Depth

**Goal:** Simulation engine produces ETRA-grade analysis — multi-phase kill chains, cost asymmetry modeling, detection/attribution scoring, and defensive gap identification.

### 6.1 — Multi-Phase Campaign Model

The current engine models each tick independently. Real threat campaigns are multi-phase kill chains where intelligence from early phases (sensor emplacement, wireless recon) directly enables later phases (credential harvest, coercion, kinetic action). Phase 6.1 adds a campaign layer on top of the tick engine.

- [ ] `CampaignPhase` type — named phase with prerequisites (prior phases), success probability, duration range, and output effects
- [ ] `KillChain` type — ordered sequence of `CampaignPhase`s with branching (e.g., coercion vs kinetic after intelligence acquisition)
- [ ] Phase prerequisite resolution — a phase cannot begin until its prerequisites have completed; intelligence outputs from prior phases modify success probability of subsequent phases
- [ ] Campaign-level Monte Carlo — run the full kill chain N times, producing probability distributions over which phases succeed, at what tick, and which branch is taken
- [ ] Compound kill chain visualization in browser — Sankey diagram or flow chart showing phase progression with probability annotations

### 6.2 — Cost Asymmetry Analysis

The ETRA's central finding is that defense costs orders of magnitude more than attack. The engine should quantify this.

- [ ] `AttackerBudget` and `DefenderBudget` tracking — separate resource pools with explicit dollar-denominated costs per capability
- [ ] Cost-per-capability cards — each tech card carries acquisition cost, recurring cost, and deployment timeline (not just abstract resource units)
- [ ] Asymmetry ratio output — for each scenario, compute the ratio of defender investment required to close each gap vs attacker investment to exploit it
- [ ] Budget constraint mode — run simulations with defender budget caps to identify which gaps remain open at each funding level
- [ ] Cost-effectiveness frontier visualization — chart showing defensive coverage vs investment, with diminishing returns visible

### 6.3 — Detection and Attribution Modeling

Current engine treats combat as symmetric Lanchester attrition. Covert operations are fundamentally about detection probability, not force-on-force combat.

- [ ] `DetectionProbability` per operation phase — each campaign phase has a per-tick detection probability that accumulates over time (the longer you operate, the more likely you're caught)
- [ ] `AttributionDifficulty` scoring — post-incident, how hard is it to identify the actor? Scored by hardware traceability, operational footprint, forensic recoverability
- [ ] Detection triggers — when an operation is detected, model the defender's response (investigation, perimeter hardening, public disclosure) and its effect on subsequent phases
- [ ] False positive modeling — defender classification systems have false positive rates; each false positive (destroying a CNN drone, investigating a utility sensor that's actually legitimate) carries political and resource cost
- [ ] Attribution confidence output — Monte Carlo produces probability distribution over attribution outcomes (definitive identification, analytical assessment, no attribution)

### 6.4 — Doctrinal Seam Analysis

The ETRA identifies that the most dangerous attacks exploit gaps between defensive disciplines (physical security, cyber, C-UAS). The engine should model this explicitly.

- [ ] `DefensiveDomain` type — physical security, network security, counter-UAS, executive protection, each with coverage area, response time, and organizational owner
- [ ] Seam identification — automatically detect regions/operations that fall between two or more defensive domains with no single owner
- [ ] Cross-domain correlation — model the delay and friction of cross-domain incident response (C-UAS detects drone → physical security investigates rooftop → IT security checks wireless → 45-minute loop vs 15-second attack window)
- [ ] Seam exploitation scoring — for each scenario, compute how much of the attack success probability comes from exploiting inter-domain gaps vs overcoming any single domain
- [ ] Organizational friction model — model the "not my job" heuristic where no single role owns verification of exterior equipment, off-site staff device security, or rooftop inspection

### 6.5 — Feasibility Matrix Output

The ETRA uses structured feasibility tables. The engine should produce them automatically.

- [ ] Per-scenario feasibility matrix — technology readiness, operational complexity, detection probability, success probability, consequence severity, attribution difficulty
- [ ] Confidence ratings — each feasibility factor has a confidence level (high/medium/low) based on the variance in Monte Carlo outcomes
- [ ] Sensitivity to assumptions — which feasibility factors change the outcome most? (connects to existing sensitivity analysis)
- [ ] Comparative scenario matrix — side-by-side feasibility comparison across scenarios (which is cheapest, most likely to succeed, hardest to attribute)
- [ ] PDF/Markdown report generation — export analysis results as a structured document matching ETRA format

### 6.6 — Non-Kinetic Outcome Modeling

Current engine measures outcomes as territorial control and force elimination. Many ETRA scenarios succeed through coercion, information operations, or institutional erosion — not kinetic action.

- [ ] `InformationDominance` metric — who controls the narrative? Measured by media coverage, public trust delta, and narrative coherence
- [ ] `InstitutionalErosion` metric — cumulative damage to institutional trust, legitimacy, and operational effectiveness
- [ ] `CoercionPressure` metric — the credible threat level imposed by demonstrated capability (even without kinetic use)
- [ ] `PoliticalCost` metric — the cost to the defender of overreaction (security theater, restricted events, civil liberties impact)
- [ ] Victory conditions based on non-kinetic metrics — coercion succeeds when the target changes policy, not when territory changes hands

---

## Phase 7: Scenario Library

**Goal:** Comprehensive library of ETRA-grade scenarios covering major threat archetypes.

- [ ] Autonomous drone swarm executive decapitation (ETRA Scenario 1)
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
