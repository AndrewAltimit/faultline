# CLAUDE.md

This file provides guidance to AI coding agents working with code in this repository.

## Project Overview

Faultline is an analytical research tool for conflict simulation. It takes TOML scenario configurations and runs deterministic Monte Carlo simulations producing probability distributions of outcomes. Primary targets: WASM (browser) and native CLI.

All scenario data must be derived from publicly available open-source intelligence (OSINT). See [LEGAL.md](LEGAL.md) for sourcing requirements and export control analysis.

All code is authored by AI agents under human direction. No external contributions are accepted (see `CONTRIBUTING.md`).

## Build and Test Commands

This is a Cargo workspace. All CI runs containerized via Docker but the commands work locally:

```bash
# Format check
cargo fmt --all -- --check

# Lint (warnings are errors in CI)
cargo clippy --all-targets -- -D warnings

# Run all tests
cargo test

# Run a single crate's tests
cargo test -p faultline-types

# Run a specific test by name
cargo test -p faultline-engine -- combat_lanchester

# Build release
cargo build --release

# License and advisory audit
cargo deny check

# Run a single simulation
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --single-run

# Run Monte Carlo batch
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000

# Counterfactual override + delta report (Epic B)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --counterfactual "faction.alpha.initial_morale=0.3"

# Side-by-side comparison of two scenarios (Epic B)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --compare scenarios/tutorial_asymmetric.toml

# Replay a saved manifest and assert bit-identical output (Epic Q)
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml \
    --verify ./output/manifest.json

# Migrate a scenario forward to the current schema version (Epic O)
# Prints to stdout by default; --in-place rewrites the source file.
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --migrate --in-place

# Build WASM
wasm-pack build crates/faultline-backend-wasm --target web --out-dir ../../site/pkg --no-typescript

# Run frontend JS unit tests (Node 22+; uses node:test, no install required)
node --test tests/integration/*.test.mjs
```

## Analytics surfaced in `report.md` (Epic C)

Beyond the win-rate / feasibility / kill-chain tables that earlier
epics shipped, every Monte Carlo run now also emits:

- **Time & Attribution Dynamics** — per-chain time-to-first-detection
  (right-censored when never detected), defender-reaction-time
  distribution (gap from first detection to run end), and per-phase
  Kaplan-Meier survival curves with cumulative hazard. Sections elide
  when the chain produces no signal.
- **Pareto Frontier** — non-dominated runs across (attacker cost,
  success, stealth = `1 - max chain detection`). Surfaces the
  achievable trade-off envelope before reaching for a sweep.
- **Output Correlation Matrix** — Pearson correlations across the
  six built-in per-run scalars (duration, casualties, attacker /
  defender spend, mean attribution, max detection). Constant series
  show as `—` (correlation undefined; deliberately not zero).

The schema for all five outputs lives on `MonteCarloSummary` /
`CampaignSummary` in `crates/faultline-types/src/stats.rs`. The
producers are pure functions of `RunResult` data and live in
`crates/faultline-stats/src/time_dynamics.rs` — they never re-run
the engine. Morris elementary-effects screening (the
variance-decomposition replacement for pure OAT sensitivity sweeps)
lives in `crates/faultline-stats/src/morris.rs`; not currently CLI-
exposed but callable from library consumers.

`BranchCondition::EscalationThreshold` (Epic C) adds hysteresis to
phase branching — a branch that only fires when a global metric has
stayed on the requested side of a threshold for `sustained_ticks`
consecutive end-of-tick snapshots. The engine sizes its rolling
metric-history buffer to the longest window any branch in the
scenario asks for; legacy scenarios with no such branch pay zero
overhead. Schema reference is in `docs/scenario_schema.md` under
`PhaseBranch`.

CI pipeline order: **fmt -> clippy -> test -> build -> cargo-deny -> grep-guard -> verify-bundled -> verify-migration -> js-tests**.

The JS tests cover the pure-logic frontend modules (sharing roundtrip,
heatmap aggregation, the Pinned MC results store, the comparison-delta
computation that mirrors `faultline_stats::counterfactual::compute_delta`,
the LCS unified-diff renderer, the grep-guard CI script, and the
site/scenarios symlink contract). They run on the host (not in the
rust-ci container) and only depend on `node:test`; CI provisions the
runtime with `actions/setup-node@v4`.

The grep-guard stage (`tools/ci/grep-guard.sh`) blocks any commit that
re-introduces references coupling Faultline to a specific external
threat-assessment publication series. The patterns it bans, the
whitelist, and the rationale are documented inline in the script. To
run it locally: `./tools/ci/grep-guard.sh` — exit 0 = clean, exit 1 =
banned-pattern match found.

The verify-bundled stage (`tools/ci/verify-bundled-scenarios.sh`)
emits a `manifest.json` for every TOML in `scenarios/` and replays
each one via `faultline-cli --verify` to confirm bit-identical
output. Catches drift in the determinism contract before it leaks
into a release. Run locally: `./tools/ci/verify-bundled-scenarios.sh`.

The verify-migration stage (`tools/ci/verify-migration.sh`) runs
`faultline-cli --migrate` on every TOML in `scenarios/` and
re-validates the migrated form. Catches drift between the schema
migration framework and the bundled scenarios. Schema versioning
lives in `crates/faultline-types/src/migration.rs`; see
`docs/scenario_schema.md` for the schema-evolution policy. Run
locally: `./tools/ci/verify-migration.sh`.

To match CI exactly (containerized):
```bash
docker compose --profile ci run --rm rust-ci cargo test
```

## Code Style

- Rust Edition 2024. Formatting enforced by `rustfmt.toml`: 100-char max line width, 4-space indentation, Unix newlines, `Tall` fn params layout.
- Run `cargo fmt --all` before committing. CI rejects unformatted code.
- Clippy warnings treated as errors in CI: `cargo clippy --all-targets -- -D warnings`.
- Workspace-level lints in root `Cargo.toml`: `clippy::dbg_macro`, `clippy::todo`, `clippy::unimplemented`, `clippy::clone_on_ref_ptr` are warnings; `clippy::unwrap_used` is deny. `unsafe_op_in_unsafe_fn` is a warning.
- **No `unwrap()` anywhere** — including tests. Use `expect("descriptive reason")` instead.
- Edition 2024: `gen` is a keyword — use `r#gen` for random generation calls.

## Workspace Structure

```
crates/
  faultline-types/       # Shared data structures (zero logic, leaf crate)
  faultline-geo/         # Geography, maps, terrain (depends on: types)
  faultline-tech/        # Technology capability cards (depends on: types)
  faultline-politics/    # Political climate, loyalty (depends on: types)
  faultline-events/      # Event system (depends on: types)
  faultline-engine/      # Core simulation loop (depends on: types, geo, tech, politics, events)
  faultline-stats/       # Monte Carlo runner (depends on: engine, types)
  faultline-backend-wasm/# Browser WASM frontend (depends on: engine, stats, types)
  faultline-cli/         # Headless CLI runner (depends on: engine, stats, types)
```

## Architecture

- **Determinism is non-negotiable.** Same config + same seed = identical output on native and WASM. Uses `ChaCha8Rng`. Use `BTreeMap` for deterministic iteration (never `HashMap`).
- **No `unwrap()`.** Workspace-level `clippy::unwrap_used = "deny"`. All error paths must be handled.
- **WASM-compatible engine.** `faultline-engine` must compile to `wasm32-unknown-unknown`. No `std::fs`, no `std::thread`, no `rayon` in the engine crate. Parallelism lives only in `faultline-cli` (rayon) and `faultline-backend-wasm` (web workers).
- All IDs are newtypes wrapping `String` (defined via `define_id!` macro in `faultline-types/src/ids.rs`).
- All config structs derive `Serialize, Deserialize, Clone, Debug`.
- Technology modifiers are "capability cards" — named bundles of statistical effects derived from OSINT.
- Scenarios are TOML files in `scenarios/`. The browser app reads them via `site/scenarios/`, which is a symlink to `../scenarios` so the source of truth lives in one place. The GitHub Pages deploy workflow materializes the symlink (replaces it with a real copy) before uploading the artifact, since the upload only includes `site/`.
- The browser tech-card library at `site/js/app/tech-library.js` records each card's open-source provenance via `source_ref` (a domain-generic descriptor — *not* a citation to any specific publication). Adding a card with a section-level fingerprint to a specific external document will fail the grep-guard CI stage.

## Scenario Data Policy

Faultline models aggregate statistical effects of real-world systems. When writing or reviewing scenarios:

- **All capability parameters must be sourceable from public OSINT** (IISS Military Balance, CRS reports, congressional testimony, published defense analyses, academic literature).
- **Describe effects, not implementations.** A tech card says "detection range 300km against 1m² RCS" (published spec), not "use X-band phased array with Y waveform" (technical data).
- **No classified, CUI, or export-controlled information.** If you can't find it in a public source, don't include it.

## Security Considerations

- No OpenAI/Codex integrations — disabled due to security concerns (government surveillance partnerships).
- No Google/Gemini integrations — same concerns.
- PR reviews use Claude Code (security + quality profiles) and Qwen 3.5 via OpenRouter.

## CI/CD Pipeline

Two GitHub Actions workflows on self-hosted runners:

- **`main-ci.yml`** — Runs on main push and tags. CI stages (fmt, clippy, test, build, cargo-deny), WASM build via wasm-pack, GitHub Pages deployment. Auto-creates GitHub issues on failure.
- **`pr-validation.yml`** — Runs on PRs. CI stages + Claude Code AI review (security + quality profiles) + OpenRouter/Qwen 3.5 general review + automated agent fix iterations (max 5, extendable with `[CONTINUE]` comment). Add `no-auto-fix` label to disable automated fixes.

Agent commit authors: `AI Review Agent`, `AI Pipeline Agent`, `AI Agent Bot`.

## Known Advisory Exemptions

One advisory is currently exempted in `deny.toml`:

- `RUSTSEC-2026-0097` — rand 0.8 unsound only when a custom logger calls `rand::rng()` and `ThreadRng` reseeds inside that logger. Faultline uses `tracing` (not `log`) and never calls rand from a logging context. Upgrading to rand 0.9+ requires coordinated updates across `rand_chacha`, `rand_distr`, `statrs`, and `nalgebra` and is planned for a future release.

`cargo deny check` otherwise passes clean.
