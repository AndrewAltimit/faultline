//! Scenario schema versioning and migration.
//!
//! Every scenario carries a `meta.schema_version` (defaulting to 1
//! when absent for backwards compatibility). Loading goes through
//! [`load_scenario_str`], which:
//!   1. Parses the TOML to a `toml::Value`,
//!   2. Reads `meta.schema_version`,
//!   3. Runs migration steps in order from that version up to
//!      [`CURRENT_SCHEMA_VERSION`],
//!   4. Stamps the new version into `meta.schema_version`,
//!   5. Deserializes the migrated value into a [`Scenario`].
//!
//! Adding a new schema version (vN → vN+1):
//!   1. Bump [`CURRENT_SCHEMA_VERSION`] to N+1.
//!   2. Append a [`MigrationStep`] to [`MIGRATIONS`] whose `apply`
//!      function rewrites the `toml::Value` from shape vN to shape
//!      vN+1.
//!   3. Update bundled scenarios (`scenarios/*.toml`) to the new
//!      `schema_version` either by hand or by running
//!      `faultline scenarios/foo.toml --migrate --in-place`.
//!   4. Add a fixture under `crates/faultline-types/tests/fixtures/`
//!      that pins the old shape, so the `verify-migration` CI check
//!      keeps the migrator honest.
//!
//! Determinism contract: this module operates on `toml::Value` only;
//! it never touches RNGs or simulation state. Its output for a given
//! input is purely a function of the registered migration steps.

use serde::Deserialize;
use thiserror::Error;

use crate::scenario::Scenario;

/// The schema version this build of Faultline emits and understands.
///
/// A scenario authored under any version `<= CURRENT_SCHEMA_VERSION`
/// loads via the migrator. A scenario at a higher version is rejected
/// with [`MigrationError::NewerThanSupported`] — the user is asked to
/// upgrade Faultline rather than risk a silent partial parse.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// A migration step rewrites a TOML value from `from` to `from + 1`.
///
/// Steps mutate in place; the [`migrate`] driver iterates from the
/// source version up to [`CURRENT_SCHEMA_VERSION`].
struct MigrationStep {
    from: u32,
    apply: fn(&mut toml::Value) -> Result<(), MigrationError>,
}

/// Registered migration steps in ascending `from` order.
///
/// Initially empty: v1 is the first version, so there is nothing to
/// migrate yet. New steps append here when [`CURRENT_SCHEMA_VERSION`]
/// bumps.
const MIGRATIONS: &[MigrationStep] = &[];

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error(
        "scenario uses schema version {found}, but this build supports up to version {supported}; upgrade Faultline"
    )]
    NewerThanSupported { found: u32, supported: u32 },

    #[error(
        "no migration registered to advance from version {from}; this is a build bug — every version below CURRENT_SCHEMA_VERSION must have a step"
    )]
    MissingStep { from: u32 },

    #[error("invalid scenario TOML structure: {0}")]
    Structure(String),

    #[error("failed to serialize migrated scenario back to TOML: {0}")]
    Serialize(String),
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to parse scenario TOML: {0}")]
    Parse(toml::de::Error),

    #[error("scenario migration failed: {0}")]
    Migration(#[from] MigrationError),

    #[error("failed to deserialize migrated scenario: {0}")]
    Deserialize(toml::de::Error),
}

/// Result of [`load_scenario_str`].
///
/// `source_version` records the version the on-disk TOML was authored
/// against, distinct from the in-memory [`Scenario`] which is always
/// at [`CURRENT_SCHEMA_VERSION`] post-migration. The CLI and the
/// browser frontend surface a "scenario was migrated" warning when
/// `migrated == true` so an analyst notices stale fixtures rather
/// than discovering them later as a hash drift.
pub struct LoadedScenario {
    pub scenario: Scenario,
    pub source_version: u32,
    pub migrated: bool,
}

/// Read `meta.schema_version` from a parsed TOML value.
///
/// Defaults to 1 when the field is absent — that's the implicit
/// version for scenarios authored before this field existed. Errors
/// only when the field exists but is not a non-negative integer that
/// fits in `u32`; silent coercion would let a malformed scenario
/// bypass migration.
pub fn extract_schema_version(value: &toml::Value) -> Result<u32, MigrationError> {
    let Some(meta) = value.get("meta") else {
        return Ok(1);
    };
    let Some(field) = meta.get("schema_version") else {
        return Ok(1);
    };
    let int = field.as_integer().ok_or_else(|| {
        MigrationError::Structure(format!(
            "meta.schema_version must be an integer, got: {field}"
        ))
    })?;
    u32::try_from(int).map_err(|_| {
        MigrationError::Structure(format!(
            "meta.schema_version must fit in u32 (>= 0, <= {}), got: {int}",
            u32::MAX
        ))
    })
}

/// Run all registered migration steps in order to advance `value`
/// from `from` up to [`CURRENT_SCHEMA_VERSION`], then stamp the
/// current version into `meta.schema_version` so subsequent
/// serialization reflects it.
pub fn migrate(value: toml::Value, from: u32) -> Result<toml::Value, MigrationError> {
    apply_chain(value, from, CURRENT_SCHEMA_VERSION, MIGRATIONS)
}

/// Generic chain driver. Lifted out of [`migrate`] so tests can
/// inject a synthetic migration sequence and exercise the chain logic
/// without waiting for v2 to actually ship — `MIGRATIONS` is empty at
/// v1, so production-only tests can't prove the loop runs.
fn apply_chain(
    mut value: toml::Value,
    from: u32,
    target: u32,
    steps: &[MigrationStep],
) -> Result<toml::Value, MigrationError> {
    if from > target {
        return Err(MigrationError::NewerThanSupported {
            found: from,
            supported: target,
        });
    }
    let mut current = from;
    while current < target {
        let step = steps
            .iter()
            .find(|s| s.from == current)
            .ok_or(MigrationError::MissingStep { from: current })?;
        (step.apply)(&mut value)?;
        current += 1;
    }
    stamp_version(&mut value, target)?;
    Ok(value)
}

fn stamp_version(value: &mut toml::Value, version: u32) -> Result<(), MigrationError> {
    let table = value
        .as_table_mut()
        .ok_or_else(|| MigrationError::Structure("scenario root must be a TOML table".into()))?;
    let meta = table
        .entry("meta".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let meta_table = meta
        .as_table_mut()
        .ok_or_else(|| MigrationError::Structure("[meta] must be a table".into()))?;
    meta_table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(i64::from(version)),
    );
    Ok(())
}

/// Parse a TOML string, run schema migrations forward to current,
/// and deserialize into a [`Scenario`].
///
/// Both the CLI and the WASM frontend route their scenario loading
/// through this function so the migration policy stays consistent.
pub fn load_scenario_str(toml_str: &str) -> Result<LoadedScenario, LoadError> {
    let value: toml::Value = toml::from_str(toml_str).map_err(LoadError::Parse)?;
    let source_version = extract_schema_version(&value)?;
    let migrated_value = migrate(value, source_version)?;
    let scenario = Scenario::deserialize(migrated_value).map_err(LoadError::Deserialize)?;
    Ok(LoadedScenario {
        scenario,
        source_version,
        migrated: source_version != CURRENT_SCHEMA_VERSION,
    })
}

/// Run migrations on a TOML string and re-emit the upgraded TOML.
///
/// Powers `faultline-cli --migrate`: an analyst with a stale fixture
/// can persist the upgraded form on disk without running a sim.
/// Returns the migrated TOML as a string.
pub fn migrate_scenario_str(toml_str: &str) -> Result<String, LoadError> {
    let value: toml::Value = toml::from_str(toml_str).map_err(LoadError::Parse)?;
    let source_version = extract_schema_version(&value)?;
    let migrated = migrate(value, source_version)?;
    toml::to_string_pretty(&migrated)
        .map_err(|e| LoadError::Migration(MigrationError::Serialize(e.to_string())))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal scenario TOML used to exercise the migrator without
    /// requiring the full `Scenario` shape — these tests target the
    /// extract/migrate/stamp helpers, not deserialization.
    const META_ONLY_V1: &str = "[meta]\nname = \"x\"\n";

    #[test]
    fn extract_defaults_to_one_when_field_absent() {
        let v: toml::Value = toml::from_str(META_ONLY_V1).expect("parse");
        assert_eq!(
            extract_schema_version(&v).expect("extract"),
            1,
            "absent schema_version must default to 1"
        );
    }

    #[test]
    fn extract_reads_explicit_version() {
        let v: toml::Value =
            toml::from_str("[meta]\nschema_version = 1\nname = \"x\"\n").expect("parse");
        assert_eq!(extract_schema_version(&v).expect("extract"), 1);
    }

    #[test]
    fn extract_rejects_non_integer_field() {
        let v: toml::Value =
            toml::from_str("[meta]\nschema_version = \"oops\"\nname = \"x\"\n").expect("parse");
        let err = extract_schema_version(&v).expect_err("must reject string");
        assert!(matches!(err, MigrationError::Structure(_)));
    }

    #[test]
    fn extract_rejects_negative_integer() {
        let v: toml::Value =
            toml::from_str("[meta]\nschema_version = -1\nname = \"x\"\n").expect("parse");
        let err = extract_schema_version(&v).expect_err("must reject negative");
        assert!(matches!(err, MigrationError::Structure(_)));
    }

    #[test]
    fn migrate_rejects_newer_than_supported() {
        let v: toml::Value = toml::from_str(META_ONLY_V1).expect("parse");
        let err =
            migrate(v, CURRENT_SCHEMA_VERSION + 1).expect_err("newer-than-supported must fail");
        assert!(matches!(err, MigrationError::NewerThanSupported { .. }));
    }

    #[test]
    fn migrate_at_current_version_is_noop_modulo_stamp() {
        let v: toml::Value = toml::from_str(META_ONLY_V1).expect("parse");
        let migrated = migrate(v, CURRENT_SCHEMA_VERSION).expect("noop migrate");
        // The stamp guarantees the field is present after migration
        // even if it wasn't before — that's how downstream
        // deserialization sees a consistent shape.
        let stamped = extract_schema_version(&migrated).expect("extract after migrate");
        assert_eq!(stamped, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn migrate_stamps_into_meta_when_meta_missing() {
        // A scenario without a `[meta]` table at all (degenerate input
        // — the engine would reject it later) should still get a
        // synthesized meta with the schema_version field. That keeps
        // the stamping invariant simple: post-migration, the field is
        // always present, regardless of the input shape.
        let v: toml::Value = toml::Value::Table(toml::Table::new());
        let migrated = migrate(v, CURRENT_SCHEMA_VERSION).expect("migrate empty");
        assert_eq!(
            extract_schema_version(&migrated).expect("extract"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrate_scenario_str_roundtrips_at_current_version() {
        // A scenario authored at the current version should round-trip
        // through migrate_scenario_str without semantic change. We
        // don't compare bytes (TOML serialization is not stable across
        // toml-rs versions) — we re-parse and check the schema_version.
        let toml_str = "[meta]\nschema_version = 1\nname = \"x\"\n";
        let migrated = migrate_scenario_str(toml_str).expect("migrate");
        let v: toml::Value = toml::from_str(&migrated).expect("re-parse");
        assert_eq!(extract_schema_version(&v).expect("extract"), 1);
    }

    /// Test fixture: a synthetic v0→v1 migration that adds a sentinel
    /// field. Used by [`apply_chain_runs_synthetic_steps_in_order`]
    /// to prove the chain driver actually walks the steps.
    fn fake_v0_to_v1(value: &mut toml::Value) -> Result<(), MigrationError> {
        let table = value.as_table_mut().expect("root table");
        let meta = table
            .entry("meta".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let meta_table = meta.as_table_mut().expect("meta table");
        meta_table.insert("__test_v0_to_v1".into(), toml::Value::Boolean(true));
        Ok(())
    }

    fn fake_v1_to_v2(value: &mut toml::Value) -> Result<(), MigrationError> {
        let table = value.as_table_mut().expect("root table");
        let meta = table
            .entry("meta".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let meta_table = meta.as_table_mut().expect("meta table");
        meta_table.insert("__test_v1_to_v2".into(), toml::Value::Boolean(true));
        Ok(())
    }

    #[test]
    fn apply_chain_runs_synthetic_steps_in_order() {
        // The production MIGRATIONS slice is empty at v1, so it can't
        // prove the chain driver actually runs steps. Inject a fake
        // chain v0 → v1 → v2 and confirm each step's marker shows up
        // in the migrated value, in the right order.
        let steps = &[
            MigrationStep {
                from: 0,
                apply: fake_v0_to_v1,
            },
            MigrationStep {
                from: 1,
                apply: fake_v1_to_v2,
            },
        ];
        let v: toml::Value = toml::from_str("[meta]\nname = \"x\"\n").expect("parse");
        let migrated = apply_chain(v, 0, 2, steps).expect("chain");

        let meta = migrated
            .as_table()
            .and_then(|t| t.get("meta"))
            .and_then(|m| m.as_table())
            .expect("meta table after migration");
        assert_eq!(
            meta.get("__test_v0_to_v1"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(
            meta.get("__test_v1_to_v2"),
            Some(&toml::Value::Boolean(true))
        );
        assert_eq!(meta.get("schema_version"), Some(&toml::Value::Integer(2)));
    }

    #[test]
    fn apply_chain_errors_on_missing_step() {
        // A chain that's missing v1→v2 should fail loudly when asked
        // to migrate from v0 to v2. Silent skipping would let a
        // half-migrated scenario reach the engine.
        let steps = &[MigrationStep {
            from: 0,
            apply: fake_v0_to_v1,
        }];
        let v: toml::Value = toml::from_str("[meta]\nname = \"x\"\n").expect("parse");
        let err = apply_chain(v, 0, 2, steps).expect_err("missing-step must error");
        assert!(matches!(err, MigrationError::MissingStep { from: 1 }));
    }

    #[test]
    fn apply_chain_propagates_step_failures() {
        // If a step itself returns an error, the driver must propagate
        // it rather than swallow and continue. A failed migration is
        // not a "best-effort" operation.
        fn always_fail(_: &mut toml::Value) -> Result<(), MigrationError> {
            Err(MigrationError::Structure("simulated failure".into()))
        }
        let steps = &[MigrationStep {
            from: 0,
            apply: always_fail,
        }];
        let v: toml::Value = toml::from_str("[meta]\nname = \"x\"\n").expect("parse");
        let err = apply_chain(v, 0, 1, steps).expect_err("step error must propagate");
        assert!(matches!(err, MigrationError::Structure(_)));
    }

    #[test]
    fn load_scenario_str_accepts_legacy_scenario_without_field() {
        // A bundled scenario stripped of its `schema_version = 1`
        // line must still load — the default-to-1 path is the
        // backwards-compat hatch for fixtures authored before the
        // field existed. We use the real tutorial scenario so the
        // full Scenario shape is exercised, not a synthetic stub.
        const TUTORIAL_TOML: &str = include_str!("../../../scenarios/tutorial_symmetric.toml");
        let stripped = TUTORIAL_TOML.replacen("\nschema_version = 1\n", "\n", 1);
        assert!(
            !stripped.contains("schema_version"),
            "test fixture must actually have the field stripped"
        );
        let loaded = load_scenario_str(&stripped).expect("legacy scenario must load");
        assert_eq!(loaded.source_version, 1, "absent field reads as v1");
        assert!(
            !loaded.migrated,
            "implicit v1 source equals current version, so no migration occurred"
        );
        assert_eq!(loaded.scenario.meta.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn load_scenario_str_accepts_explicit_current_version() {
        // The bundled scenario as-shipped (with `schema_version = 1`)
        // must load identically to the legacy form above. Together
        // these two tests pin the equivalence between explicit-v1
        // and implicit-v1 — that equivalence is what makes the
        // `#[serde(default = "...")]` choice safe.
        const TUTORIAL_TOML: &str = include_str!("../../../scenarios/tutorial_symmetric.toml");
        let loaded = load_scenario_str(TUTORIAL_TOML).expect("bundled scenario must load");
        assert_eq!(loaded.source_version, 1);
        assert!(!loaded.migrated);
        assert_eq!(loaded.scenario.meta.schema_version, CURRENT_SCHEMA_VERSION);
    }
}
