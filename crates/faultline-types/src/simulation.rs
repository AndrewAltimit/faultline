use serde::{Deserialize, Serialize};

use crate::belief::BeliefModelConfig;

/// Top-level simulation parameters.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub max_ticks: u32,
    pub tick_duration: TickDuration,
    pub monte_carlo_runs: u32,
    pub seed: Option<u64>,
    pub fog_of_war: bool,
    pub attrition_model: AttritionModel,
    pub snapshot_interval: u32,
    /// Belief-asymmetry model configuration (Epic M round-one).
    /// `None` = legacy fast path (the engine's belief phase
    /// short-circuits and the AI consumes ground truth as before).
    /// `Some(cfg)` with `cfg.enabled = true` activates per-faction
    /// persistent belief state, observation-driven updates, decay,
    /// and the deception / intel-share event variants. Adding the
    /// field with `enabled = false` is bit-identical to omitting it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub belief_model: Option<BeliefModelConfig>,
}

/// How much real-world time each tick represents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TickDuration {
    Hours(u32),
    Days(u32),
    Weeks(u32),
}

impl Default for TickDuration {
    fn default() -> Self {
        Self::Days(1)
    }
}

/// The combat attrition model to use.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum AttritionModel {
    #[default]
    LanchesterLinear,
    LanchesterSquare,
    Hybrid,
    Stochastic {
        noise: f64,
    },
}
