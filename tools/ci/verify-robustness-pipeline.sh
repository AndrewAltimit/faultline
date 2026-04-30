#!/usr/bin/env bash
#
# Epic I round-two end-to-end smoke test for the robustness pipeline.
#
# Library-level tests in `crates/faultline-stats/tests/epic_i_robustness.rs`
# cover the runner contract; this script exercises the *CLI glue*:
#
#   1. Run --search on the bundled robustness demo to produce a
#      `search.json`.
#   2. Run --robustness --robustness-from-search on that JSON to produce
#      a `robustness.json` + `manifest.json`.
#   3. Replay the robustness manifest with --verify and confirm
#      bit-identical output.
#   4. Tamper with the source `search.json` and confirm --verify rejects
#      with a hash-mismatch error rather than silently re-deriving.
#
# The library-level manifest_replay_produces_identical_output_hash test
# already pins the runner-level invariant; this script catches drift in
# the CLI-side load_robustness_postures / load_robustness_postures
# integration and the verify replay's source-file integrity check.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Workdir is RELATIVE to the repo root because the
# `--robustness-from-search` path-safety check rejects absolute paths
# (the same check `--compare` uses to refuse a crafted manifest from
# reading arbitrary files). The CLI is invoked from $REPO_ROOT below,
# so the relative path resolves correctly.
WORKDIR_REL="output/ci-robustness"
WORKDIR="${REPO_ROOT}/${WORKDIR_REL}"
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"

SCENARIO="scenarios/defender_robustness_demo.toml"

if [[ ! -f "$SCENARIO" ]]; then
    echo "[verify-robustness] expected $SCENARIO to exist; skipping"
    exit 0
fi

echo "[verify-robustness] building faultline-cli..."
cargo build --release -p faultline-cli >/dev/null
BIN="${REPO_ROOT}/target/release/faultline"

# Step 1 — search.
echo "[verify-robustness] step 1: --search"
"$BIN" "$SCENARIO" --search \
    --search-method grid \
    --search-trials 4 \
    --search-runs 8 \
    --search-seed 11 \
    --search-objective "maximize_win_rate:blue" \
    --search-objective minimize_max_chain_success \
    --seed 7 \
    -o "$WORKDIR_REL/search" \
    --quiet

if [[ ! -f "$WORKDIR_REL/search/search.json" ]]; then
    echo "[verify-robustness] FAIL: search did not produce search.json"
    exit 1
fi

# Step 2 — robustness from the search. Pass the WORKDIR-relative
# search.json path so the CLI's absolute-path safety check accepts it.
echo "[verify-robustness] step 2: --robustness --robustness-from-search"
"$BIN" "$SCENARIO" --robustness \
    --robustness-from-search "$WORKDIR_REL/search/search.json" \
    --robustness-runs 8 \
    --robustness-objective "maximize_win_rate:blue" \
    --robustness-objective minimize_max_chain_success \
    --seed 7 \
    -o "$WORKDIR_REL/robustness" \
    --quiet

if [[ ! -f "$WORKDIR_REL/robustness/manifest.json" ]]; then
    echo "[verify-robustness] FAIL: robustness did not produce manifest.json"
    exit 1
fi
if [[ ! -f "$WORKDIR_REL/robustness/robustness.json" ]]; then
    echo "[verify-robustness] FAIL: robustness did not produce robustness.json"
    exit 1
fi

# Step 3 — replay must be bit-identical.
echo "[verify-robustness] step 3: --verify"
if ! "$BIN" "$SCENARIO" --verify "$WORKDIR_REL/robustness/manifest.json" \
        -o "$WORKDIR_REL/replay" --quiet; then
    echo "[verify-robustness] FAIL: replay did not match manifest"
    exit 1
fi

# Step 4 — tamper with the source search.json and confirm verify
# rejects. The robustness manifest stored the SHA-256 of the source
# file; mutating any byte must flip the hash and cause verify to bail
# with a clear error.
echo "[verify-robustness] step 4: hash-mismatch rejection on tampered source"
echo " " >> "$WORKDIR_REL/search/search.json"
if "$BIN" "$SCENARIO" --verify "$WORKDIR_REL/robustness/manifest.json" \
        -o "$WORKDIR_REL/replay-tampered" --quiet 2>/dev/null; then
    echo "[verify-robustness] FAIL: verify accepted tampered search.json"
    exit 1
fi
echo "[verify-robustness] OK: tampered source correctly rejected"

echo "[verify-robustness] all checks passed"
