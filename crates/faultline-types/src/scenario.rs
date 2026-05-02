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
        }
    }
}
