/**
 * Tests for tools/ci/grep-guard.sh — the CI gate that blocks
 * re-introduction of references coupling Faultline to a specific
 * external threat-assessment publication series.
 *
 * The guard is a bash script, not a JS module, but exit-code behavior
 * is the contract worth pinning. Each test plants a fixture directory,
 * points the script at it via the `FAULTLINE_SCAN_ROOT` env var, and
 * asserts the script's exit code (0 = clean, 1 = violation).
 *
 * Run with:
 *   node --test tests/integration/grep-guard.test.mjs
 */

import { test, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, '..', '..');
const scriptPath = join(repoRoot, 'tools', 'ci', 'grep-guard.sh');

let fixture;

beforeEach(() => {
  fixture = mkdtempSync(join(tmpdir(), 'faultline-grep-guard-'));
});

afterEach(() => {
  if (fixture) rmSync(fixture, { recursive: true, force: true });
});

/**
 * Run the guard against the current fixture and return
 * `{ status, stdout }`. Status 0 = clean, 1 = violation. Doesn't throw
 * on non-zero exit so tests can assert against either outcome.
 */
function runGuard() {
  try {
    const stdout = execFileSync(scriptPath, [], {
      env: { ...process.env, FAULTLINE_SCAN_ROOT: fixture },
      encoding: 'utf8',
    });
    return { status: 0, stdout };
  } catch (e) {
    return { status: e.status ?? -1, stdout: e.stdout?.toString() || '' };
  }
}

function plant(relPath, content) {
  const full = join(fixture, relPath);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, content);
}

// ---------------------------------------------------------------------------
// Clean-tree behavior
// ---------------------------------------------------------------------------

test('grep-guard: empty tree exits 0', () => {
  const { status } = runGuard();
  assert.equal(status, 0);
});

test('grep-guard: tree with only neutral content exits 0', () => {
  plant('src/main.rs', 'fn main() { println!("hello"); }');
  plant('docs/README.md', '# Faultline\n\nA conflict simulator.');
  const { status } = runGuard();
  assert.equal(status, 0);
});

// ---------------------------------------------------------------------------
// Violation detection — each banned pattern
// ---------------------------------------------------------------------------

test('grep-guard: catches \\bETRA\\b in code comments', () => {
  // The bare acronym is the most common reintroduction pattern (a
  // contributor pasting in legacy notes). The guard MUST flag it.
  plant('src/lib.rs', '// derived from the ETRA framework');
  const { status, stdout } = runGuard();
  assert.equal(status, 1);
  assert.match(stdout, /banned reference pattern\(s\) found/);
  assert.match(stdout, /src\/lib\.rs/);
});

test('grep-guard: catches etra_ref schema field', () => {
  // The previous JS schema field name. If it returns to the codebase,
  // it would re-enable structured coupling at the data-shape layer
  // even if the values were generic.
  plant('site/js/app/cards.js', "{ etra_ref: 'something' }");
  const { status } = runGuard();
  assert.equal(status, 1);
});

test('grep-guard: catches ETRA-YYYY- document identifiers', () => {
  // The fingerprint pattern — these uniquely identify specific
  // external publications.
  plant('docs/notes.md', 'Reference: ETRA-2026-WMD-001 covers...');
  const { status, stdout } = runGuard();
  assert.equal(status, 1);
  assert.match(stdout, /docs\/notes\.md/);
});

// ---------------------------------------------------------------------------
// False-positive avoidance
// ---------------------------------------------------------------------------

test('grep-guard: does not match "ETRA" inside other words', () => {
  // The \b word boundary in the regex must prevent accidental matches
  // inside legitimate words like "penetration", "getrandom", etc.
  // Without the boundary, half the engine would trigger.
  //
  // Fixtures must contain the *uppercase* substring "ETRA" inside
  // another word — otherwise the test would pass for the wrong reason
  // (case mismatch alone, since the regex is case-sensitive). The
  // identifiers below embed ETRA between word characters, which is
  // exactly what \b should reject.
  plant(
    'src/lib.rs',
    `
    let SPETRAL_VALUE = 1.0;       // P-E-T-R-A-L: \\b should reject (P|E and A|L are word/word).
    let XETRAY_FIELD = 2.0;        // X-E-T-R-A-Y: same.
    const ETRACTION_RATE = 0.42;   // E-T-R-A-C: \\b should reject (A|C is word/word).
    `,
  );
  const { status } = runGuard();
  assert.equal(status, 0);
});

// ---------------------------------------------------------------------------
// File-type filtering
// ---------------------------------------------------------------------------

test('grep-guard: scans .toml scenario files', () => {
  plant('scenarios/test.toml', '# Header: based on the ETRA framework\n');
  const { status } = runGuard();
  assert.equal(status, 1);
});

test('grep-guard: scans .css comments', () => {
  // The original audit caught an ETRA reference in app.css. Coverage
  // here pins that the scan continues to include CSS.
  plant('site/css/test.css', '/* ETRA-derived panel styles */');
  const { status } = runGuard();
  assert.equal(status, 1);
});

test('grep-guard: ignores file extensions outside the include list', () => {
  // Binary files, vendored libraries, lockfiles, and the like get
  // skipped by extension. Putting a banned pattern in a `.lock` file
  // shouldn't fail the build.
  //
  // Note: `.lock` is excluded *by omission* — the guard scans only the
  // extensions in `INCLUDES` (.rs, .toml, .md, .html, .css, .js, .mjs,
  // .yml, .yaml, .sh). There is no explicit `--exclude=*.lock`. Adding
  // `.lock` to the include list would break this test.
  plant('Cargo.lock', '# version = "ETRA"');
  const { status } = runGuard();
  assert.equal(status, 0);
});

test('grep-guard: ignores excluded directories', () => {
  // target/, pkg/, node_modules/, and .git/ are excluded — vendored
  // or generated content can carry the substring without the human
  // author having introduced it.
  plant('target/debug/build/note.md', 'ETRA artifact note');
  plant('site/pkg/generated.js', 'const ETRA = true;');
  plant('node_modules/dep/lib.js', '// ETRA');
  const { status } = runGuard();
  assert.equal(status, 0);
});

test('grep-guard: does not double-scan symlinked directories', () => {
  // The real repo has `site/scenarios -> ../scenarios`. A naive
  // `grep -r` on BSD/macOS follows that symlink and reports every
  // banned match in scenarios/ twice (once via the canonical path,
  // once via the symlinked one). The find-based enumeration in the
  // guard is meant to treat symlinks as leaves so the underlying
  // directory is scanned exactly once.
  //
  // This test plants a violation in scenarios/, points a sibling
  // symlink at it (mirroring the real layout), and asserts the
  // violation appears in stdout exactly one time. If the guard
  // regresses to a recursive-grep approach that follows symlinks,
  // the match count would be 2 and this test would fail.
  plant('scenarios/test.toml', '# ETRA-grade scenario\n');
  mkdirSync(join(fixture, 'site'), { recursive: true });
  symlinkSync('../scenarios', join(fixture, 'site', 'scenarios'));
  const { status, stdout } = runGuard();
  assert.equal(status, 1);
  const matches = stdout.match(/scenarios\/test\.toml/g) || [];
  assert.equal(
    matches.length,
    1,
    `expected exactly one match line, got ${matches.length}: ${stdout}`,
  );
});

// ---------------------------------------------------------------------------
// Whitelist behavior
// ---------------------------------------------------------------------------

test('grep-guard: whitelisted improvement-plan.md does not trigger', () => {
  // The Round-Two roadmap describes the cleanup itself and quotes the
  // patterns it bans. Whitelisting prevents the doc from being a
  // permanent build-failure source.
  plant(
    'docs/improvement-plan.md',
    'Replace "ETRA-style" / "ETRA-grade" / etra_ref references.',
  );
  const { status } = runGuard();
  assert.equal(status, 0);
});

test('grep-guard: whitelist match is path-exact, not basename', () => {
  // A file at docs/copy/improvement-plan.md should NOT be whitelisted
  // — only the exact path docs/improvement-plan.md is. Otherwise
  // someone could rename a file with banned content into a different
  // directory and bypass the guard by accident.
  plant('docs/copy/improvement-plan.md', 'mention of ETRA here');
  const { status, stdout } = runGuard();
  assert.equal(status, 1);
  assert.match(stdout, /docs\/copy\/improvement-plan\.md/);
});

test('grep-guard: whitelisted file co-existing with violation still fails', () => {
  // The whitelist only suppresses the specific file. A real violation
  // elsewhere in the same run must still cause exit 1.
  plant('docs/improvement-plan.md', '// describes the ban: ETRA');
  plant('src/lib.rs', '// real violation: ETRA');
  const { status, stdout } = runGuard();
  assert.equal(status, 1);
  assert.match(stdout, /src\/lib\.rs/);
});
