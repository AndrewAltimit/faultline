#!/usr/bin/env bash
#
# Epic O schema migration smoke test.
#
# For every bundled scenario, runs `--migrate` to advance the source
# through the registered migration chain, then re-validates the
# emitted TOML. Catches drift between the migrator and the live
# Scenario type — for instance, a future migration that emits a field
# the new struct can't deserialize, or a missing migration step that
# silently leaves an old scenario at an old version.
#
# Currently, schema_version = 1 is the only version, so `--migrate`
# is a no-op and this script's value is mostly forward-looking: it
# locks in the contract that *bundled scenarios always migrate
# cleanly*, so the moment a v2 ships and bundled fixtures fall
# behind, CI fails loudly instead of producing scenarios that load
# only via the silent default-to-1 fallback.
#
# Library-level tests in `crates/faultline-types/src/migration.rs`
# cover the migrator's individual code paths in detail; this script
# is the end-to-end gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Workspace-rooted output dir so the docker-rooted CI container can
# clean it up via the repo-wide ownership-fix step.
WORKDIR="${REPO_ROOT}/output/ci-migrate"
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"

echo "[verify-migration] building faultline-cli..."
cargo build --release -p faultline-cli >/dev/null
BIN="${REPO_ROOT}/target/release/faultline"

# `nullglob` so an empty `scenarios/` directory yields an empty array.
shopt -s nullglob
scenarios=(scenarios/*.toml)
if [[ ${#scenarios[@]} -eq 0 ]]; then
    echo "[verify-migration] no scenarios found under scenarios/; nothing to verify"
    exit 0
fi
fail=0

for scenario in "${scenarios[@]}"; do
    name=$(basename "$scenario" .toml)
    migrated_path="${WORKDIR}/${name}.toml"

    echo "[verify-migration] ${name}: migrate → ${migrated_path}"
    if ! "$BIN" "$scenario" --migrate --quiet >"$migrated_path"; then
        echo "[verify-migration] FAIL: migrate ${name}"
        fail=1
        continue
    fi

    # The migrator is contractually required to stamp meta.schema_version.
    # `grep -F -- 'schema_version'` matches the field name as a literal so a
    # future field rename is caught loudly rather than silently passing.
    if ! grep -q '^schema_version' "$migrated_path"; then
        echo "[verify-migration] FAIL: ${name} — migrated form missing schema_version stamp"
        fail=1
        continue
    fi

    echo "[verify-migration] ${name}: re-validate"
    if ! "$BIN" "$migrated_path" --validate --quiet \
            -o "${WORKDIR}/${name}-validate" >/dev/null 2>&1; then
        echo "[verify-migration] FAIL: ${name} migrated form did not re-validate"
        fail=1
        continue
    fi

    echo "[verify-migration] OK: ${name}"
done

if [[ "$fail" -ne 0 ]]; then
    echo "[verify-migration] one or more scenarios failed migration check"
    exit 1
fi

echo "[verify-migration] all ${#scenarios[@]} scenarios migrated and re-validated cleanly"

# ----------------------------------------------------------------------------
# Negative-path checks
# ----------------------------------------------------------------------------
# These pin the error contract analysts and downstream tooling rely on:
#   1. A scenario authored against a future schema version exits non-zero
#      with a clear "newer than supported" error rather than silently
#      parsing as v1.
#   2. `--migrate --in-place` actually rewrites the source file (catches
#      a future refactor that turns --in-place into a no-op).
#   3. `--migrate` does not start the engine — no manifest.json is
#      emitted, no Monte Carlo runs, the output dir stays untouched.

echo
echo "[verify-migration] running negative-path checks..."

NEG_DIR="${WORKDIR}/neg"
mkdir -p "$NEG_DIR"

# (1) Future-version rejection.
# Take a real scenario, bump schema_version to a far-future value, and
# confirm --migrate fails. We use sed to keep the rest of the scenario
# byte-identical so any failure here is unambiguously about the version
# field, not a side-effect of file content drift.
future_scenario="${NEG_DIR}/tutorial_v999.toml"
sed 's/^schema_version = 1$/schema_version = 999/' \
    scenarios/tutorial_symmetric.toml > "$future_scenario"
if ! grep -q '^schema_version = 999' "$future_scenario"; then
    echo "[verify-migration] FAIL: fixture mutation did not take effect"
    exit 1
fi

if "$BIN" "$future_scenario" --migrate --quiet > "${NEG_DIR}/future.out" 2>"${NEG_DIR}/future.err"; then
    echo "[verify-migration] FAIL: --migrate accepted schema_version=999 (must reject)"
    exit 1
fi
if ! grep -qi 'newer\|supports up to' "${NEG_DIR}/future.err"; then
    echo "[verify-migration] FAIL: --migrate v999 error message missing 'newer/supports up to':"
    cat "${NEG_DIR}/future.err"
    exit 1
fi
echo "[verify-migration] OK: future schema_version rejected with clear message"

# (2) --in-place actually overwrites the source.
# Copy a scenario, strip the schema_version line, run --migrate
# --in-place, and assert the file now contains the field. Catches a
# regression where --in-place silently doesn't write.
inplace_scenario="${NEG_DIR}/tutorial_no_version.toml"
sed '/^schema_version = 1$/d' scenarios/tutorial_symmetric.toml > "$inplace_scenario"
if grep -q '^schema_version' "$inplace_scenario"; then
    echo "[verify-migration] FAIL: failed to strip schema_version from fixture"
    exit 1
fi

if ! "$BIN" "$inplace_scenario" --migrate --in-place --quiet 2>/dev/null; then
    echo "[verify-migration] FAIL: --migrate --in-place returned non-zero"
    exit 1
fi
if ! grep -q '^schema_version' "$inplace_scenario"; then
    echo "[verify-migration] FAIL: --in-place did not stamp schema_version into source file"
    exit 1
fi
echo "[verify-migration] OK: --in-place rewrote the source file with stamped version"

# (3) --migrate does not run the engine.
# The smoke test would catch most regressions here because --migrate
# short-circuits before output-dir creation, but a future change might
# accidentally re-introduce an engine invocation. Confirm no
# manifest.json appears in a fresh output dir.
engine_check_dir="${NEG_DIR}/engine_check_out"
rm -rf "$engine_check_dir"
"$BIN" scenarios/tutorial_symmetric.toml --migrate --quiet \
    -o "$engine_check_dir" > /dev/null
if [[ -f "${engine_check_dir}/manifest.json" ]]; then
    echo "[verify-migration] FAIL: --migrate emitted a manifest.json (it should never run the engine)"
    exit 1
fi
echo "[verify-migration] OK: --migrate did not start the engine"

echo "[verify-migration] all checks (positive + negative) passed"
