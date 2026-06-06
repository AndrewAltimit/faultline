use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::campaign::KillChain;
use crate::events::EventDefinition;
use crate::faction::Faction;
use crate::ids::{EventId, FactionId, KillChainId, NetworkId, TechCardId, VictoryId};
use crate::map::{EnvironmentSchedule, MapConfig};
use crate::network::Network;
use crate::politics::PoliticalClimate;
use crate::simulation::SimulationConfig;
use crate::stats::ConfidenceLevel;
use crate::strategy_space::StrategySpace;
use crate::tech::TechCard;
use crate::victory::VictoryCondition;

/// The complete definition of a simulation scenario.
///
/// `Scenario::default()` produces a syntactically valid but semantically
/// empty scenario (no factions, no regions). Engine validation rejects
/// the empty form, so the default is only useful as a base for
/// `..Default::default()` spread in test helpers — it lets new top-level
/// fields land without rewriting every test fixture.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scenario {
    pub meta: ScenarioMeta,
    pub map: MapConfig,
    pub factions: BTreeMap<FactionId, Faction>,
    pub technology: BTreeMap<TechCardId, TechCard>,
    pub political_climate: PoliticalClimate,
    pub events: BTreeMap<EventId, EventDefinition>,
    pub simulation: SimulationConfig,
    pub victory_conditions: BTreeMap<VictoryId, VictoryCondition>,
    /// Multi-phase kill chains. Optional — scenarios without
    /// campaign analysis omit this field entirely.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kill_chains: BTreeMap<KillChainId, KillChain>,
    /// Defender budget cap in dollars. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defender_budget: Option<f64>,
    /// Attacker budget cap in dollars. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attacker_budget: Option<f64>,
    /// Optional global environmental schedule (weather, time-of-day).
    /// Empty schedule = no effect; omitted entirely from serialized
    /// output when no windows are declared so legacy scenarios stay
    /// byte-identical.
    #[serde(default, skip_serializing_if = "EnvironmentSchedule::is_empty")]
    pub environment: EnvironmentSchedule,
    /// Optional strategy-search declaration. Names which
    /// scenario parameters are decision variables and what domain each
    /// can take. Consumed by the `--search` CLI mode in `faultline-cli`
    /// and `faultline_stats::search`. Skipped from serialization when
    /// empty so legacy scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "StrategySpace::is_empty")]
    pub strategy_space: StrategySpace,
    /// Optional typed network primitives — supply / comms /
    /// social / financial graphs declared per-scenario. Each network is
    /// independent (no cross-network nodes); cross-network coupling
    /// happens via [`crate::events::EventEffect`] firing into multiple
    /// targets. Empty by default so legacy scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub networks: BTreeMap<NetworkId, Network>,
}

/// Metadata about the scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioMeta {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
    /// Coarse author-supplied confidence tag for the scenario as a
    /// whole. Signals "this is a conceptual sketch" vs.
    /// "this is publication-ready rigor" to report readers. Orthogonal
    /// to the Wilson CIs on individual rates — those measure sampling
    /// uncertainty; this one measures parameter defensibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceLevel>,
    /// Faultline schema version this scenario was authored against.
    /// Distinct from `version` (an author-supplied scenario version
    /// string). Defaults to 1 when absent so legacy scenarios load
    /// unchanged; the migration framework
    /// (`faultline_types::migration`) advances older versions forward
    /// to `CURRENT_SCHEMA_VERSION` at load time. Always serialized so
    /// downstream hashes are stable regardless of whether the source
    /// TOML included the field explicitly.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Optional historical analogue this scenario back-tests against.
    /// `None` means the scenario is "purely synthetic" — no claim about
    /// fit to a real-world precedent. When `Some`, the calibration
    /// pipeline in `faultline_stats::calibration` compares MC outcomes
    /// against the declared observations and surfaces a per-observation
    /// verdict in the report. Skipped from serialization when absent
    /// so legacy scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical_analogue: Option<HistoricalAnalogue>,
    /// One-line statement of the analytical question the scenario is
    /// built to explore — e.g. "How much does early air-defense
    /// suppression shift the win distribution?". Purely descriptive: it
    /// has no engine effect and is never rendered into the Monte Carlo
    /// report. `None` = the author did not declare an explicit purpose.
    /// Validation rejects a present-but-empty string (a silent-no-op
    /// shape). Skipped from serialization when absent so legacy
    /// scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytical_purpose: Option<String>,
    /// Coarse category the scenario belongs to (tutorial, red-team,
    /// calibration back-test, demo, reference analogue). Purely
    /// descriptive, no engine effect, never rendered into the Monte
    /// Carlo report. Skipped from serialization when absent so legacy
    /// scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_type: Option<ScenarioType>,
    /// Public OSINT sources the scenario's parameters are derived from.
    /// Each entry is a free-form citation string (publisher / title /
    /// year is the useful minimum). This is the scenario-wide analogue
    /// of [`HistoricalAnalogue::sources`]: it documents the provenance
    /// of the capability parameters themselves, independent of any
    /// historical back-test. Purely descriptive, no engine effect.
    /// Validation rejects whitespace-only entries. Skipped from
    /// serialization when empty so legacy scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub osint_sources: Vec<String>,
    /// Short prose describing the modeled attacker's intent, doctrine,
    /// and assumed objectives — the "red" framing an analyst reads
    /// before interpreting outcomes. Purely descriptive, no engine
    /// effect. Validation rejects a present-but-empty string. Skipped
    /// from serialization when absent so legacy scenarios stay
    /// byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_team_profile: Option<String>,
    /// Short prose describing the modeled defender's posture, readiness,
    /// and assumed constraints — the "blue" framing. Purely descriptive,
    /// no engine effect. Validation rejects a present-but-empty string.
    /// Skipped from serialization when absent so legacy scenarios stay
    /// byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blue_team_posture: Option<String>,
    /// Free-form names of the parameters the author considers most
    /// consequential to the outcome distribution — a reading guide for
    /// where to point `--sensitivity` / `--robustness`. Purely
    /// descriptive: this does NOT drive the sensitivity sweep (that is
    /// the engine's job); it records author intent. Validation rejects
    /// whitespace-only entries. Skipped from serialization when empty so
    /// legacy scenarios stay byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensitivity_parameters: Vec<String>,
}

/// Coarse author-declared category for a scenario.
///
/// Purely descriptive metadata — it has no engine effect and is never
/// rendered into the deterministic Monte Carlo report. Serialized in
/// `snake_case` so the TOML reads `scenario_type = "red_team"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioType {
    /// A teaching scenario that demonstrates one mechanic in isolation.
    Tutorial,
    /// An adversary-perspective scenario probing attacker options.
    RedTeam,
    /// A scenario whose purpose is to back-test against a historical
    /// precedent (usually paired with a `historical_analogue`).
    Calibration,
    /// A showcase scenario for the CLI / browser demo surface.
    Demo,
    /// A reference scenario modelling a named real-world analogue.
    Reference,
}

fn default_schema_version() -> u32 {
    crate::migration::CURRENT_SCHEMA_VERSION
}

impl Default for ScenarioMeta {
    /// Default targets the current schema version so test helpers built
    /// from `..Default::default()` don't accidentally emit
    /// `schema_version = 0` and trip the migration framework's
    /// stale-fixture warning.
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            author: String::new(),
            version: String::new(),
            tags: Vec::new(),
            confidence: None,
            schema_version: default_schema_version(),
            historical_analogue: None,
            analytical_purpose: None,
            scenario_type: None,
            osint_sources: Vec::new(),
            red_team_profile: None,
            blue_team_posture: None,
            sensitivity_parameters: Vec::new(),
        }
    }
}

/// A real-world precedent the scenario claims to model.
///
/// Authoring a `historical_analogue` is the scenario author's commitment
/// that the engine output, run with these parameters, *should* match
/// what actually happened in the named precedent within the declared
/// uncertainty. The calibration pipeline in
/// `faultline_stats::calibration` then computes per-observation
/// verdicts (does the MC outcome distribution overlap the historical
/// one?) and rolls them up into a scenario-wide calibration confidence.
///
/// This is the substrate Epic N is built on: until a scenario declares
/// what it's modelling, every output is internally consistent but
/// externally unjustified. A scenario without a `historical_analogue`
/// is reported as "purely synthetic" — the analyst is told what that
/// means for result interpretation rather than left to assume the
/// numbers are calibrated.
///
/// Sources must be generic OSINT references (RAND, IISS, CRS, academic
/// literature, congressional testimony, etc.) — see `LEGAL.md`. Specific
/// external threat-assessment publication series are blocked by the
/// `tools/ci/grep-guard.sh` CI step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoricalAnalogue {
    /// Short name for the precedent, used as the section header in
    /// reports. e.g. "Russo-Georgian War (2008)".
    pub name: String,
    /// One-paragraph description of the precedent, explaining what is
    /// being modelled and what it is not. The author is expected to be
    /// honest about what the analogue captures vs. what is missing.
    pub description: String,
    /// Free-form date or date-range. Not parsed; the engine treats this
    /// as a label for human readers. Examples: "2008-08-07 to
    /// 2008-08-12", "Q4 2014", "1979-1989".
    pub period: String,
    /// Open-source citations supporting the observations below. Each
    /// entry is a free-form reference string — author / publisher /
    /// title is the minimum the report renderer can usefully display.
    /// Required to be non-empty by validation: an analogue without
    /// sources is conceptually a back-test against the author's
    /// recollection, which Epic N is explicitly designed to discourage.
    pub sources: Vec<String>,
    /// Author's coarse confidence in the analogue's representativeness
    /// (not in the historical record itself — per-observation
    /// confidence captures that). High = "this scenario is structurally
    /// a faithful model of the precedent"; Low = "this is the closest
    /// analogue I could find but the structural fit is loose".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceLevel>,
    /// One or more observed outcomes the MC distribution should match.
    /// Validation requires at least one — an analogue with zero
    /// observations is a label without content.
    pub observations: Vec<HistoricalObservation>,
}

/// A single observed outcome from the historical precedent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoricalObservation {
    /// What was observed. The variant carries any metric-specific
    /// fields (named faction for `Winner` / `WinRate`, range for
    /// `DurationTicks`).
    pub metric: HistoricalMetric,
    /// Author's confidence in the historical record for this specific
    /// observation. A `Winner` observation for a clean conventional
    /// conflict is High; a `WinRate` observation extrapolated from a
    /// handful of similar incidents is Low. Surfaced alongside the
    /// calibration verdict so the reader can weight a pass/fail
    /// against how solid the ground truth is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceLevel>,
    /// Free-form notes about how this observation was derived from the
    /// sources. The renderer surfaces this directly so the analyst sees
    /// the author's reasoning, not just the verdict.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// What kind of historical observation is being made.
///
/// Adding a new variant requires updating
/// `faultline_stats::calibration::evaluate_observation` — the match is
/// exhaustive and will fail to compile otherwise. That's the intended
/// failure mode: a new metric without MC reduction logic would silently
/// produce no calibration verdict, which defeats the section's purpose.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoricalMetric {
    /// The named faction was the historical victor. Calibration =
    /// MC probability mass on this faction as outcome winner.
    Winner { faction: FactionId },
    /// The named faction won with frequency in `[low, high]` across a
    /// reference set of similar precedents. Calibration = MC win rate
    /// for this faction, plus whether its Wilson CI overlaps the
    /// historical interval.
    WinRate {
        faction: FactionId,
        low: f64,
        high: f64,
    },
    /// The conflict resolved in `[low, high]` ticks (inclusive on both
    /// ends). Calibration = fraction of MC runs whose `final_tick`
    /// falls in the interval.
    DurationTicks { low: u64, high: u64 },
}

/// Validate the descriptive author-supplied metadata on a
/// [`ScenarioMeta`].
///
/// These fields have no engine effect, but the project convention is to
/// **fail loud at scenario load** rather than silently accept a
/// no-op shape. The realistic failure modes for descriptive metadata are
/// *present-but-empty* values: a `analytical_purpose = ""` or an
/// `osint_sources = [""]` entry looks authored but carries no
/// information, so we reject them at load instead of letting them slip
/// through as a silent no-op.
///
/// Wired into [`crate::migration::load_scenario_str`], which is the
/// single load path the CLI and the WASM frontend both route through, so
/// every scenario is checked regardless of caller. Returns the first
/// failure as a human-readable string (mirroring
/// [`crate::belief::validate_belief_model`]).
pub fn validate_meta(meta: &ScenarioMeta) -> Result<(), String> {
    // Present-but-empty optional prose: the key was authored but holds
    // no content. Reject so the author either fills it in or drops it.
    if let Some(purpose) = meta.analytical_purpose.as_ref()
        && purpose.trim().is_empty()
    {
        return Err("meta.analytical_purpose is present but empty; \
             state the question the scenario explores or remove the key"
            .to_string());
    }
    if let Some(profile) = meta.red_team_profile.as_ref()
        && profile.trim().is_empty()
    {
        return Err("meta.red_team_profile is present but empty; \
             describe the attacker's intent or remove the key"
            .to_string());
    }
    if let Some(posture) = meta.blue_team_posture.as_ref()
        && posture.trim().is_empty()
    {
        return Err("meta.blue_team_posture is present but empty; \
             describe the defender's posture or remove the key"
            .to_string());
    }
    // Vec fields: a whitespace-only entry is a citation / parameter name
    // that names nothing. Reject it rather than render an empty bullet.
    for (idx, src) in meta.osint_sources.iter().enumerate() {
        if src.trim().is_empty() {
            return Err(format!(
                "meta.osint_sources[{idx}] is empty or whitespace-only; \
                 every source must be a non-empty citation string"
            ));
        }
    }
    for (idx, param) in meta.sensitivity_parameters.iter().enumerate() {
        if param.trim().is_empty() {
            return Err(format!(
                "meta.sensitivity_parameters[{idx}] is empty or \
                 whitespace-only; every entry must name a parameter"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ScenarioMeta {
        ScenarioMeta::default()
    }

    #[test]
    fn validate_meta_accepts_default() {
        validate_meta(&meta()).expect("empty metadata is valid");
    }

    #[test]
    fn validate_meta_accepts_fully_populated() {
        let m = ScenarioMeta {
            analytical_purpose: Some("Does early SEAD shift the win rate?".to_string()),
            scenario_type: Some(ScenarioType::RedTeam),
            osint_sources: vec![
                "IISS Military Balance 2024".to_string(),
                "CRS R45995".to_string(),
            ],
            red_team_profile: Some("Air-centric opening salvo.".to_string()),
            blue_team_posture: Some("Layered IADS, degraded readiness.".to_string()),
            sensitivity_parameters: vec!["sead_effectiveness".to_string()],
            ..meta()
        };
        validate_meta(&m).expect("fully populated metadata is valid");
    }

    #[test]
    fn validate_meta_rejects_empty_analytical_purpose() {
        let m = ScenarioMeta {
            analytical_purpose: Some("   ".to_string()),
            ..meta()
        };
        let err = validate_meta(&m).expect_err("present-but-empty purpose must be rejected");
        assert!(err.contains("analytical_purpose"), "got: {err}");
    }

    #[test]
    fn validate_meta_rejects_empty_red_team_profile() {
        let m = ScenarioMeta {
            red_team_profile: Some(String::new()),
            ..meta()
        };
        let err = validate_meta(&m).expect_err("empty red_team_profile must be rejected");
        assert!(err.contains("red_team_profile"), "got: {err}");
    }

    #[test]
    fn validate_meta_rejects_empty_blue_team_posture() {
        let m = ScenarioMeta {
            blue_team_posture: Some("\t".to_string()),
            ..meta()
        };
        let err = validate_meta(&m).expect_err("whitespace blue_team_posture must be rejected");
        assert!(err.contains("blue_team_posture"), "got: {err}");
    }

    #[test]
    fn validate_meta_rejects_whitespace_osint_source() {
        let m = ScenarioMeta {
            osint_sources: vec!["IISS Military Balance".to_string(), "  ".to_string()],
            ..meta()
        };
        let err = validate_meta(&m).expect_err("whitespace-only source must be rejected");
        assert!(err.contains("osint_sources[1]"), "got: {err}");
    }

    #[test]
    fn validate_meta_rejects_empty_sensitivity_parameter() {
        let m = ScenarioMeta {
            sensitivity_parameters: vec![String::new()],
            ..meta()
        };
        let err = validate_meta(&m).expect_err("empty sensitivity parameter name must be rejected");
        assert!(err.contains("sensitivity_parameters[0]"), "got: {err}");
    }

    #[test]
    fn scenario_type_round_trips_snake_case() {
        let toml_str = "\
            name = \"x\"\n\
            description = \"\"\n\
            author = \"\"\n\
            version = \"\"\n\
            tags = []\n\
            scenario_type = \"red_team\"\n";
        let m: ScenarioMeta = toml::from_str(toml_str).expect("parse");
        assert_eq!(m.scenario_type, Some(ScenarioType::RedTeam));
        let back = toml::to_string(&m).expect("serialize");
        assert!(back.contains("scenario_type = \"red_team\""), "got: {back}");
    }

    #[test]
    fn new_meta_fields_omitted_when_absent() {
        // Serialization of a default meta must not emit any of the new
        // optional keys, so legacy scenarios stay byte-identical.
        let back = toml::to_string(&meta()).expect("serialize");
        for key in [
            "analytical_purpose",
            "scenario_type",
            "osint_sources",
            "red_team_profile",
            "blue_team_posture",
            "sensitivity_parameters",
        ] {
            assert!(
                !back.contains(key),
                "default meta must omit `{key}`; got: {back}"
            );
        }
    }
}
