use serde::{Deserialize, Serialize};
use thiserror::Error;

use faultline_types::ids::TechCardId;
use faultline_types::map::TerrainType;
use faultline_types::tech::{TechCard, TechEffect};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during technology effect resolution.
#[derive(Debug, Error)]
pub enum TechError {
    #[error("tech card not found: {0}")]
    CardNotFound(TechCardId),

    #[error("invalid effect configuration: {0}")]
    InvalidEffect(String),

    #[error("conflicting tech requirements: {0}")]
    Conflict(String),
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A resolved tech effect with the final magnitude after terrain
/// modifiers have been applied.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedEffect {
    /// The original effect definition.
    pub effect: TechEffect,
    /// Final effectiveness multiplier in `(0.0, ...]`.
    pub effectiveness: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply terrain modifiers to a tech card's effects and return the
/// list of resolved effects with adjusted effectiveness.
///
/// Each effect's base value is scaled by the terrain modifier that
/// matches the given terrain type (if any). When no modifier matches,
/// the effectiveness defaults to `1.0`.
pub fn apply_tech_effects(tech: &TechCard, terrain: &TerrainType) -> Vec<ResolvedEffect> {
    // Find the terrain modifier for the current terrain type.
    let terrain_effectiveness = tech
        .terrain_modifiers
        .iter()
        .find(|m| m.terrain == *terrain)
        .map_or(1.0, |m| m.effectiveness);

    tech.effects
        .iter()
        .map(|effect| ResolvedEffect {
            effect: effect.clone(),
            effectiveness: terrain_effectiveness,
        })
        .collect()
}

/// Check whether the given tech card is countered by any of the
/// currently active tech cards.
pub fn is_countered(tech: &TechCard, active_techs: &[TechCardId]) -> bool {
    tech.countered_by
        .iter()
        .any(|counter| active_techs.contains(counter))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::tech::{TechCategory, TerrainTechModifier};

    fn sample_card() -> TechCard {
        TechCard {
            id: TechCardId::from("cyber-01"),
            name: "Cyber Disruption Suite".into(),
            description: "Disrupts enemy networks".into(),
            category: TechCategory::Cyber,
            effects: vec![TechEffect::CommsDisruption { factor: 0.6 }],
            cost_per_tick: 2.0,
            deployment_cost: 10.0,
            countered_by: vec![TechCardId::from("firewall-01")],
            terrain_modifiers: vec![
                TerrainTechModifier {
                    terrain: TerrainType::Urban,
                    effectiveness: 1.5,
                },
                TerrainTechModifier {
                    terrain: TerrainType::Desert,
                    effectiveness: 0.5,
                },
            ],
            coverage_limit: Some(3),
        }
    }

    #[test]
    fn terrain_bonus_applies() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Urban);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn terrain_penalty_applies() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Desert);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_effectiveness_when_no_modifier() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Arctic);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn is_countered_true_when_active() {
        let card = sample_card();
        let active = vec![TechCardId::from("firewall-01")];
        assert!(is_countered(&card, &active));
    }

    #[test]
    fn is_countered_false_when_not_active() {
        let card = sample_card();
        let active = vec![TechCardId::from("other-99")];
        assert!(!is_countered(&card, &active));
    }

    #[test]
    fn apply_tech_effects_multiple_effects() {
        let card = TechCard {
            id: TechCardId::from("multi-01"),
            name: "Multi-Effect Suite".into(),
            description: "Three distinct effects".into(),
            category: TechCategory::Cyber,
            effects: vec![
                TechEffect::CommsDisruption { factor: 0.5 },
                TechEffect::CombatModifier { factor: 1.2 },
                TechEffect::DetectionModifier { factor: 0.8 },
            ],
            cost_per_tick: 1.0,
            deployment_cost: 5.0,
            countered_by: vec![],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        let resolved = apply_tech_effects(&card, &TerrainType::Urban);
        assert_eq!(resolved.len(), 3, "all three effects should be resolved");
        // No terrain modifier configured, so effectiveness should be 1.0.
        for r in &resolved {
            assert!(
                (r.effectiveness - 1.0).abs() < f64::EPSILON,
                "default effectiveness should be 1.0"
            );
        }
    }

    #[test]
    fn apply_tech_effects_empty_effects() {
        let card = TechCard {
            id: TechCardId::from("empty-01"),
            name: "No Effects".into(),
            description: "Card with no effects".into(),
            category: TechCategory::Surveillance,
            effects: vec![],
            cost_per_tick: 0.0,
            deployment_cost: 0.0,
            countered_by: vec![],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        let resolved = apply_tech_effects(&card, &TerrainType::Forest);
        assert!(
            resolved.is_empty(),
            "empty effects should return empty resolved list"
        );
    }

    #[test]
    fn is_countered_with_multiple_counters() {
        let card = TechCard {
            id: TechCardId::from("drone-01"),
            name: "Drone Swarm".into(),
            description: "Autonomous drone swarm".into(),
            category: TechCategory::OffensiveDrone,
            effects: vec![TechEffect::CombatModifier { factor: 1.5 }],
            cost_per_tick: 3.0,
            deployment_cost: 20.0,
            countered_by: vec![
                TechCardId::from("counter-uas-01"),
                TechCardId::from("ew-jammer-01"),
            ],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        // Only one of the two counters is active.
        let active = vec![
            TechCardId::from("unrelated-99"),
            TechCardId::from("ew-jammer-01"),
        ];
        assert!(
            is_countered(&card, &active),
            "card should be countered when one of its counters is active"
        );

        // Neither counter is active.
        let no_counter = vec![TechCardId::from("unrelated-99")];
        assert!(
            !is_countered(&card, &no_counter),
            "card should not be countered when no counter is active"
        );
    }
}
