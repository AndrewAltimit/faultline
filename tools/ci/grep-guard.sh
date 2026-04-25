#!/usr/bin/env bash
#
# Reference-sanitization guard.
#
# Faultline is a generic conflict-simulation tool. To preserve the
# legal posture in LEGAL.md ("analytical tool, not paired with any
# specific operational publication"), the codebase must not contain
# references to specific external threat-assessment series. This
# script fails CI if any of those patterns re-enter the tree.
#
# Patterns blocked:
#   \bETRA\b           — the bare acronym
#   etra_ref           — the previous schema field name
#   ETRA-YYYY-         — specific document identifiers (e.g. ETRA-2026-WMD-001)
#
# Whitelist:
#   docs/improvement-plan.md  — Epic G's section describes the cleanup
#                                itself and legitimately mentions the
#                                patterns it bans.
#   docs/ARCHIVE/             — archived docs may preserve historical
#                                terminology; reviewable as a separate
#                                policy decision per file.
#
# Usage:
#   ./tools/ci/grep-guard.sh
#   exit 0 → clean
#   exit 1 → matches found (printed to stdout)
set -euo pipefail

PATTERN='\bETRA\b|etra_ref|ETRA-[0-9]{4}-'

# File-type allowlist: only scan source files. Skips binaries, build
# artifacts, vendored deps, and the rendered docs/ output.
INCLUDES=(
  --include='*.rs'
  --include='*.toml'
  --include='*.md'
  --include='*.html'
  --include='*.css'
  --include='*.js'
  --include='*.mjs'
  --include='*.yml'
  --include='*.yaml'
  --include='*.sh'
)

# Directory excludes: build outputs, generated WASM bundle, git internals.
EXCLUDES=(
  --exclude-dir=target
  --exclude-dir=node_modules
  --exclude-dir=pkg
  --exclude-dir=.git
)

# Per-file whitelist (pruned from the match list after scanning).
#   docs/improvement-plan.md  — Epic G section legitimately mentions
#                               the patterns it bans (it describes the
#                               cleanup itself).
#   tools/ci/grep-guard.sh    — this script defines the patterns; the
#                               regex literals must remain readable.
#   tests/integration/grep-guard.test.mjs — fixtures for the script's
#                               own test suite plant the patterns
#                               into a temp dir.
WHITELIST=(
  'docs/improvement-plan.md'
  'tools/ci/grep-guard.sh'
  'tests/integration/grep-guard.test.mjs'
)

# Default scan root is the repo (script lives at tools/ci/). Tests
# override via FAULTLINE_SCAN_ROOT so they can point at a fixture
# directory without having to plant files in the real tree.
cd "${FAULTLINE_SCAN_ROOT:-$(dirname "$0")/../..}"

raw_matches=$(grep -rnEI "${INCLUDES[@]}" "${EXCLUDES[@]}" "$PATTERN" . || true)

if [[ -z "$raw_matches" ]]; then
  echo "grep-guard: clean — no banned patterns found"
  exit 0
fi

# Filter out whitelisted files. Match by suffix so the relative path
# (`./docs/...` vs `docs/...`) doesn't matter.
filtered=$(echo "$raw_matches" | awk -F: -v whitelist="${WHITELIST[*]}" '
  BEGIN {
    n = split(whitelist, w, " ")
    for (i = 1; i <= n; i++) ok[w[i]] = 1
  }
  {
    path = $1
    sub(/^\.\//, "", path)
    if (!(path in ok)) print
  }
')

if [[ -z "$filtered" ]]; then
  echo "grep-guard: clean — all matches in whitelisted files"
  exit 0
fi

echo "grep-guard: FAIL — banned reference pattern(s) found:"
echo "$filtered"
echo
echo "These patterns are blocked to keep Faultline visibly decoupled from"
echo "any specific external threat-assessment publication. See"
echo "tools/ci/grep-guard.sh for the rule and rationale."
exit 1
