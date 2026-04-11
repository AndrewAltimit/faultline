use serde::{Deserialize, Serialize};
use thiserror::Error;

use faultline_types::faction::Institution;
use faultline_types::ids::{FactionId, InstitutionId, SegmentId};
use faultline_types::politics::{CivilianAction, PoliticalClimate};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during political climate operations.
#[derive(Debug, Error)]
pub enum PoliticsError {
    #[error("institution not found: {0}")]
    InstitutionNotFound(InstitutionId),

    #[error("faction not found: {0}")]
    FactionNotFound(FactionId),

    #[error("tension value out of range: {0}")]
    TensionOutOfRange(f64),

    #[error("invalid segment configuration: {0}")]
    InvalidSegment(String),
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A delta applied to tension values during an update.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TensionDelta {
    /// Optional faction scope; `None` means global tension.
    pub faction: Option<FactionId>,
    /// Additive change to tension (can be negative).
    pub delta: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Update the political climate's tension from a list of deltas.
///
/// Global tension is clamped to `[0.0, 1.0]`.
pub fn update_tension(climate: &mut PoliticalClimate, events: &[TensionDelta]) {
    for event in events {
        if event.faction.is_none() {
            climate.tension = (climate.tension + event.delta).clamp(0.0, 1.0);
        }
        // Faction-specific tension is not yet tracked in PoliticalClimate;
        // global tension is the only target for now.
    }
}

/// Evaluate the effective loyalty of an institution given the
/// current political climate.
///
/// Returns a loyalty score in `[0.0, 1.0]`. Higher global tension
/// erodes institutional loyalty.
pub fn evaluate_loyalty(institution: &Institution, climate: &PoliticalClimate) -> f64 {
    let erosion = climate.tension * 0.3;
    (institution.loyalty - erosion).clamp(0.0, 1.0)
}

/// Check whether an institution has fractured (loyalty below its
/// fracture threshold).
///
/// Returns `false` if no fracture threshold is configured.
pub fn check_fracture(institution: &Institution) -> bool {
    match institution.fracture_threshold {
        Some(threshold) => institution.loyalty < threshold,
        None => false,
    }
}

/// A segment that was newly activated this tick.
#[derive(Clone, Debug)]
pub struct ActivationResult {
    pub segment_id: SegmentId,
    pub favored_faction: FactionId,
    pub actions: Vec<CivilianAction>,
    pub concentrated_in: Vec<faultline_types::ids::RegionId>,
}

/// Stochastically update civilian population segment sympathies.
///
/// Each segment's faction sympathies drift slightly based on the
/// segment's volatility and the current global tension.
///
/// Returns a list of segments that were newly activated this tick.
pub fn update_civilian_segments(
    climate: &mut PoliticalClimate,
    _tick: u32,
    rng: &mut impl rand::Rng,
) -> Vec<ActivationResult> {
    let global_tension = climate.tension;
    let mut activations = Vec::new();

    for segment in &mut climate.population_segments {
        for sympathy in &mut segment.sympathies {
            // Small random drift scaled by volatility and tension.
            let noise: f64 = (rng.r#gen::<f64>() - 0.5) * segment.volatility * 0.1;
            let tension_pull = (global_tension - 0.5) * 0.02;
            sympathy.sympathy = (sympathy.sympathy + noise + tension_pull).clamp(-1.0, 1.0);
        }

        // Check activation threshold.
        if !segment.activated && !segment.sympathies.is_empty() {
            let max_sym = segment
                .sympathies
                .iter()
                .max_by(|a, b| a.sympathy.total_cmp(&b.sympathy));

            if let Some(top) = max_sym
                && top.sympathy >= segment.activation_threshold
            {
                segment.activated = true;
                activations.push(ActivationResult {
                    segment_id: segment.id.clone(),
                    favored_faction: top.faction.clone(),
                    actions: segment.activation_actions.clone(),
                    concentrated_in: segment.concentrated_in.clone(),
                });
            }
        }
    }

    activations
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::faction::{Institution, InstitutionType};
    use faultline_types::politics::MediaLandscape;

    fn sample_climate() -> PoliticalClimate {
        PoliticalClimate {
            tension: 0.5,
            institutional_trust: 0.7,
            media_landscape: MediaLandscape {
                fragmentation: 0.5,
                disinformation_susceptibility: 0.3,
                state_control: 0.4,
                social_media_penetration: 0.8,
                internet_availability: 0.9,
            },
            population_segments: Vec::new(),
            global_modifiers: Vec::new(),
        }
    }

    fn sample_institution() -> Institution {
        Institution {
            id: InstitutionId::from("mil-hq"),
            name: "Military HQ".into(),
            institution_type: InstitutionType::NationalGuard,
            loyalty: 0.8,
            effectiveness: 0.7,
            personnel: 5000,
            fracture_threshold: Some(0.3),
        }
    }

    #[test]
    fn update_tension_clamps_to_bounds() {
        let mut climate = sample_climate();
        climate.tension = 0.9;

        update_tension(
            &mut climate,
            &[TensionDelta {
                faction: None,
                delta: 0.5,
            }],
        );
        assert!((climate.tension - 1.0).abs() < f64::EPSILON);

        update_tension(
            &mut climate,
            &[TensionDelta {
                faction: None,
                delta: -2.0,
            }],
        );
        assert!(climate.tension.abs() < f64::EPSILON);
    }

    #[test]
    fn evaluate_loyalty_erodes_with_tension() {
        let institution = sample_institution();
        let mut climate = sample_climate();
        climate.tension = 1.0;

        let loyalty = evaluate_loyalty(&institution, &climate);
        assert!((loyalty - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn check_fracture_detects_low_loyalty() {
        let intact = sample_institution();
        assert!(!check_fracture(&intact));

        let fractured = Institution {
            loyalty: 0.2,
            ..sample_institution()
        };
        assert!(check_fracture(&fractured));
    }

    #[test]
    fn check_fracture_returns_false_without_threshold() {
        let inst = Institution {
            fracture_threshold: None,
            loyalty: 0.0,
            ..sample_institution()
        };
        assert!(!check_fracture(&inst));
    }

    #[test]
    fn update_civilian_segments_shifts_sympathies() {
        use faultline_types::ids::{FactionId, SegmentId};
        use faultline_types::politics::{FactionSympathy, PopulationSegment};
        use rand::SeedableRng;

        let mut climate = sample_climate();
        climate.tension = 0.9; // high tension
        climate.population_segments.push(PopulationSegment {
            id: SegmentId::from("urban-pop"),
            name: "Urban Population".into(),
            fraction: 0.6,
            concentrated_in: vec![],
            sympathies: vec![
                FactionSympathy {
                    faction: FactionId::from("gov"),
                    sympathy: 0.5,
                },
                FactionSympathy {
                    faction: FactionId::from("rebel"),
                    sympathy: -0.3,
                },
            ],
            activation_threshold: 0.8,
            activation_actions: vec![],
            volatility: 0.8,
            activated: false,
        });

        let original_sympathies: Vec<f64> = climate.population_segments[0]
            .sympathies
            .iter()
            .map(|s| s.sympathy)
            .collect();

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        // Run several ticks to increase chance of drift.
        for tick in 0..10 {
            update_civilian_segments(&mut climate, tick, &mut rng);
        }

        let updated_sympathies: Vec<f64> = climate.population_segments[0]
            .sympathies
            .iter()
            .map(|s| s.sympathy)
            .collect();

        // With high tension and volatility, sympathies should have shifted.
        let any_changed = original_sympathies
            .iter()
            .zip(updated_sympathies.iter())
            .any(|(orig, upd)| (orig - upd).abs() > f64::EPSILON);
        assert!(any_changed, "sympathies should shift after updates");

        // All values should remain clamped within [-1.0, 1.0].
        for s in &updated_sympathies {
            assert!(
                *s >= -1.0 && *s <= 1.0,
                "sympathy {s} should be clamped to [-1.0, 1.0]"
            );
        }
    }

    #[test]
    fn evaluate_loyalty_clamps_to_zero() {
        // Institution with very low loyalty and very high tension.
        let inst = Institution {
            loyalty: 0.1,
            ..sample_institution()
        };
        let mut climate = sample_climate();
        climate.tension = 1.0; // max tension -> erosion = 0.3

        let loyalty = evaluate_loyalty(&inst, &climate);
        // 0.1 - 0.3 = -0.2, clamped to 0.0
        assert!(
            loyalty >= 0.0,
            "loyalty should never be negative, got {loyalty}"
        );
        assert!(
            loyalty.abs() < f64::EPSILON,
            "loyalty should clamp to 0.0, got {loyalty}"
        );
    }

    #[test]
    fn update_tension_multiple_deltas() {
        let mut climate = sample_climate();
        climate.tension = 0.5;

        let deltas = vec![
            TensionDelta {
                faction: None,
                delta: 0.1,
            },
            TensionDelta {
                faction: None,
                delta: 0.15,
            },
            TensionDelta {
                faction: None,
                delta: -0.05,
            },
        ];
        update_tension(&mut climate, &deltas);
        // 0.5 + 0.1 + 0.15 - 0.05 = 0.7
        assert!(
            (climate.tension - 0.7).abs() < 1e-10,
            "cumulative tension should be 0.7, got {}",
            climate.tension
        );
    }
}
