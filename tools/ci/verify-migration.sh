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
