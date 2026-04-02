# Faultline Roadmap

Current state: Phase 1 complete, portions of Phases 2-3 scaffolded.

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

## Phase 2: Intelligence

**Goal:** Faction AI makes non-trivial decisions, fog of war works, events fire reliably.

- [ ] Faction AI doctrine variants — currently all factions use the same utility weights regardless of `Doctrine` enum value; implement distinct behavior for Guerrilla, Defensive, Disruption, CounterInsurgency, Blitzkrieg
- [ ] Fog of war — `fog_of_war` config flag exists but `FactionWorldView` is not populated with partial information; factions currently see everything
- [ ] Event chains — `chain` field on `EventDefinition` is parsed but chained events are not activated when a parent fires
- [ ] Technology card terrain modifiers — `faultline-tech` resolves effects but the engine doesn't fully integrate terrain-adjusted tech effects into combat/politics phases
- [ ] Civilian segment activation — `activation_threshold` and `activation_actions` are defined but `update_civilian_segments()` only drifts sympathies; it doesn't spawn militia, trigger sabotage, or produce intelligence
- [ ] `tutorial_asymmetric.toml` scenario (insurgency vs conventional)
- [ ] Integration tests: event chains fire correctly, AI adapts to doctrine, fog of war limits detection

---

## Phase 3: Monte Carlo

**Goal:** Batch simulation with full statistical output and sensitivity analysis.

- [ ] Sensitivity analysis — `--sensitivity` flag is accepted but prints "not yet implemented"; implement parameter sweep (vary one input, measure outcome variance)
- [ ] `MonteCarloSummary` regional control — `regional_control` field exists in types but `compute_summary()` doesn't populate per-region control probabilities
- [ ] `MonteCarloSummary` event probabilities — `event_probabilities` field not yet computed
- [ ] Snapshot delta encoding — full state snapshots work but 365 snapshots x 1000 runs may be memory-heavy; implement delta encoding
- [ ] `us_institutional_fracture.toml` scenario (multi-faction US institutional breakdown)
- [ ] Benchmark: 1000 runs of 365-tick scenario under 60s on modern hardware
- [ ] CSV event log output (one row per event firing across all runs)

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
