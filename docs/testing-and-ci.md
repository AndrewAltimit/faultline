# Testing and CI

This document describes how Faultline is tested and what each stage of the CI pipeline
does. The full pipeline runs on every push to `main` and on every pull request.

---

## Running tests locally

### Rust tests

```bash
# Run the full workspace test suite
cargo test

# Run a single crate's tests
cargo test -p faultline-types

# Run a specific test by name (substring match)
cargo test -p faultline-engine -- combat_lanchester
```

### JS frontend tests

The JS tests require Node 22+ and have no install step — they depend only on `node:test`,
which ships with the runtime.

```bash
node --test tests/integration/*.test.mjs
```

These tests cover the pure-logic frontend modules: sharing roundtrip, heatmap aggregation,
the Pinned MC results store, the comparison-delta computation (mirrors
`faultline_stats::counterfactual::compute_delta`), the LCS unified-diff renderer, the
grep-guard CI script, and the `site/scenarios` symlink contract. They run on the host (not
inside the Rust CI container); CI provisions the runtime with `actions/setup-node@v4`.

### Containerized (matches CI exactly)

```bash
docker compose --profile ci run --rm rust-ci cargo test
```

---

## Property tests

Added in R3-5, the `proptest`-backed suites pin *invariants* across the whole input space
rather than checking specific outputs at a single seed. They live alongside the existing
fixed-seed integration tests.

### Why property tests

Faultline's correctness guarantee is that the same config + seed produces identical output
on every platform. Fixed-seed integration tests catch "this output is wrong at seed 42".
They do not catch "an unrelated refactor introduced a `HashMap` somewhere in the trial
pipeline and now run-to-run output is non-deterministic under most seeds." Property tests
do, because they exercise arbitrary seeds drawn by `proptest` on every run.

The seeded-RNG / `BTreeMap`-iteration determinism contract is exactly the invariant
property tests are suited for. The four current suites target the modules that either
handle RNG directly or compute statistical bounds.

### The four suites

**`crates/faultline-engine/tests/property_invariants.rs`**

For any seed: faction `total_strength` >= 0 across every snapshot, faction morale stays
in `[0, 1]`, tension stays in `[0, 1]`, and two engine runs at the same seed produce
bit-identical `RunResult` JSON (the determinism contract that `--verify` depends on).
Uses `scenarios/tutorial_symmetric.toml` via `include_str!` to exercise a realistic
engine path. Proptest budget is 16 cases so the 4 properties x ~100 ticks each finish
under a second.

**`crates/faultline-stats/tests/property_uncertainty.rs`**

Wilson bounds always contain the point estimate, narrow monotonically with sample size,
and `wilson_from_rate` agrees with the count form. Bootstrap CI is bit-identical for the
same `(values, seed)` and always satisfies `lower <= upper`.

**`crates/faultline-stats/tests/property_network_metrics.rs`**

Disrupting nodes or zeroing edges never *increases* max-flow on a static topology (the
canonical monotone-degradation invariant), max-flow is always non-negative and finite, and
Brandes betweenness scores stay in `[0, 1]` with descending-rank output.

**`crates/faultline-stats/tests/property_search.rs`**

Same `search_seed` => bit-identical `SearchResult` JSON across random seeds, every
trial's continuous assignments stay in `[low, high]`, every grid-mode trial hits one of
the enumerated `enumerate_levels` values, and the Pareto frontier is strictly ascending
and in-bounds. Uses an inline minimal scenario fixture (two regions, two factions, 30 max
ticks, `num_runs = 2`) so the engine path stays fast under proptest's 24-case budget.

### How to add a property test

Adding a new property is one new `#[test]` inside an existing `proptest!` block in the
relevant file — no scaffolding is required since `proptest` is already a workspace
dev-dependency on every relevant crate. Keep fixtures scenario-minimal so the suite stays
fast.

---

## CI pipeline stages

The full 10-stage pipeline order is:

```
fmt -> clippy -> test -> build -> cargo-deny -> grep-guard -> verify-bundled -> verify-migration -> verify-robustness -> js-tests
```

| Stage | What it does |
|---|---|
| `fmt` | `cargo fmt --all -- --check` — rejects unformatted code |
| `clippy` | `cargo clippy --all-targets -- -D warnings` — warnings are errors |
| `test` | `cargo test` — full workspace unit + integration + property suite |
| `build` | `cargo build --release` — confirms the release binary links cleanly |
| `cargo-deny` | `cargo deny check` — license audit and advisory check against `deny.toml` |
| `grep-guard` | `tools/ci/grep-guard.sh` — blocks banned reference patterns (see below) |
| `verify-bundled` | `tools/ci/verify-bundled-scenarios.sh` — replay all 28 bundled scenarios for bit-identical output |
| `verify-migration` | `tools/ci/verify-migration.sh` — migrate every bundled scenario and re-validate |
| `verify-robustness` | `tools/ci/verify-robustness-pipeline.sh` — end-to-end search -> robustness -> verify flow |
| `js-tests` | `node --test tests/integration/*.test.mjs` — pure-logic JS frontend tests |

### grep-guard (`tools/ci/grep-guard.sh`)

Blocks any commit that re-introduces references coupling Faultline to a specific external
threat-assessment publication series. The banned patterns, the whitelist, and the
rationale are documented inline in the script itself.

```bash
# Run locally — exit 0 = clean, exit 1 = banned-pattern match found
./tools/ci/grep-guard.sh
```

### verify-bundled (`tools/ci/verify-bundled-scenarios.sh`)

Emits a `manifest.json` for every TOML in `scenarios/` and replays each one via
`faultline-cli --verify` to confirm bit-identical output. The rendered Markdown is part
of the manifest content hash, so any change to section ordering or unconditional output
is caught here before it leaks into a release. Currently covers 28 bundled scenarios.

```bash
./tools/ci/verify-bundled-scenarios.sh
```

### verify-migration (`tools/ci/verify-migration.sh`)

Runs `faultline-cli --migrate` on every TOML in `scenarios/` and re-validates the
migrated form. Catches drift between the schema migration framework and the bundled
scenarios. Schema versioning lives in `crates/faultline-types/src/migration.rs`; see
`docs/scenario_schema.md` for the schema-evolution policy.

```bash
./tools/ci/verify-migration.sh
```

### verify-robustness (`tools/ci/verify-robustness-pipeline.sh`)

Exercises the full `--search -> --robustness --robustness-from-search -> --verify` flow
against `scenarios/defender_robustness_demo.toml`, then tampers with the source
`search.json` and confirms `--verify` rejects on hash mismatch. This catches CLI-glue
regressions in the search-then-robustness flow that the library-level tests in
`crates/faultline-stats/tests/epic_i_robustness.rs` cannot reach.

```bash
./tools/ci/verify-robustness-pipeline.sh
```

---

## GitHub Actions workflows

Both workflows run on self-hosted runners.

### `main-ci.yml`

Triggered on push to `main` and on version tags. Runs:

1. The full 10-stage CI pipeline described above.
2. WASM build via `wasm-pack build`.
3. GitHub Pages deployment of the browser frontend.

Auto-creates a GitHub issue when any stage fails.

### `pr-validation.yml`

Triggered on pull requests. Runs:

1. The full 10-stage CI pipeline.
2. Claude Code AI review (security and quality profiles).
3. OpenRouter / Qwen 3.7 (`qwen/qwen3.7-max`) general code review.
4. Automated agent fix iterations — up to 5 rounds, extendable by posting a `[CONTINUE]`
   comment on the PR. Add the `no-auto-fix` label to disable automated fixes entirely.

**Agent commit authors:** `AI Review Agent`, `AI Pipeline Agent`, `AI Agent Bot`.

---

## Known advisory exemptions

One advisory is currently exempted in `deny.toml`:

**`RUSTSEC-2026-0097`** — `rand` 0.8 unsound only when a custom logger calls
`rand::rng()` and `ThreadRng` reseeds inside that logger. Faultline uses `tracing` (not
`log`) and never calls rand from a logging context, so the unsound path is unreachable.
Upgrading to `rand` 0.9+ requires coordinated updates across `rand_chacha`, `rand_distr`,
`statrs`, and `nalgebra`; that is planned for a future release.

All other `cargo deny check` checks pass clean.
