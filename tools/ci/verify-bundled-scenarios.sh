#!/usr/bin/env bash
#
# Epic Q reproducibility smoke test.
#
# For each bundled scenario, runs a small Monte Carlo batch to emit a
# manifest, then re-runs --verify against the same scenario file and
# asserts the replay is bit-identical. Catches drift between the
# determinism contract and the CLI wiring (e.g. a future refactor
# that introduces a non-deterministic codepath).
#
# Run-count is intentionally tiny — this is a smoke test, not a
# stress test. The library-level tests in
# `crates/faultline-stats/tests/report_integration.rs` cover the
# determinism contract in detail; this script just confirms the CLI
# glue still works end-to-end.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Use a fixed temp directory under the workspace so docker-rooted
# CI containers can clean up via the repo-wide ownership-fix step.
WORKDIR="${REPO_ROOT}/output/ci-verify"
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"

# Tiny, deterministic batch — 5 runs is enough to exercise the
# manifest hash + replay path on every scenario without ballooning CI
# time.
RUNS=5
SEED=20260425

# Pre-build once so per-scenario invocations don't recompile.
echo "[verify-bundled] building faultline-cli..."
cargo build --release -p faultline-cli >/dev/null
BIN="${REPO_ROOT}/target/release/faultline"

scenarios=(scenarios/*.toml)
fail=0

for scenario in "${scenarios[@]}"; do
    name=$(basename "$scenario" .toml)
    out="${WORKDIR}/${name}"
    rm -rf "$out"
    mkdir -p "$out"

    echo "[verify-bundled] ${name}: emit"
    "$BIN" "$scenario" -n "$RUNS" -s "$SEED" -o "$out" --quiet \
        || { echo "[verify-bundled] FAIL: emit ${name}"; fail=1; continue; }

    if [[ ! -f "${out}/manifest.json" ]]; then
        echo "[verify-bundled] FAIL: ${name} produced no manifest.json"
        fail=1
        continue
    fi

    echo "[verify-bundled] ${name}: verify"
    if ! "$BIN" "$scenario" --verify "${out}/manifest.json" \
            -o "${out}/replay" --quiet; then
        echo "[verify-bundled] FAIL: replay ${name} did not match manifest"
        fail=1
        continue
    fi

    echo "[verify-bundled] OK: ${name}"
done

if [[ "$fail" -ne 0 ]]; then
    echo "[verify-bundled] one or more scenarios failed reproducibility check"
    exit 1
fi

echo "[verify-bundled] all $((${#scenarios[@]})) scenarios verified"
