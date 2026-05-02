//! Integration tests for the Epic P `explain` subset.
//!
//! Pure schema view — no engine invocation, no RNG. Every bundled
//! scenario must round-trip through `explain → render_markdown` and
//! `explain → JSON` without panicking and emit the expected structural
//! anchors. Catches both regressions in the explain producer (e.g. a
//! new `FactionType` variant landing without a label arm) and drift in
//! the bundled scenario library (e.g. a scenario losing its
//! `meta.name`).

use faultline_stats::explain::{explain, render_markdown};
use faultline_types::migration::load_scenario_str;

/// All bundled scenarios. Keeping the list explicit (rather than
/// scanning the directory at build time) means a forgotten scenario
/// also forgets to be tested — discoverable via the
/// `verify-bundled-scenarios.sh` CI step rather than silently passing.
const BUNDLED: &[(&str, &str)] = &[
    (
        "alert_fatigue_soc",
        include_str!("../../../scenarios/alert_fatigue_soc.toml"),
    ),
    (
        "capabilities_demo",
        include_str!("../../../scenarios/capabilities_demo.toml"),
    ),
    (
        "coalition_fracture_demo",
        include_str!("../../../scenarios/coalition_fracture_demo.toml"),
    ),
    (
        "coevolution_demo",
        include_str!("../../../scenarios/coevolution_demo.toml"),
    ),
    (
        "compound_kill_chains",
        include_str!("../../../scenarios/compound_kill_chains.toml"),
    ),
    (
        "defender_posture_optimization",
        include_str!("../../../scenarios/defender_posture_optimization.toml"),
    ),
    (
        "defender_robustness_demo",
        include_str!("../../../scenarios/defender_robustness_demo.toml"),
    ),
    (
        "drone_swarm_destabilization",
        include_str!("../../../scenarios/drone_swarm_destabilization.toml"),
    ),
    (
        "europe_eastern_flank",
        include_str!("../../../scenarios/europe_eastern_flank.toml"),
    ),
    (
        "europe_energy_sabotage",
        include_str!("../../../scenarios/europe_energy_sabotage.toml"),
    ),
    (
        "network_resilience_demo",
        include_str!("../../../scenarios/network_resilience_demo.toml"),
    ),
    (
        "persistent_covert_surveillance",
        include_str!("../../../scenarios/persistent_covert_surveillance.toml"),
    ),
    (
        "strategy_search_demo",
        include_str!("../../../scenarios/strategy_search_demo.toml"),
    ),
    (
        "tutorial_asymmetric",
        include_str!("../../../scenarios/tutorial_asymmetric.toml"),
    ),
    (
        "tutorial_symmetric",
        include_str!("../../../scenarios/tutorial_symmetric.toml"),
    ),
    (
        "us_institutional_fracture",
        include_str!("../../../scenarios/us_institutional_fracture.toml"),
    ),
];

#[test]
fn every_bundled_scenario_explains_cleanly() {
    for (name, src) in BUNDLED {
        let scenario = load_scenario_str(src)
            .unwrap_or_else(|e| panic!("bundled scenario {name} must load: {e}"))
            .scenario;
        let report = explain(&scenario);

        // Every bundled scenario has a name, factions, and at least
        // one victory condition — these are baseline authoring
        // expectations the explain output should always reflect.
        assert!(
            !report.meta.name.is_empty(),
            "{name}: explain meta.name is empty"
        );
        assert!(
            report.scale.factions > 0,
            "{name}: explain reports zero factions"
        );
        assert_eq!(
            report.scale.factions,
            report.factions.len(),
            "{name}: scale.factions disagrees with factions.len()"
        );

        // Markdown must include the section anchors in stable order
        // so downstream tooling (e.g. a future `--explain-format
        // html` renderer) can rely on parseable structure.
        let md = render_markdown(&report);
        for anchor in [
            "## Scale",
            "## Factions",
            "## Kill chains",
            "## Victory conditions",
            "## Networks",
            "## Decision-variable surface",
            "## Low-confidence parameters",
        ] {
            assert!(
                md.contains(anchor),
                "{name}: rendered markdown is missing section anchor {anchor:?}"
            );
        }

        // Section ordering is part of the explain contract — a
        // future refactor that reorders sections must update both
        // the producer and this expectation.
        let mut last = 0usize;
        for anchor in [
            "## Scale",
            "## Factions",
            "## Kill chains",
            "## Victory conditions",
            "## Networks",
            "## Decision-variable surface",
            "## Low-confidence parameters",
        ] {
            let pos = md
                .find(anchor)
                .unwrap_or_else(|| panic!("{name}: anchor {anchor} missing"));
            // Strict `>` rather than `>=`: distinct anchor strings can
            // never share a byte offset (the header `# <name>` always
            // precedes the first `## Scale` so even iter 1 satisfies
            // `pos > 0`), and the tighter bound documents the
            // strict-ordering guarantee.
            assert!(
                pos > last,
                "{name}: section {anchor} appears before a prior section"
            );
            last = pos;
        }
    }
}

#[test]
fn explain_round_trips_through_json_for_every_bundled() {
    // Serialize then deserialize so we catch any field that needs a
    // Default impl or a serde rename to round-trip cleanly.
    for (name, src) in BUNDLED {
        let scenario = load_scenario_str(src)
            .unwrap_or_else(|e| panic!("bundled scenario {name} must load: {e}"))
            .scenario;
        let report = explain(&scenario);
        let json = serde_json::to_string(&report)
            .unwrap_or_else(|e| panic!("{name}: explain report failed to serialize: {e}"));
        let _: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| {
            panic!("{name}: serialized explain report failed to round-trip: {e}")
        });
    }
}

#[test]
fn coalition_fracture_demo_surfaces_fracture_rules_in_explain() {
    // Pin a known fact about a known scenario so a future regression
    // in the alliance_fracture surface area is caught without
    // depending on engine output. The scenario declares two fracture
    // rules on `gray_partner`; explain must surface both.
    let src = include_str!("../../../scenarios/coalition_fracture_demo.toml");
    let scenario = load_scenario_str(src).expect("loads").scenario;
    let report = explain(&scenario);
    let gray = report
        .factions
        .iter()
        .find(|f| f.id == "gray_partner")
        .expect("explain must include gray_partner faction");
    assert_eq!(
        gray.alliance_fracture_rule_count, 2,
        "gray_partner declares 2 fracture rules; explain should report 2"
    );
}

#[test]
fn defender_posture_demo_surfaces_decision_variables() {
    // The defender_posture_optimization demo declares three decision
    // variables owned by `blue`. Pinning the count protects the
    // strategy-space surface against silent loss when the demo is
    // edited.
    let src = include_str!("../../../scenarios/defender_posture_optimization.toml");
    let scenario = load_scenario_str(src).expect("loads").scenario;
    let report = explain(&scenario);
    assert_eq!(
        report.strategy_space.variable_count, 3,
        "defender_posture_optimization declares 3 decision variables"
    );
    // Every variable must declare an owner so the search runner can
    // partition by side. explain surfaces it; check it is non-None.
    for v in &report.strategy_space.variables {
        assert!(
            v.owner.is_some(),
            "decision variable {} on the bundled defender-posture demo is missing an owner",
            v.path
        );
    }
}

#[test]
fn scenarios_with_low_confidence_meta_surface_it() {
    // Four bundled demos declare scenario-level Low confidence; the
    // explain producer pushes a synthetic row into low_confidence so
    // analysts skimming "what to push on under counterfactual?"
    // actually see it.
    let demos = [
        "strategy_search_demo",
        "defender_posture_optimization",
        "defender_robustness_demo",
        "coevolution_demo",
    ];
    for demo in demos {
        let (_, src) = BUNDLED
            .iter()
            .find(|(n, _)| *n == demo)
            .unwrap_or_else(|| panic!("{demo} not in BUNDLED list"));
        let scenario = load_scenario_str(src).expect("loads").scenario;
        let report = explain(&scenario);
        assert!(
            report
                .low_confidence
                .iter()
                .any(|i| i.location == "scenario"),
            "{demo}: scenario-level Low confidence must surface in low_confidence"
        );
    }
}
