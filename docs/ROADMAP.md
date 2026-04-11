# Faultline Roadmap

Current state: Phases 1-3 complete.

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

## Phase 4: Browser

**Goal:** WASM app on GitHub Pages with map visualization and scenario editor.

- [x] WASM compilation pipeline (wasm-pack build in CI, output to `site/pkg/`)
- [ ] Canvas/WebGL map renderer with region coloring by controlling faction
- [ ] Force unit icons with strength indicators on map
- [ ] Scenario editor — TOML text editor with syntax highlighting (CodeMirror/Monaco from CDN)
- [ ] Visual faction builder (form-based alternative to raw TOML)
- [ ] Map region selector (click regions to configure)
- [ ] Scenario import/export via browser file API
- [ ] Preset library (bundled example scenarios)
- [ ] Simulation controls: play / pause / step / speed slider
- [ ] Timeline scrubber (jump to any snapshotted tick)
- [ ] Results dashboard: win probability bar chart, duration histogram
- [ ] Single-run replay with event log sidebar
- [x] GitHub Pages deployment of WASM artifacts

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

## Ongoing

- [ ] Scenario validation improvements — check referential integrity (all IDs resolve), no dangling faction references, probability bounds [0,1], event chain cycle detection
- [ ] Property tests — determinism across platforms (native vs WASM), conservation laws (no strength created from nothing), monotonic clocks
- [ ] Additional scenarios covering different conflict archetypes
