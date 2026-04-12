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

# Build WASM
wasm-pack build crates/faultline-backend-wasm --target web --out-dir ../../site/pkg --no-typescript
```

CI pipeline order: **fmt -> clippy -> test -> build -> cargo-deny**.

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
- Scenarios are TOML files in `scenarios/`.

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
