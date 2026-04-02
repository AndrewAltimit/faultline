use faultline_events::EventError;
use faultline_geo::GeoError;
use faultline_types::ids::{FactionId, RegionId};

/// All errors that can arise from the simulation engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("scenario validation failed: {0}")]
    ScenarioInvalid(String),

    #[error("faction not found: {0}")]
    FactionNotFound(FactionId),

    #[error("region not found: {0}")]
    RegionNotFound(RegionId),

    #[error("geography error: {0}")]
    Geo(#[from] GeoError),

    #[error("event error: {0}")]
    Event(#[from] EventError),

    #[error("simulation exceeded max ticks ({0})")]
    MaxTicksExceeded(u32),

    #[error("no factions in scenario")]
    NoFactions,

    #[error("no regions in scenario")]
    NoRegions,

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}
