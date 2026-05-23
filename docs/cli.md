# Faultline CLI Reference

`faultline-cli` is the headless command-line runner for Faultline scenario simulations. It reads a TOML scenario file, runs deterministic Monte Carlo simulations, and writes analytical reports to an output directory.

**Binary invocation pattern:**

```bash
cargo run -p faultline-cli -- <SCENARIO> [flags]
```

All flags documented below were verified against `crates/faultline-cli/src/main.rs`.

---

## Global Flags

These flags apply to every invocation. Run-mode flags and schema-operation flags are documented in their own sections below.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `SCENARIO` | path (positional, required) | — | Path to the `.toml` scenario file |
| `-n`, `--runs` | u32 | `1000` | Number of Monte Carlo runs |
| `-s`, `--seed` | u64 (optional) | random | RNG seed; omit for a fresh seed each invocation |
| `-o`, `--output` | path | `./output` | Output directory (created if absent) |
| `-f`, `--format` | `json\|csv\|both` | `both` | Output format for tabular data |
| `-j`, `--jobs` | u32 (optional) | num CPUs | Parallelism (CLI only; WASM uses web workers) |
| `-v`, `--verbose` | flag | off | Emit per-run progress to stderr |
| `--quiet` | flag | off | Suppress all non-error output |

---

## Run Modes

Run modes are mutually exclusive. Omitting all of them runs the default Monte Carlo batch.

### Monte Carlo Batch (default)

Runs `-n` simulations and produces the full analytical report.

**Artifacts written to `--output`:**

- `summary.json` — `MonteCarloSummary` with win rates, kill-chain feasibility, network resilience, defender capacity, supply pressure, narrative dynamics, displacement flows, belief asymmetry, utility decomposition, calibration, and correlation matrix
- `runs.csv` — per-run scalar table
- `report.md` — human-readable Markdown report
- `manifest.json` — scenario hash + MC config + output hash (consumed by `--verify`)

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000
```

---

### Single Run (`--single-run`)

Runs the engine exactly once and writes the full `RunResult` as JSON. Useful for inspecting per-tick state, civilian activations, kill-chain phase traces, and other per-run detail that the MC summary elides.

**Artifacts:** `single_run.json`

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --single-run
```

---

### Sensitivity Analysis (`--sensitivity`)

Sweeps a single parameter across a range and summarises how output metrics respond. Produces a structured sensitivity report rather than a full MC summary.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--sensitivity-param <PARAM>` | required | Dot-path to the parameter (same syntax as `--counterfactual`) |
| `--sensitivity-range <LOW:HIGH:STEPS>` | `0.1:0.9:5` | Range specification |
| `--sensitivity-runs <N>` | `100` | MC runs per parameter value |

**Artifacts:** `sensitivity.json`, `sensitivity.csv`

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml \
    --sensitivity \
    --sensitivity-param "faction.alpha.initial_morale" \
    --sensitivity-range "0.2:0.9:8" \
    --sensitivity-runs 200
```

---

### Counterfactual (`--counterfactual`)

Overrides one or more scenario parameters, runs MC on both the baseline and the modified scenario, and produces a delta report comparing the two. The `--counterfactual` flag is repeatable.

**Artifacts:** `comparison.json`, `comparison_report.md`

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --counterfactual "faction.alpha.initial_morale=0.3"
```

Multiple overrides:

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --counterfactual "faction.alpha.initial_morale=0.3" \
    --counterfactual "faction.alpha.initial_resources=50.0"
```

---

### Side-by-Side Comparison (`--compare`)

Runs MC on two separate scenario files and produces a unified delta report. The first positional argument is the baseline; `--compare` names the comparator.

**Artifacts:** `comparison.json`, `comparison_report.md`

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --compare scenarios/tutorial_asymmetric.toml
```

---

### Strategy Search (`--search`)

Samples the `[strategy_space]` declared in the scenario, evaluates each trial via MC, and reports the best-by-objective trials plus the non-dominated Pareto frontier. Uses its own seed (`--search-seed`) independent of the inner MC seed so trial-to-trial deltas isolate parameter effects from sampling noise.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--search-method <random\|grid>` | `random` | Sampling strategy |
| `--search-trials <N>` | `32` | Number of parameter assignments to evaluate |
| `--search-runs <N>` | `100` | MC runs per trial |
| `--search-seed <SEED>` | random | Seed for trial sampling (inner MC seed is set by `--seed`) |
| `--search-objective <OBJ>` | — | Optimisation objective; repeatable |

**Available objectives:**

- `maximize_win_rate:<faction_id>` — maximise the named faction's win rate
- `minimize_duration` — minimise mean run length in ticks
- `minimize_max_chain_success` — minimise the maximum kill-chain success rate
- `maximize_detection` — maximise mean kill-chain detection rate
- `minimize_attacker_cost` — minimise mean attacker spend
- `maximize_attacker_cost` — maximise mean attacker spend (e.g. for a defender objective)
- `minimize_defender_cost` — minimise mean defender spend
- `maximize_cost_asymmetry` — maximise attacker cost relative to defender cost

**Artifacts:** `search.json`, `search_report.md`

```bash
# Attacker-side strategy search
cargo run -p faultline-cli -- scenarios/strategy_search_demo.toml \
    --search --search-trials 16 --search-runs 50 \
    --search-method grid \
    --search-objective maximize_win_rate:alpha \
    --search-objective minimize_duration

# Defender-posture optimisation
cargo run -p faultline-cli -- scenarios/defender_posture_optimization.toml \
    --search --search-trials 8 --search-runs 30 \
    --search-method grid \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    --search-objective maximize_detection
```

---

### Robustness Analysis (`--robustness`)

Evaluates every defender posture (typically the Pareto frontier from a prior `--search`) against every attacker profile declared in `[strategy_space.attacker_profiles]` and ranks postures by their worst-case profile. The expected analyst flow is `--search` then `--robustness --robustness-from-search`.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--robustness-from-search <PATH>` | — | Path to a `search.json` produced by `--search` |
| `--robustness-runs <N>` | `100` | MC runs per (posture × attacker profile) cell |
| `--robustness-objective <OBJ>` | — | Ranking objective; repeatable (same syntax as `--search-objective`) |
| `--robustness-skip-baseline` | flag | Skip the natural-state baseline evaluation |

**Artifacts:** `robustness.json`, `robustness_report.md`

```bash
# Step 1 — search for Pareto-optimal defender postures
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --search --search-method grid --search-trials 8 --search-runs 16 \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    -o ./output/search_phase

# Step 2 — evaluate those postures against every attacker profile
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --robustness \
    --robustness-from-search ./output/search_phase/search.json \
    --robustness-runs 16 \
    --robustness-objective "maximize_win_rate:blue" \
    --robustness-objective minimize_max_chain_success \
    -o ./output/robustness_phase
```

If `--robustness-from-search` is omitted the runner evaluates the natural-state baseline posture against all declared attacker profiles.

---

### Adversarial Co-evolution (`--coevolve`)

Runs an alternating best-response loop between an attacker and a defender. Each round the active side re-optimises only the `[strategy_space]` variables it owns against the opponent's currently-frozen assignment. The loop terminates at Nash equilibrium, on cycle detection, or when `--coevolve-rounds` is exhausted.

Both `--coevolve-attacker` and `--coevolve-defender` must own at least one `[strategy_space]` variable (via `owner = "<faction>"`).

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--coevolve-attacker <FACTION>` | required | Attacker faction id |
| `--coevolve-defender <FACTION>` | required | Defender faction id |
| `--coevolve-attacker-objective <OBJ>` | required | Attacker optimisation objective |
| `--coevolve-defender-objective <OBJ>` | required | Defender optimisation objective |
| `--coevolve-method <random\|grid>` | `grid` | Search method for each side's sub-space |
| `--coevolve-trials <N>` | `8` | Trials per side per round |
| `--coevolve-runs <N>` | `30` | MC runs per trial |
| `--coevolve-rounds <N>` | `8` | Maximum rounds before declaring `NoEquilibrium` |
| `--coevolve-seed <SEED>` | random | Seed for round-by-round trial sampling |
| `--coevolve-initial-mover <attacker\|defender>` | `defender` | Which side moves first in round 0 |

**Artifacts:** `coevolve.json`, `coevolve_report.md`

A `COEVOLVE <status> rounds=N manifest_hash=...` line is also printed to stdout for CI scripts.

```bash
cargo run -p faultline-cli -- scenarios/coevolution_demo.toml --coevolve \
    --coevolve-attacker red --coevolve-defender blue \
    --coevolve-attacker-objective "maximize_win_rate:red" \
    --coevolve-defender-objective minimize_max_chain_success \
    --coevolve-method grid \
    --coevolve-trials 4 --coevolve-runs 10 \
    --coevolve-rounds 6 --coevolve-seed 1
```

---

### Determinism Verification (`--verify`)

Replays a saved `manifest.json` and asserts that the engine produces bit-identical output. Prints `VERIFY OK` on success or `VERIFY FAILED` with a diff on mismatch.

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml \
    --verify ./output/manifest.json
```

---

## Schema Operations

These modes do not run the engine. They are mutually exclusive with all run modes and with each other.

### Validate (`--validate`)

Loads and validates the scenario TOML against the current schema. Prints the validity result and any validation errors to stdout.

```bash
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --validate
```

---

### Migrate (`--migrate`)

Upgrades the scenario TOML to the current schema version. Prints the upgraded TOML to stdout by default. Pass `--in-place` to rewrite the source file.

```bash
# Print migrated TOML to stdout
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate

# Rewrite in place
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate --in-place
```

---

### Explain (`--explain`)

Produces a structured "what does this scenario model?" summary without running the engine. The Markdown output covers: scenario header, scale counts, factions, kill chains, victory conditions, networks, the `[strategy_space]` decision-variable surface, and any author-flagged Low-confidence parameters.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--explain-format <markdown\|json>` | `markdown` | Output format |

```bash
# Markdown (default)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --explain

# JSON (structured ExplainReport)
cargo run -p faultline-cli -- scenarios/strategy_search_demo.toml \
    --explain --explain-format json
```

---

## Bundled Archetype Demos

These commands exercise the bundled demonstration scenarios. Each scenario is designed to showcase one or more engine mechanics.

### Tutorial scenarios

```bash
# Symmetric baseline — good starting point for counterfactual exploration
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000

# Asymmetric variant — compares cleanly against the symmetric baseline
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --compare scenarios/tutorial_asymmetric.toml
```

### Network Resilience (Epic L)

Supply and communications graphs under scripted interdiction. The report's "Network Resilience" section shows per-network mean/max disrupted-node and component counts plus the Brandes critical-node ranking on the static topology.

```bash
cargo run -p faultline-cli -- scenarios/network_resilience_demo.toml -n 16
```

### Supply-Network Interdiction (Epic D round-three item 2)

A Blue defender owns two `kind = "supply"` networks. A scripted attacker chains three interdiction events that progressively cut Blue's residual supply capacity. The report's "Supply Pressure" section quantifies the resulting per-tick income attenuation — pressure = residual / baseline, multiplied into `resource_rate` every attrition tick.

```bash
cargo run -p faultline-cli -- scenarios/supply_interdiction_demo.toml -n 16
```

### Multi-Front Resource Contention (Epic D round-three item 3)

A 3-tier SOC defender (tier-1 triage -> tier-2 IR -> tier-3 forensics) with declared cross-role escalation policy. Tier-1 saturates first, spills to tier-2 at 80% capacity; tier-2 saturates next, spills to tier-3 at 70%; tier-3 (terminal) absorbs the residual. The report's "Defender Capacity" section gains a "Cross-role escalation" sub-table whose `In` / `Out` columns trace the spillover chain by inspection.

```bash
cargo run -p faultline-cli -- scenarios/multifront_soc_escalation.toml -n 16
```

### Alert Fatigue SOC (Epic K)

Single-tier SOC baseline demonstrating the defender capacity model without cross-role escalation.

```bash
cargo run -p faultline-cli -- scenarios/alert_fatigue_soc.toml -n 100
```

### Calibration Scaffold (Epic N)

The scenario declares a `[meta.historical_analogue]` block with three observations (Winner, WinRate, DurationTicks). The report's `## Calibration` section computes a per-observation verdict (Pass/Marginal/Fail) plus a roll-up. Scenarios without a declared analogue render a "purely synthetic" disclaimer in the same section.

```bash
cargo run -p faultline-cli -- scenarios/calibration_demo.toml -n 100
```

### Narrative Competition + Displacement Flows (Epic D round-three item 4)

Two-region archetype with three factions: Red and Blue push competing `MediaEvent` narratives (Red reinforces twice, Blue once); a scripted `Displacement` event seeds 30% displaced fraction in `frontier_north` that propagates to `frontier_south` over the run; a population segment's `Flee` action adds organic displacement once its sympathy crosses the activation threshold. The report's `## Narrative Dynamics` section ranks per-faction information dominance and per-narrative trajectory (firing rate, peak strength, modal favored faction); the `## Displacement Flows` section captures peak / mean / inflow / outflow per region.

```bash
cargo run -p faultline-cli -- scenarios/narrative_competition_demo.toml -n 16
```

### Belief Asymmetry / False Flag (Epic M round-one)

Two factions, four regions in a square. Alpha runs a `DeceptionOp::FalseForceStrength` campaign that plants a phantom 500-strength force in Bravo's belief state (Alpha's actual infantry is 100). Bravo's AI consumes the belief-derived world view and reads the phantom as real, shifting posture against a non-existent threat. Mid-run, an `IntelligenceShare::FactionResources` event refreshes Bravo's belief about Alpha's resources from ground truth (modelling exfiltrated intel). The report's `## Belief Asymmetry` section captures per-faction mean force-strength error, deception event counts, and terminal-deceived belief counts.

```bash
cargo run -p faultline-cli -- scenarios/false_flag_demo.toml -n 16
```

### Coalition Fracture (Epic D round-two)

The scenario declares two `alliance_fracture` rules on a Cooperative `gray_partner` faction: one trips on attribution accumulation against `red_attacker`'s kill chain, the other on political tension. The report's `## Alliance Dynamics` section ranks per-rule fire rate, mean fire tick, and terminal-stance distribution across runs.

As of Epic D round-three item 1, the post-fracture stance is consumed by combat targeting and AI decision-making. The victory-check phase still ignores diplomacy.

```bash
cargo run -p faultline-cli -- scenarios/coalition_fracture_demo.toml -n 32
```

### Strategy Search (Epic H round-one)

Demonstrates `[strategy_space]` decision variables with attacker-side objectives.

```bash
cargo run -p faultline-cli -- scenarios/strategy_search_demo.toml \
    --search --search-trials 16 --search-runs 50 \
    --search-method grid \
    --search-objective maximize_win_rate:alpha \
    --search-objective minimize_duration
```

### Defender-Posture Optimisation (Epic I round-one)

Uses `--search` with defender-aligned objectives. The report's Counter-Recommendation section ranks Pareto-frontier postures against the do-nothing baseline.

```bash
cargo run -p faultline-cli -- scenarios/defender_posture_optimization.toml \
    --search --search-trials 8 --search-runs 30 \
    --search-method grid \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    --search-objective maximize_detection
```

### Defender-Posture Robustness (Epic I round-two)

Full search-then-robustness pipeline. Evaluates every defender posture against every attacker profile declared in `[strategy_space.attacker_profiles]` and ranks postures by worst-case profile.

```bash
# Phase 1 — identify Pareto-optimal defender postures
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --search --search-method grid --search-trials 8 --search-runs 16 \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    -o ./output/search_phase

# Phase 2 — rank those postures by worst-case attacker profile
cargo run -p faultline-cli -- scenarios/defender_robustness_demo.toml \
    --robustness \
    --robustness-from-search ./output/search_phase/search.json \
    --robustness-runs 16 \
    --robustness-objective "maximize_win_rate:blue" \
    --robustness-objective minimize_max_chain_success \
    -o ./output/robustness_phase
```

### Adversarial Co-evolution (Epic H round-two)

Both sides own at least one `[strategy_space]` variable via the `owner = "<faction>"` tag. `--coevolve-method grid` enumerates each side's full sub-space per round; the loop terminates when the joint state stabilises across two consecutive rounds (Nash equilibrium), when a cycle of any period >= 2 is detected, or at `--coevolve-rounds`.

```bash
cargo run -p faultline-cli -- scenarios/coevolution_demo.toml --coevolve \
    --coevolve-attacker red --coevolve-defender blue \
    --coevolve-attacker-objective "maximize_win_rate:red" \
    --coevolve-defender-objective minimize_max_chain_success \
    --coevolve-method grid \
    --coevolve-trials 4 --coevolve-runs 10 \
    --coevolve-rounds 6 --coevolve-seed 1
```

### Adaptive Utility / Multi-Term AI (Epic J round-one)

Two factions with contrasting utility profiles: `red` is a control-maximizing aggressor with a deadline-pressure trigger; `blue` is a cautious defender with a morale-panic trigger. Demonstrates the full `[utility]` mechanic end-to-end. The report's `## Utility Decomposition` section shows per-faction term means and per-trigger fire rates.

```bash
cargo run -p faultline-cli -- scenarios/adaptive_utility_demo.toml -n 100
```

---

## WASM Build and JS Tests

### Build the browser WASM package

Outputs to `site/pkg/`. Requires [wasm-pack](https://rustwasm.github.io/wasm-pack/).

```bash
wasm-pack build crates/faultline-backend-wasm --target web --out-dir ../../site/pkg --no-typescript
```

### Run frontend JS unit tests

Requires Node 22+. Uses the built-in `node:test` runner; no `npm install` required.

```bash
node --test tests/integration/*.test.mjs
```

The JS tests cover: sharing roundtrip, heatmap aggregation, the Pinned MC results store, the comparison-delta computation, the LCS unified-diff renderer, the grep-guard CI script, and the `site/scenarios` symlink contract.
