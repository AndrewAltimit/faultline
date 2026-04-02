//! Combat resolution using Lanchester attrition models.

use rand::Rng;

use faultline_types::simulation::AttritionModel;

/// Outcome of a combat engagement between two forces.
#[derive(Clone, Debug)]
pub struct CombatResult {
    /// Strength lost by side A.
    pub attrition_a: f64,
    /// Strength lost by side B.
    pub attrition_b: f64,
    /// Whether side A routed (morale collapsed).
    pub rout_a: bool,
    /// Whether side B routed.
    pub rout_b: bool,
    /// Whether side A surrendered.
    pub surrender_a: bool,
    /// Whether side B surrendered.
    pub surrender_b: bool,
}

/// Parameters for a single combat engagement.
#[derive(Clone, Debug)]
pub struct CombatParams {
    /// Strength of side A.
    pub strength_a: f64,
    /// Strength of side B.
    pub strength_b: f64,
    /// Morale of side A in `[0.0, 1.0]`.
    pub morale_a: f64,
    /// Morale of side B in `[0.0, 1.0]`.
    pub morale_b: f64,
    /// Terrain defense modifier for the defender (side B).
    pub terrain_defense: f64,
    /// Tech combat modifier for side A (multiplicative bonus).
    pub tech_modifier_a: f64,
    /// Tech combat modifier for side B.
    pub tech_modifier_b: f64,
    /// Whether side A uses guerrilla tactics.
    pub guerrilla_a: bool,
    /// Whether side B uses guerrilla tactics.
    pub guerrilla_b: bool,
    /// Base attrition coefficient.
    pub attrition_coeff: f64,
}

impl Default for CombatParams {
    fn default() -> Self {
        Self {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.8,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        }
    }
}

/// Resolve combat between two forces using the specified attrition
/// model. Returns a [`CombatResult`] describing losses and morale
/// outcomes.
pub fn resolve_combat(
    params: &CombatParams,
    model: &AttritionModel,
    rng: &mut impl Rng,
) -> CombatResult {
    let k = params.attrition_coeff;

    // Compute raw attrition based on model.
    let (raw_att_a, raw_att_b) = match model {
        AttritionModel::LanchesterLinear => {
            lanchester_linear(k, params.strength_a, params.strength_b)
        },
        AttritionModel::LanchesterSquare => {
            lanchester_square(k, params.strength_a, params.strength_b)
        },
        AttritionModel::Hybrid => hybrid_attrition(
            k,
            params.strength_a,
            params.strength_b,
            params.guerrilla_a,
            params.guerrilla_b,
        ),
        AttritionModel::Stochastic { noise } => {
            let (base_a, base_b) = lanchester_linear(k, params.strength_a, params.strength_b);
            stochastic_noise(base_a, base_b, *noise, rng)
        },
    };

    // Apply modifiers: terrain defense helps side B, tech modifiers
    // increase the damage each side inflicts.
    let morale_mod_a = morale_modifier(params.morale_a);
    let morale_mod_b = morale_modifier(params.morale_b);

    // Attrition A receives = damage from B * B's modifiers / A's defense
    let attrition_a = (raw_att_a * params.tech_modifier_b * morale_mod_b).max(0.0);

    // Attrition B receives = damage from A * A's modifiers / B's defense
    let attrition_b = (raw_att_b * params.tech_modifier_a * morale_mod_a
        / params.terrain_defense.max(0.1))
    .max(0.0);

    // Morale checks: project post-combat morale.
    let post_morale_a = project_morale(params.morale_a, attrition_a, params.strength_a);
    let post_morale_b = project_morale(params.morale_b, attrition_b, params.strength_b);

    CombatResult {
        attrition_a,
        attrition_b,
        rout_a: (0.1..0.2).contains(&post_morale_a),
        rout_b: (0.1..0.2).contains(&post_morale_b),
        surrender_a: post_morale_a < 0.1,
        surrender_b: post_morale_b < 0.1,
    }
}

// -----------------------------------------------------------------------
// Lanchester models
// -----------------------------------------------------------------------

/// Linear law: attrition proportional to enemy strength.
/// Used for area-fire / guerrilla engagements.
fn lanchester_linear(k: f64, str_a: f64, str_b: f64) -> (f64, f64) {
    let att_a = k * str_b;
    let att_b = k * str_a;
    (att_a, att_b)
}

/// Square law: attrition proportional to enemy strength squared.
/// Used for aimed-fire / conventional engagements.
fn lanchester_square(k: f64, str_a: f64, str_b: f64) -> (f64, f64) {
    let att_a = k * str_b * str_b;
    let att_b = k * str_a * str_a;
    (att_a, att_b)
}

/// Hybrid: guerrilla units use linear law, conventional use square.
fn hybrid_attrition(
    k: f64,
    str_a: f64,
    str_b: f64,
    guerrilla_a: bool,
    guerrilla_b: bool,
) -> (f64, f64) {
    // Damage inflicted ON side A (by B):
    let att_a = if guerrilla_b {
        // B is guerrilla -> linear law for B's attack
        k * str_b
    } else {
        k * str_b * str_b
    };

    // Damage inflicted ON side B (by A):
    let att_b = if guerrilla_a {
        k * str_a
    } else {
        k * str_a * str_a
    };

    (att_a, att_b)
}

/// Add stochastic noise to base attrition values.
fn stochastic_noise(base_a: f64, base_b: f64, noise: f64, rng: &mut impl Rng) -> (f64, f64) {
    let jitter_a: f64 = 1.0 + (rng.r#gen::<f64>() - 0.5) * 2.0 * noise;
    let jitter_b: f64 = 1.0 + (rng.r#gen::<f64>() - 0.5) * 2.0 * noise;
    ((base_a * jitter_a).max(0.0), (base_b * jitter_b).max(0.0))
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Morale modifier: higher morale = more effective combat.
/// Diminishing returns above 0.8; steep penalty below 0.3.
fn morale_modifier(morale: f64) -> f64 {
    if morale >= 0.8 {
        1.0 + (morale - 0.8) * 0.5
    } else if morale >= 0.3 {
        0.6 + (morale - 0.3) * 0.8
    } else {
        (morale * 2.0).max(0.1)
    }
}

/// Project morale after taking casualties.
fn project_morale(current_morale: f64, casualties: f64, strength: f64) -> f64 {
    if strength <= 0.0 {
        return 0.0;
    }
    let casualty_ratio = (casualties / strength).min(1.0);
    // Heavy losses tank morale.
    let morale_loss = casualty_ratio * 0.5;
    (current_morale - morale_loss).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_law_symmetric() {
        let (a, b) = lanchester_linear(0.01, 100.0, 100.0);
        assert!((a - b).abs() < f64::EPSILON);
        assert!((a - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn square_law_favors_larger_force() {
        let (att_a, att_b) = lanchester_square(0.01, 200.0, 100.0);
        // Side A is stronger so inflicts more damage (att_b > att_a)
        assert!(att_b > att_a);
    }

    #[test]
    fn morale_modifier_increases_with_morale() {
        let low = morale_modifier(0.2);
        let high = morale_modifier(0.9);
        assert!(high > low);
    }

    #[test]
    fn project_morale_drops_with_casualties() {
        let m = project_morale(0.8, 50.0, 100.0);
        assert!(m < 0.8);
        assert!(m > 0.0);
    }

    #[test]
    fn resolve_combat_symmetric() {
        let params = CombatParams {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.8,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let mut rng = rand::thread_rng();
        let result = resolve_combat(&params, &AttritionModel::LanchesterLinear, &mut rng);
        // Both sides should take attrition.
        assert!(result.attrition_a > 0.0, "side A should take damage");
        assert!(result.attrition_b > 0.0, "side B should take damage");
        // Symmetric inputs => attrition values should be equal.
        assert!(
            (result.attrition_a - result.attrition_b).abs() < 1e-6,
            "symmetric forces should take equal attrition"
        );
    }

    #[test]
    fn resolve_combat_asymmetric() {
        let params = CombatParams {
            strength_a: 200.0,
            strength_b: 100.0,
            morale_a: 0.8,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let mut rng = rand::thread_rng();
        let result = resolve_combat(&params, &AttritionModel::LanchesterLinear, &mut rng);
        // Stronger side (A) should take less damage than weaker side (B).
        assert!(
            result.attrition_a < result.attrition_b,
            "stronger side should take less damage: a={} b={}",
            result.attrition_a,
            result.attrition_b,
        );
    }

    #[test]
    fn resolve_combat_terrain_defense_bonus() {
        let base_params = CombatParams {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.8,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let mut rng = rand::thread_rng();
        let result_no_terrain =
            resolve_combat(&base_params, &AttritionModel::LanchesterLinear, &mut rng);

        let defended_params = CombatParams {
            terrain_defense: 1.5,
            ..base_params
        };
        let result_terrain = resolve_combat(
            &defended_params,
            &AttritionModel::LanchesterLinear,
            &mut rng,
        );

        // Defender (side B) with terrain bonus should take less damage.
        assert!(
            result_terrain.attrition_b < result_no_terrain.attrition_b,
            "terrain defense should reduce defender attrition: \
             defended={} undefended={}",
            result_terrain.attrition_b,
            result_no_terrain.attrition_b,
        );
    }

    #[test]
    fn rout_on_low_morale() {
        // Morale 0.15 should lead to rout when taking attrition.
        let params = CombatParams {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.15,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let mut rng = rand::thread_rng();
        let result = resolve_combat(&params, &AttritionModel::LanchesterLinear, &mut rng);
        // Post-morale for A: 0.15 - (attrition_a / 100) * 0.5
        // With low starting morale, rout (0.1..0.2 range) is expected.
        assert!(
            result.rout_a || result.surrender_a,
            "low morale force should rout or surrender"
        );
    }

    #[test]
    fn surrender_on_very_low_morale() {
        // Morale 0.05 should lead to surrender (post morale < 0.1).
        let params = CombatParams {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.05,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let mut rng = rand::thread_rng();
        let result = resolve_combat(&params, &AttritionModel::LanchesterLinear, &mut rng);
        assert!(result.surrender_a, "very low morale force should surrender");
    }

    #[test]
    fn stochastic_model_produces_different_results() {
        use rand::SeedableRng;
        use rand_chacha::ChaCha8Rng;

        let params = CombatParams {
            strength_a: 100.0,
            strength_b: 100.0,
            morale_a: 0.8,
            morale_b: 0.8,
            terrain_defense: 1.0,
            tech_modifier_a: 1.0,
            tech_modifier_b: 1.0,
            guerrilla_a: false,
            guerrilla_b: false,
            attrition_coeff: 0.01,
        };
        let model = AttritionModel::Stochastic { noise: 0.3 };

        let mut rng1 = ChaCha8Rng::seed_from_u64(1);
        let result1 = resolve_combat(&params, &model, &mut rng1);

        let mut rng2 = ChaCha8Rng::seed_from_u64(999);
        let result2 = resolve_combat(&params, &model, &mut rng2);

        // Different seeds should produce different attrition values.
        let diff_a = (result1.attrition_a - result2.attrition_a).abs();
        let diff_b = (result1.attrition_b - result2.attrition_b).abs();
        assert!(
            diff_a > 1e-10 || diff_b > 1e-10,
            "different RNG seeds should produce different attrition \
             values: diff_a={diff_a} diff_b={diff_b}"
        );
    }
}
