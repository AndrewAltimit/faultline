//! End-to-end integration tests for the schema migration framework.
//!
//! The unit tests in `migration::tests` cover the helper functions
//! (extract_schema_version, apply_chain, stamp_version) in isolation.
//! These integration tests exercise the public surface — `load_scenario_str`
//! and `migrate_scenario_str` — against full scenario shapes and
//! enumerate the [`LoadError`] variants so the CLI/WASM consumers can
//! rely on their error contract.

use faultline_types::migration::{
    CURRENT_SCHEMA_VERSION, LoadError, MigrationError, load_scenario_str, migrate_scenario_str,
};

const TUTORIAL_TOML: &str = include_str!("../../../scenarios/tutorial_symmetric.toml");

// ---------------------------------------------------------------------------
// load_scenario_str — error path coverage
// ---------------------------------------------------------------------------

#[test]
fn load_rejects_schema_version_above_current() {
    // A scenario authored against a future schema (e.g. v9) must be
    // rejected with a clear NewerThanSupported error rather than
    // silently parsed as v1. This is the upgrade-Faultline signal.
    let toml = TUTORIAL_TOML.replacen("schema_version = 1", "schema_version = 9", 1);
    assert!(
        toml.contains("schema_version = 9"),
        "fixture mutation must take effect"
    );
    let err = load_scenario_str(&toml).expect_err("future-version scenario must fail");
    match err {
        LoadError::Migration(MigrationError::NewerThanSupported { found, supported }) => {
            assert_eq!(found, 9);
            assert_eq!(supported, CURRENT_SCHEMA_VERSION);
        },
        other => panic!("expected NewerThanSupported, got {other:?}"),
    }
}

#[test]
fn load_rejects_schema_version_with_wrong_type() {
    // schema_version must be an integer. Strings, floats, booleans are
    // rejected with a Structure error rather than coerced to 1 (which
    // would silently bypass migration on a malformed file).
    let toml = TUTORIAL_TOML.replacen("schema_version = 1", "schema_version = \"v1\"", 1);
    let err = load_scenario_str(&toml).expect_err("string schema_version must fail");
    match err {
        LoadError::Migration(MigrationError::Structure(msg)) => {
            assert!(
                msg.contains("schema_version"),
                "error message should name the bad field; got: {msg}"
            );
        },
        other => panic!("expected Structure error, got {other:?}"),
    }
}

#[test]
fn load_rejects_negative_schema_version() {
    // Negative integers don't fit in u32. The error path is the same
    // as for wrong-type: Structure with a clear message.
    let toml = TUTORIAL_TOML.replacen("schema_version = 1", "schema_version = -3", 1);
    let err = load_scenario_str(&toml).expect_err("negative schema_version must fail");
    assert!(matches!(
        err,
        LoadError::Migration(MigrationError::Structure(_))
    ));
}

#[test]
fn load_rejects_malformed_toml() {
    // Garbage input fails at the toml::from_str stage with a Parse
    // error. The migration code never sees the value.
    let err = load_scenario_str("this is :: not valid toml {{{").expect_err("malformed must fail");
    assert!(
        matches!(err, LoadError::Parse(_)),
        "expected Parse, got {err:?}"
    );
}

#[test]
fn load_rejects_toml_that_parses_but_fails_scenario_schema() {
    // A TOML that has [meta] but is missing required fields like map
    // or factions parses fine as toml::Value but fails Scenario
    // deserialization. The error must be Deserialize, not Parse —
    // they're distinct conditions and consumers may want to react
    // differently (e.g. show a "TOML syntax error" vs "missing
    // required field" message).
    let toml = "[meta]\nschema_version = 1\nname = \"x\"\n";
    let err = load_scenario_str(toml).expect_err("incomplete scenario must fail");
    assert!(
        matches!(err, LoadError::Deserialize(_)),
        "expected Deserialize, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// load_scenario_str — happy path on the full bundled scenario
// ---------------------------------------------------------------------------

#[test]
fn load_real_bundled_scenario_succeeds() {
    // End-to-end: the production tutorial scenario, with its real
    // [factions], [map], [simulation] etc., must round-trip through
    // load_scenario_str cleanly. Catches any case where adding the
    // schema_version field broke deserialization of the full shape.
    let loaded = load_scenario_str(TUTORIAL_TOML).expect("bundled scenario must load");
    assert_eq!(loaded.source_version, 1);
    assert!(!loaded.migrated);
    assert_eq!(loaded.scenario.meta.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
        loaded.scenario.meta.name, "Tutorial — Symmetric Conflict",
        "field deserialization must still work after the schema_version addition"
    );
}

#[test]
fn load_full_scenario_without_schema_version_field() {
    // Strip the schema_version line from a real scenario and confirm
    // it still loads via the default-to-1 path. This pins the
    // backwards-compat hatch for fixtures authored before the field
    // existed.
    let stripped = TUTORIAL_TOML.replacen("\nschema_version = 1\n", "\n", 1);
    assert!(!stripped.contains("schema_version"));
    let loaded =
        load_scenario_str(&stripped).expect("legacy bundled scenario must load via default");
    assert_eq!(loaded.source_version, 1);
    assert!(!loaded.migrated);
    assert_eq!(loaded.scenario.meta.schema_version, CURRENT_SCHEMA_VERSION);
}

// ---------------------------------------------------------------------------
// migrate_scenario_str — round-trip on the full bundled scenario
// ---------------------------------------------------------------------------

#[test]
fn migrate_full_scenario_roundtrips() {
    // Migrate the full tutorial, then load the migrated form and
    // confirm it deserializes cleanly. This is the contract the
    // verify-migration CI script enforces at the shell level — having
    // it as a Rust test gives faster local feedback when developing
    // future migration steps.
    let migrated = migrate_scenario_str(TUTORIAL_TOML).expect("migrate full scenario");
    let reloaded = load_scenario_str(&migrated).expect("migrated form must reload");
    assert_eq!(reloaded.source_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
        reloaded.scenario.meta.schema_version,
        CURRENT_SCHEMA_VERSION
    );
    // The scenario name is part of the meta block; if the migration
    // dropped or corrupted [meta], this would change.
    assert_eq!(reloaded.scenario.meta.name, "Tutorial — Symmetric Conflict");
}

#[test]
fn migrate_strips_field_then_restores_default() {
    // A scenario without the schema_version field should round-trip
    // through migrate_scenario_str with the field stamped in. This is
    // the upgrade path for legacy fixtures: load → migrate → re-emit
    // produces the canonical form an analyst can commit.
    let stripped = TUTORIAL_TOML.replacen("\nschema_version = 1\n", "\n", 1);
    let migrated = migrate_scenario_str(&stripped).expect("migrate stripped form");
    assert!(
        migrated.contains("schema_version"),
        "migrated TOML must explicitly carry the schema_version field"
    );
    let reloaded = load_scenario_str(&migrated).expect("re-load after migrate");
    assert_eq!(
        reloaded.scenario.meta.schema_version,
        CURRENT_SCHEMA_VERSION
    );
}

// ---------------------------------------------------------------------------
// Determinism: migration is a pure function of input
// ---------------------------------------------------------------------------

#[test]
fn migrate_is_deterministic() {
    // The migration framework must be a pure function of (input,
    // CURRENT_SCHEMA_VERSION, MIGRATIONS). Running migrate twice on
    // the same input must yield byte-identical TOML. This is the
    // foundation that lets Epic Q manifest hashes mean anything —
    // a non-deterministic migrator would make scenario_hash unstable
    // even at fixed source.
    let a = migrate_scenario_str(TUTORIAL_TOML).expect("first migrate");
    let b = migrate_scenario_str(TUTORIAL_TOML).expect("second migrate");
    assert_eq!(a, b, "migrate must produce identical output across runs");
}
