use serde::{Deserialize, Serialize};

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
