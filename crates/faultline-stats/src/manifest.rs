//! Run manifests and replay determinism (Epic Q).
//!
//! A [`RunManifest`] is the smallest object that pins down "this exact
//! Faultline output came from this exact input under this exact engine."
//! It captures the scenario content hash, the Monte Carlo configuration
//! (run count, seed, mode), the engine version, and the output content
//! hash so external citers can reference a specific run by its
//! `manifest_hash` and `faultline-cli verify` can re-derive the output
//! bit-for-bit from the original scenario.
//!
//! ## Determinism contract
//!
//! Hashes are computed over the canonical JSON form of the parsed
//! `Scenario` (for inputs) and `MonteCarloSummary` (for outputs). The
//! parsed-JSON-hash is robust to TOML formatting churn (whitespace,
//! comment edits, key reordering) — only semantic changes to the
//! scenario flip `scenario_hash`. JSON serialization is deterministic
//! across native and WASM because every `Map` in the scenario tree is a
//! `BTreeMap` (sorted-key serialization) and floats round-trip through
//! ryu (deterministic on stable Rust).
//!
//! `host_platform` is recorded for diagnostics only and is **not** part
//! of the manifest hash — the determinism contract requires identical
//! output across platforms for the same seed, so a verify run on Linux
//! must accept a manifest produced on macOS.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloSummary};

/// Engine version baked in at compile time from the workspace
/// `Cargo.toml`. Surfaces in every emitted manifest so a reader can
/// match the binary that produced an output to the manifest claiming
/// it.
///
/// `CARGO_PKG_VERSION` is set by Cargo automatically at build time, so
/// no build script is required to populate it. The value flows in via
/// the `faultline-stats` crate, which inherits the workspace version.
pub const FAULTLINE_ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current manifest schema version. Bump when `RunManifest` gains a
/// breaking field; older verifiers will refuse mismatched versions
/// rather than silently misinterpreting a manifest.
pub const MANIFEST_VERSION: u32 = 1;

/// What kind of run produced the output the manifest pins. The variant
/// matters because the Monte Carlo run path, the counterfactual path,
/// and the compare path are different code paths in the CLI — verify
/// must replay the same one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManifestMode {
    /// `--single-run` — one engine pass with the recorded seed.
    SingleRun,
    /// Default Monte Carlo — `num_runs` engine passes seeded
    /// from `base_seed`.
    MonteCarlo,
    /// `--counterfactual` — baseline + overridden batch.
    Counterfactual { overrides: Vec<String> },
    /// `--compare` — baseline + alt scenario batch. The alt scenario's
    /// content hash is recorded so the verifier can refuse a stale
    /// alt file.
    Compare {
        alt_scenario_path: String,
        alt_scenario_hash: String,
    },
    /// `--sensitivity` — sweep over a single parameter.
    Sensitivity {
        param: String,
        low: f64,
        high: f64,
        steps: u32,
        runs_per_step: u32,
    },
    /// `--search` — strategy-search batch (Epic H). The search-only
    /// seed is recorded separately from `mc_config.base_seed`: search
    /// uses an independent RNG so that re-running with the same
    /// `search_seed` reproduces the trial assignments while the inner
    /// MC seed reproduces each trial's evaluation. Recorded objective
    /// labels (not the structured enum) keep the JSON stable across
    /// future objective additions.
    Search {
        method: crate::search::SearchMethod,
        trials: u32,
        search_seed: u64,
        objectives: Vec<String>,
        /// Whether the search emitted a "do nothing" baseline trial
        /// alongside its sampled trials (Epic I). Recorded so the
        /// verify path reproduces the same SearchResult shape — the
        /// output_hash includes the baseline when present, so a
        /// mismatched setting would fail replay.
        ///
        /// `#[serde(default)]` so older manifests without this field
        /// (Epic H round-one shape) replay with `false`, matching the
        /// SearchResult shape they were hashed under.
        #[serde(default)]
        compute_baseline: bool,
    },
}

/// The Monte Carlo parameters that, combined with the scenario, fix
/// the output bit-for-bit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestMcConfig {
    pub num_runs: u32,
    /// The base seed actually used for the run. CLI may resolve a
    /// random seed at runtime; the manifest always records the
    /// concrete value so verify is reproducible.
    pub base_seed: u64,
    pub collect_snapshots: bool,
}

impl ManifestMcConfig {
    /// Lift a `MonteCarloConfig` into the manifest shape. The seed
    /// must already have been resolved (no `None`) — the CLI's
    /// "random seed if unspecified" path resolves to a concrete `u64`
    /// before constructing the manifest.
    pub fn from_config(config: &MonteCarloConfig, resolved_seed: u64) -> Self {
        Self {
            num_runs: config.num_runs,
            base_seed: resolved_seed,
            collect_snapshots: config.collect_snapshots,
        }
    }

    /// Project back to a `MonteCarloConfig` for the verify path.
    pub fn to_config(&self) -> MonteCarloConfig {
        MonteCarloConfig {
            num_runs: self.num_runs,
            seed: Some(self.base_seed),
            collect_snapshots: self.collect_snapshots,
            parallel: false,
        }
    }
}

/// The full manifest emitted alongside every run output.
///
/// `manifest_hash` is computed over the canonical JSON of every other
/// field (with `manifest_hash` stripped). External citers reference a
/// run by its `manifest_hash` — bumping any input or output changes
/// the hash, so the citation is self-checking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunManifest {
    /// Schema version of this manifest format.
    pub manifest_version: u32,
    /// Faultline engine version that produced the run.
    pub engine_version: String,
    /// Path to the scenario file as invoked. Informational only —
    /// excluded from `manifest_hash` so the same logical run invoked
    /// from different working directories produces the same citation
    /// hash. The authoritative identity check is `scenario_hash`.
    pub scenario_path: String,
    /// SHA-256 hex of the canonical JSON serialization of the parsed
    /// `Scenario`.
    pub scenario_hash: String,
    /// Monte Carlo config that drove the run.
    pub mc_config: ManifestMcConfig,
    /// What kind of run produced the output.
    pub mode: ManifestMode,
    /// Host platform descriptor (`{arch}-{os}`). Diagnostic only —
    /// excluded from the manifest hash.
    pub host_platform: String,
    /// SHA-256 hex of the canonical JSON serialization of the
    /// `MonteCarloSummary` (or `ComparisonReport` for compare mode).
    pub output_hash: String,
    /// SHA-256 hex of the canonical JSON of every other field in this
    /// manifest, with `manifest_hash` and `host_platform` excluded.
    /// Stable across platforms.
    pub manifest_hash: String,
}

/// Compute SHA-256 hex of an arbitrary serializable value's canonical
/// JSON form.
///
/// Canonical means `serde_json::to_vec` of the value with all keys in
/// `BTreeMap`-sorted order — which is already true for every map in
/// the scenario / summary tree. Returns the lowercase hex digest.
pub fn canonical_json_hash<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Hash a parsed `Scenario` by its canonical JSON form.
pub fn scenario_hash(scenario: &Scenario) -> Result<String, serde_json::Error> {
    canonical_json_hash(scenario)
}

/// Hash a `MonteCarloSummary`.
pub fn summary_hash(summary: &MonteCarloSummary) -> Result<String, serde_json::Error> {
    canonical_json_hash(summary)
}

/// Hash any serializable output (used for comparison reports / single-
/// run results).
pub fn output_hash<T: Serialize>(output: &T) -> Result<String, serde_json::Error> {
    canonical_json_hash(output)
}

/// Best-effort host platform descriptor — `{arch}-{os}`. This is
/// runtime info, so it reflects the binary's host. Only used as a
/// diagnostic field; not part of any hash.
pub fn host_platform_descriptor() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// Build a [`RunManifest`] from its component pieces. Computes
/// `manifest_hash` last by hashing the canonical JSON of the manifest
/// with `manifest_hash` and `host_platform` cleared. Caller passes
/// the precomputed hashes to keep this function side-effect-free.
pub fn build_manifest(
    scenario_path: String,
    scenario_hash: String,
    mc_config: ManifestMcConfig,
    mode: ManifestMode,
    output_hash: String,
) -> Result<RunManifest, serde_json::Error> {
    // Build with placeholders, then compute the manifest hash over the
    // platform-stripped form. This ordering is deliberate: the hash
    // closes over every field that determines reproducibility, so
    // bumping the scenario or the output flips it.
    let mut manifest = RunManifest {
        manifest_version: MANIFEST_VERSION,
        engine_version: FAULTLINE_ENGINE_VERSION.to_string(),
        scenario_path,
        scenario_hash,
        mc_config,
        mode,
        host_platform: host_platform_descriptor(),
        output_hash,
        manifest_hash: String::new(),
    };
    manifest.manifest_hash = compute_manifest_hash(&manifest)?;
    Ok(manifest)
}

/// Compute the manifest's self-hash. Excludes `manifest_hash` (would
/// be self-referential), `host_platform` (varies across the determinism
/// contract's allowed boundary), and `scenario_path` (path-sensitive:
/// the same logical run invoked as `./scenarios/x.toml` vs
/// `scenarios/x.toml` must produce the same citation hash).
///
/// `pub` so the verify path in `faultline-cli` can re-derive the saved
/// manifest's self-hash before doing an expensive replay — that's the
/// only check that catches silent tampering of fields like `output_hash`
/// or `num_runs` before replay.
pub fn compute_manifest_hash(manifest: &RunManifest) -> Result<String, serde_json::Error> {
    // Strip the excluded fields by serializing a parallel struct.
    // Embedding this as #[serde(skip)] on RunManifest itself would
    // also skip the fields on the wire, defeating the purpose.
    #[derive(Serialize)]
    struct ManifestForHashing<'a> {
        manifest_version: u32,
        engine_version: &'a str,
        scenario_hash: &'a str,
        mc_config: &'a ManifestMcConfig,
        mode: &'a ManifestMode,
        output_hash: &'a str,
    }
    let view = ManifestForHashing {
        manifest_version: manifest.manifest_version,
        engine_version: &manifest.engine_version,
        scenario_hash: &manifest.scenario_hash,
        mc_config: &manifest.mc_config,
        mode: &manifest.mode,
        output_hash: &manifest.output_hash,
    };
    canonical_json_hash(&view)
}

/// Result of a verify operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    /// Replay matched the manifest bit-for-bit.
    Match,
    /// Replay failed. `reason` describes which field mismatched.
    Mismatch { reason: String },
}

/// Compare a freshly recomputed manifest against a saved one. Returns
/// [`VerifyResult::Match`] iff every replay-bound field is identical.
/// `host_platform` and `manifest_hash` are not replay-bound and are
/// not compared here (the manifest_hash is itself a function of the
/// other fields, so a per-field check is the precise comparison).
pub fn verify_manifest(saved: &RunManifest, replayed: &RunManifest) -> VerifyResult {
    if saved.manifest_version != replayed.manifest_version {
        return VerifyResult::Mismatch {
            reason: format!(
                "manifest_version: saved={} replayed={}",
                saved.manifest_version, replayed.manifest_version
            ),
        };
    }
    if saved.engine_version != replayed.engine_version {
        return VerifyResult::Mismatch {
            reason: format!(
                "engine_version: saved={} replayed={}",
                saved.engine_version, replayed.engine_version
            ),
        };
    }
    if saved.scenario_hash != replayed.scenario_hash {
        return VerifyResult::Mismatch {
            reason: format!(
                "scenario_hash: saved={} replayed={}",
                saved.scenario_hash, replayed.scenario_hash
            ),
        };
    }
    if saved.mc_config != replayed.mc_config {
        return VerifyResult::Mismatch {
            reason: format!(
                "mc_config: saved={:?} replayed={:?}",
                saved.mc_config, replayed.mc_config
            ),
        };
    }
    if saved.mode != replayed.mode {
        return VerifyResult::Mismatch {
            reason: format!("mode: saved={:?} replayed={:?}", saved.mode, replayed.mode),
        };
    }
    if saved.output_hash != replayed.output_hash {
        return VerifyResult::Mismatch {
            reason: format!(
                "output_hash: saved={} replayed={}",
                saved.output_hash, replayed.output_hash
            ),
        };
    }
    VerifyResult::Match
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_hash_is_stable() {
        // Two identical inputs hash identically.
        let a = vec![1u32, 2, 3];
        let b = vec![1u32, 2, 3];
        assert_eq!(
            canonical_json_hash(&a).expect("hash"),
            canonical_json_hash(&b).expect("hash")
        );
    }

    #[test]
    fn canonical_json_hash_differs_on_value_change() {
        let a = vec![1u32, 2, 3];
        let b = vec![1u32, 2, 4];
        assert_ne!(
            canonical_json_hash(&a).expect("hash"),
            canonical_json_hash(&b).expect("hash")
        );
    }

    #[test]
    fn build_manifest_produces_stable_self_hash() {
        let mc = ManifestMcConfig {
            num_runs: 100,
            base_seed: 42,
            collect_snapshots: false,
        };
        let m1 = build_manifest(
            "scenarios/x.toml".into(),
            "deadbeef".into(),
            mc.clone(),
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        let m2 = build_manifest(
            "scenarios/x.toml".into(),
            "deadbeef".into(),
            mc,
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        assert_eq!(m1.manifest_hash, m2.manifest_hash);
        assert!(!m1.manifest_hash.is_empty());
    }

    #[test]
    fn manifest_hash_changes_with_input() {
        let mc = ManifestMcConfig {
            num_runs: 100,
            base_seed: 42,
            collect_snapshots: false,
        };
        let m1 = build_manifest(
            "scenarios/x.toml".into(),
            "deadbeef".into(),
            mc.clone(),
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        let m2 = build_manifest(
            "scenarios/x.toml".into(),
            "different_scenario_hash".into(),
            mc,
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        assert_ne!(m1.manifest_hash, m2.manifest_hash);
    }

    #[test]
    fn manifest_hash_independent_of_scenario_path() {
        // Citation stability: invoking the same scenario via different
        // paths (`./scenarios/x.toml` vs `scenarios/x.toml` vs an
        // absolute path) must produce the same `manifest_hash`. The
        // authoritative identity check is `scenario_hash` over the
        // parsed contents.
        let mc = ManifestMcConfig {
            num_runs: 100,
            base_seed: 42,
            collect_snapshots: false,
        };
        let m1 = build_manifest(
            "./scenarios/x.toml".into(),
            "deadbeef".into(),
            mc.clone(),
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        let m2 = build_manifest(
            "/abs/path/to/scenarios/x.toml".into(),
            "deadbeef".into(),
            mc,
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        assert_eq!(m1.manifest_hash, m2.manifest_hash);
    }

    #[test]
    fn manifest_hash_independent_of_host_platform() {
        // Two manifests with identical replay-bound fields but mutated
        // host_platform must produce the same manifest_hash.
        let mc = ManifestMcConfig {
            num_runs: 100,
            base_seed: 42,
            collect_snapshots: false,
        };
        let mut m1 = build_manifest(
            "scenarios/x.toml".into(),
            "deadbeef".into(),
            mc,
            ManifestMode::MonteCarlo,
            "cafebabe".into(),
        )
        .expect("build");
        let original_hash = m1.manifest_hash.clone();
        m1.host_platform = "totally-fake-platform".to_string();
        // Recompute via the same path the constructor uses.
        let recomputed = compute_manifest_hash(&m1).expect("hash");
        assert_eq!(recomputed, original_hash);
    }

    #[test]
    fn verify_manifest_matches_identical() {
        let mc = ManifestMcConfig {
            num_runs: 10,
            base_seed: 1,
            collect_snapshots: false,
        };
        let saved = build_manifest(
            "s.toml".into(),
            "abc".into(),
            mc.clone(),
            ManifestMode::MonteCarlo,
            "out".into(),
        )
        .expect("build");
        let replayed = saved.clone();
        assert_eq!(verify_manifest(&saved, &replayed), VerifyResult::Match);
    }

    #[test]
    fn verify_manifest_flags_output_drift() {
        let mc = ManifestMcConfig {
            num_runs: 10,
            base_seed: 1,
            collect_snapshots: false,
        };
        let saved = build_manifest(
            "s.toml".into(),
            "abc".into(),
            mc.clone(),
            ManifestMode::MonteCarlo,
            "out".into(),
        )
        .expect("build");
        let replayed = build_manifest(
            "s.toml".into(),
            "abc".into(),
            mc,
            ManifestMode::MonteCarlo,
            "different_output".into(),
        )
        .expect("build");
        match verify_manifest(&saved, &replayed) {
            VerifyResult::Match => panic!("output drift should not match"),
            VerifyResult::Mismatch { reason } => {
                assert!(reason.contains("output_hash"), "{reason}")
            },
        }
    }

    #[test]
    fn manifest_mode_serialization_round_trip() {
        let modes = vec![
            ManifestMode::SingleRun,
            ManifestMode::MonteCarlo,
            ManifestMode::Counterfactual {
                overrides: vec!["a=1".into(), "b=2".into()],
            },
            ManifestMode::Compare {
                alt_scenario_path: "alt.toml".into(),
                alt_scenario_hash: "ffff".into(),
            },
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).expect("serialize");
            let back: ManifestMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn engine_version_constant_is_populated() {
        // This keeps a regression on us if anyone replaces the env! call
        // with a placeholder. Workspace version is "0.1.0" today; we
        // assert the field is non-empty rather than the exact string so
        // routine version bumps don't churn this test.
        assert!(!FAULTLINE_ENGINE_VERSION.is_empty());
        // Sanity-check it actually came from Cargo (looks like semver).
        assert!(FAULTLINE_ENGINE_VERSION.contains('.'));
    }
}
