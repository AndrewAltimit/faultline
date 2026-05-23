# AGENTS.md

This is the canonical guide for both AI coding agents and humans working in
this repository. `CLAUDE.md` imports this file, so the two never drift.

## Project Overview

Faultline is an analytical research tool for conflict simulation. It takes TOML
scenario configurations and runs deterministic Monte Carlo simulations that
produce probability distributions of outcomes. Primary targets: WASM (browser)
and native CLI.

Faultline is **not** a predictive model — it is a tool for exploring the
consequence space of user-defined assumptions.

All scenario data must be derived from publicly available open-source
intelligence (OSINT). See [LEGAL.md](LEGAL.md) for sourcing requirements and
export control analysis. All code is authored by AI agents under human
direction; no external contributions are accepted (see
[CONTRIBUTING.md](CONTRIBUTING.md)).

## Documentation map

The detailed, subsystem-level documentation lives under [`docs/`](docs/). Start
here when you need depth on a particular area:

| Doc | Covers |
|-----|--------|
| [docs/cli.md](docs/cli.md) | Full CLI reference — every flag, run mode, output artifact, and bundled-archetype demo command. |
| [docs/engine-model.md](docs/engine-model.md) | The per-tick simulation model: phase order, combat, diplomacy, supply, defender capacity, leadership, narrative/displacement, command effectiveness, multi-term utility AI, belief asymmetry. |
| [docs/analytics.md](docs/analytics.md) | What Faultline computes across runs: the 27 Monte Carlo report sections, report module layout, search / robustness / co-evolution, calibration, and scenario explain. |
| [docs/parameter-audit.md](docs/parameter-audit.md) | The R3-2 effort to wire up previously-silent scenario parameters, plus the closed-vs-deferred status. |
| [docs/testing-and-ci.md](docs/testing-and-ci.md) | How to run tests, the property-test suites, the 10-stage CI pipeline, the GitHub Actions workflows, and the advisory exemption. |
| [docs/scenario_schema.md](docs/scenario_schema.md) | The complete scenario TOML schema reference. |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phase/epic history and forward plan. |
| [docs/improvement-plan.md](docs/improvement-plan.md) | Active priorities, open epics, and round-three follow-ups. |

## Build and Test Commands

This is a Cargo workspace. CI runs containerized via Docker, but the commands
work locally too. See [docs/cli.md](docs/cli.md) for the full CLI surface and
[docs/testing-and-ci.md](docs/testing-and-ci.md) for the complete pipeline.

```bash
# Format check, lint (warnings are errors in CI), test, release build, audit
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cargo deny check

# Single crate / single test
cargo test -p faultline-types
cargo test -p faultline-engine -- combat_lanchester

# Run a single simulation, then a Monte Carlo batch
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --single-run
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000

# Build WASM
wasm-pack build crates/faultline-backend-wasm --target web --out-dir ../../site/pkg --no-typescript

# Frontend JS unit tests (Node 22+; uses node:test, no install required)
node --test tests/integration/*.test.mjs

# Match CI exactly (containerized)
docker compose --profile ci run --rm rust-ci cargo test
```

The other run modes — counterfactual, compare, `--search`, `--sensitivity`,
`--robustness`, `--coevolve`, `--migrate`, `--explain`, `--verify` — are
documented with runnable examples in [docs/cli.md](docs/cli.md).

## Code Style

- Rust Edition 2024. Formatting enforced by `rustfmt.toml`: 100-char max line
  width, 4-space indentation, Unix newlines, `Tall` fn params layout.
- Run `cargo fmt --all` before committing. CI rejects unformatted code.
- Clippy warnings are errors in CI: `cargo clippy --all-targets -- -D warnings`.
- Workspace-level lints in root `Cargo.toml`: `clippy::dbg_macro`,
  `clippy::todo`, `clippy::unimplemented`, `clippy::clone_on_ref_ptr` are
  warnings; `clippy::unwrap_used` is **deny**. `unsafe_op_in_unsafe_fn` is a
  warning.
- **No `unwrap()` anywhere** — including tests. Use
  `expect("descriptive reason")` instead.
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

- **Determinism is non-negotiable.** Same config + same seed = identical output
  on native and WASM. Uses `ChaCha8Rng`. Use `BTreeMap` for deterministic
  iteration (**never** `HashMap`). The rendered Markdown report is part of the
  manifest content hash, so any change to report output (section ordering, new
  unconditional text) flips every bundled scenario's `output_hash` and breaks
  `--verify`; the `verify-bundled` CI stage catches this.
- **No `unwrap()`.** Workspace-level `clippy::unwrap_used = "deny"`. All error
  paths must be handled.
- **WASM-compatible engine.** `faultline-engine` must compile to
  `wasm32-unknown-unknown`. No `std::fs`, no `std::thread`, no `rayon` in the
  engine crate. Parallelism lives only in `faultline-cli` (rayon) and
  `faultline-backend-wasm` (web workers).
- All IDs are newtypes wrapping `String` (via the `define_id!` macro in
  `faultline-types/src/ids.rs`).
- All config structs derive `Serialize, Deserialize, Clone, Debug`.
- Technology modifiers are "capability cards" — named bundles of statistical
  effects derived from OSINT.
- Scenarios are TOML files in `scenarios/`. The browser app reads them via
  `site/scenarios/`, a symlink to `../scenarios` so the source of truth lives
  in one place. The GitHub Pages deploy workflow materializes the symlink
  (replaces it with a real copy) before uploading, since the upload only
  includes `site/`.

The per-tick phase order and the contract for each engine behavior are
documented in [docs/engine-model.md](docs/engine-model.md). When adding a new
feature, follow the project pattern: **fail loud at scenario load** (reject
silent-no-op shapes in validation) rather than silently no-op at tick N.

## Scenario Data Policy

Faultline models aggregate statistical effects of real-world systems. When
writing or reviewing scenarios:

- **All capability parameters must be sourceable from public OSINT** (IISS
  Military Balance, CRS reports, congressional testimony, published defense
  analyses, academic literature).
- **Describe effects, not implementations.** A tech card says "detection range
  300km against 1m² RCS" (published spec), not "use X-band phased array with Y
  waveform" (technical data).
- **No classified, CUI, or export-controlled information.** If you can't find
  it in a public source, don't include it.

## Security Considerations

- No OpenAI/Codex integrations — disabled due to security concerns (government
  surveillance partnerships).
- No Google/Gemini integrations — same concerns.
- PR reviews use Claude Code (security + quality profiles) and Qwen 3.7
  (`qwen/qwen3.7-max`) via OpenRouter.

## CI/CD Pipeline

Pipeline stage order: **fmt → clippy → test → build → cargo-deny → grep-guard →
verify-bundled → verify-migration → verify-robustness → js-tests**.

Two GitHub Actions workflows run on self-hosted runners:

- **`main-ci.yml`** — runs on main push and tags. CI stages + WASM build via
  wasm-pack + GitHub Pages deployment. Auto-creates GitHub issues on failure.
- **`pr-validation.yml`** — runs on PRs. CI stages + Claude Code AI review
  (security + quality profiles) + OpenRouter/Qwen 3.7 general review +
  automated agent fix iterations (max 5, extendable with a `[CONTINUE]`
  comment). Add the `no-auto-fix` label to disable automated fixes.

Agent commit authors: `AI Review Agent`, `AI Pipeline Agent`, `AI Agent Bot`.

The custom CI stages (grep-guard, verify-bundled, verify-migration,
verify-robustness, js-tests) and the one active advisory exemption
(`RUSTSEC-2026-0097`) are documented in
[docs/testing-and-ci.md](docs/testing-and-ci.md).
