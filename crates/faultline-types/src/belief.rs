//! Belief asymmetry primitives (Epic M round-one).
//!
//! This module defines the schema for *what each faction believes*
//! about the world, separate from what is *actually true*. Round-one
//! ships the data structures, the deception-event payload variants,
//! and the [`BeliefModelConfig`] opt-in toggle. The engine wiring
//! that consumes these types lives in `faultline-engine::belief`.
//!
//! ## Why a separate "belief" axis?
//!
//! Pre-Epic-M Faultline modelled fog of war via [`FactionWorldView`](
//! crate::strategy::FactionWorldView), which is rebuilt fresh each
//! tick from current ground truth filtered by visibility. That shape
//! is fine for "what can I see right now?", but it can't represent:
//!
//! - **Stale data.** A faction that saw an enemy force last tick but
//!   not this tick should retain the prior observation with reduced
//!   confidence rather than forgetting it instantly.
//! - **Deception.** False-flag operations, planted intel, and
//!   misdirection campaigns all change *what an opponent believes*
//!   without changing what is true. The legacy fog path has no
//!   handle for this — anything that mutates ground truth is
//!   visible to *every* faction's fog filter.
//! - **Asymmetric information.** Two factions can hold contradictory
//!   beliefs about the same fact (one is misled, one is correct);
//!   a fresh ground-truth-derived view collapses both to the same
//!   filtered view of truth.
//!
//! Round-one ships persistent [`FactionBelief`] state with per-fact
//! confidence + last-observed timestamps, and adds two new
//! [`crate::events::EventEffect`] variants (`DeceptionOp` and
//! `IntelligenceShare`) for direct manipulation. Round-two will add
//! Bayesian belief updating from indirect signals (intelligence
//! gathering, captured prisoners, tech-card surveillance) and pair
//! with Epic J round-two (utility scoring against believed state).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{FactionId, ForceId, RegionId};

/// Provenance of a belief entry.
///
/// Round-one consumers (the report layer, the validation gate, the
/// belief-accuracy aggregator) inspect this to distinguish between
/// "the faction saw this directly", "the faction saw this *N ticks
/// ago* and the entry has decayed", and "an enemy planted this".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BeliefSource {
    /// Direct observation this tick or initial visibility-derived state.
    #[default]
    DirectObservation,
    /// Observation aged from a prior direct observation. Confidence
    /// has decayed; the underlying value still reflects what was last
    /// seen but may not match current ground truth.
    Stale,
    /// Inferred from indirect signals (round-two; not produced by the
    /// round-one engine but reserved on the wire so the schema can
    /// extend without breaking).
    Inferred,
    /// Injected by an [`crate::events::EventEffect::DeceptionOp`]. The
    /// believing faction has no way to distinguish these from
    /// `DirectObservation` from inside the simulation; the engine
    /// preserves the source tag so the cross-run analyst report can
    /// surface "how often did the faction act on deceived intel?".
    Deceived,
}

/// A scalar belief about a single observable quantity (morale,
/// resources, tension), with confidence and provenance.
///
/// `value` is the believed magnitude. `confidence` is in `[0, 1]` —
/// fresh direct observations sit at `1.0` and decay each tick the
/// believer goes without re-observing the underlying truth.
/// `last_observed_tick` is the tick index of the most recent
/// observation (used by the decay function).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeliefScalar {
    pub value: f64,
    pub confidence: f64,
    pub last_observed_tick: u32,
    #[serde(default)]
    pub source: BeliefSource,
}

impl BeliefScalar {
    /// Construct a fresh direct-observation belief.
    pub fn fresh(value: f64, tick: u32) -> Self {
        Self {
            value,
            confidence: 1.0,
            last_observed_tick: tick,
            source: BeliefSource::DirectObservation,
        }
    }
}

/// A belief about an opposing force unit's location and strength.
///
/// `region` is where the believer thinks the force *is*; this can be
/// stale (if the force has moved since the last observation) or
/// outright false (if a deception event planted it). `confidence`
/// tracks observation freshness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeliefForce {
    pub force: ForceId,
    pub owner: FactionId,
    pub region: RegionId,
    pub estimated_strength: f64,
    pub confidence: f64,
    pub last_observed_tick: u32,
    #[serde(default)]
    pub source: BeliefSource,
}

/// A belief about a region's controlling faction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeliefRegion {
    pub controller: Option<FactionId>,
    pub confidence: f64,
    pub last_observed_tick: u32,
    #[serde(default)]
    pub source: BeliefSource,
}

/// Persistent per-faction belief state.
///
/// One [`FactionBelief`] exists per faction in the simulation when
/// [`BeliefModelConfig::enabled`] is `true`. Each tick the engine's
/// `belief_phase` updates these maps from current observations
/// (refreshing seen entries) and decays unseen entries. Deception
/// events (Epic M) inject false entries that look identical to
/// `DirectObservation` from inside the AI's perspective but are
/// tagged `Deceived` for post-run analytics.
///
/// All maps are keyed deterministically (`BTreeMap`). Empty maps
/// are valid — round-one initializes the state at `tick = 0` with
/// whatever is visible at that tick; later ticks may grow / shrink
/// the maps as observations come and go.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FactionBelief {
    /// Believer's own faction id (denormalised for renderer
    /// convenience).
    pub faction: FactionId,
    /// Per-region control beliefs, keyed by `RegionId`. Only regions
    /// that have ever been visible to the believer appear here.
    #[serde(default)]
    pub regions: BTreeMap<RegionId, BeliefRegion>,
    /// Per-force beliefs, keyed by `ForceId`. Forces that have left
    /// visibility decay rather than disappear from the map — the
    /// believer still thinks the force exists somewhere until enough
    /// time has passed to drop confidence below the prune threshold.
    #[serde(default)]
    pub forces: BTreeMap<ForceId, BeliefForce>,
    /// Per-faction morale beliefs. Keyed by the *target* faction
    /// (whose morale the believer holds an opinion of).
    #[serde(default)]
    pub faction_morale: BTreeMap<FactionId, BeliefScalar>,
    /// Per-faction resource beliefs. Same keying as `faction_morale`.
    #[serde(default)]
    pub faction_resources: BTreeMap<FactionId, BeliefScalar>,
    /// Tick of the most recent belief-phase update for this faction.
    /// Used by the decay function to compute the elapsed gap; also
    /// surfaced in snapshots so the analyst can see "when was this
    /// faction last refreshed?".
    #[serde(default)]
    pub last_updated_tick: u32,
    /// Cumulative count of `DeceptionOp` events this faction has been
    /// the target of across the run. Surfaced in the cross-run
    /// belief-asymmetry report.
    #[serde(default)]
    pub deception_events_received: u32,
}

/// Opt-in configuration for the belief-asymmetry mechanic.
///
/// Round-one defaults to disabled — every legacy scenario behaves
/// bit-identically to pre-Epic-M because the engine short-circuits
/// the belief phase entirely when `belief_model` is `None` or
/// `enabled = false`. Once enabled, the per-tick decay rates govern
/// how fast confidence falls off when an observation isn't
/// refreshed.
///
/// All decay rates are in `[0, 1]`, applied as
/// `confidence_next = confidence_current × (1 - decay_per_tick)`.
/// Setting a rate to `0.0` means the belief never decays from age
/// alone (only direct refresh / deception updates change it);
/// setting it to `1.0` means an unrefreshed belief drops to zero
/// confidence in one tick (functionally equivalent to disabling
/// persistence on that axis).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeliefModelConfig {
    /// Master toggle. When `false`, the engine short-circuits the
    /// belief phase and the AI stays on the legacy ground-truth
    /// path. Default `false` so adding the field to a scenario
    /// without enabling it is a no-op.
    #[serde(default)]
    pub enabled: bool,
    /// Per-tick confidence decay for [`BeliefForce`] entries that
    /// were not directly re-observed this tick.
    #[serde(default = "default_force_decay")]
    pub force_decay_per_tick: f64,
    /// Per-tick confidence decay for [`BeliefRegion`] entries.
    #[serde(default = "default_region_decay")]
    pub region_decay_per_tick: f64,
    /// Per-tick confidence decay for [`BeliefScalar`] entries
    /// (faction morale / resources).
    #[serde(default = "default_scalar_decay")]
    pub scalar_decay_per_tick: f64,
    /// Belief entries whose confidence falls strictly below this
    /// threshold are pruned from the persistent state. Defaults to
    /// `0.05` so a force unobserved for ~30 ticks at the default
    /// decay rate (5%) drops out of the believer's awareness. Set
    /// to `0.0` to never prune (entries persist indefinitely with
    /// vanishing confidence).
    #[serde(default = "default_prune_threshold")]
    pub prune_threshold: f64,
    /// Per-tick belief-snapshot capture interval. `0` means never
    /// capture (default — the snapshot stream only matters for
    /// belief-trace replay tooling, not for cross-run analytics).
    /// When set to N > 0, the engine appends one snapshot per
    /// faction every N ticks plus one terminal snapshot at run end.
    #[serde(default)]
    pub snapshot_interval: u32,
    /// Round-two: enable intelligence-weighted observation fidelity.
    ///
    /// When `false` (default — preserves round-one behavior
    /// bit-identically), every direct observation overwrites the
    /// belief at `confidence = 1.0` and `source = DirectObservation`,
    /// so a faction's belief about a *visible* opponent is always
    /// exactly ground truth.
    ///
    /// When `true`, the per-tick observation step instead:
    /// - caps observation confidence below `1.0` by an amount driven
    ///   by the observer's [`crate::faction::Faction::intelligence`]
    ///   (a high-intelligence faction observes near-perfectly; a
    ///   low-intelligence faction observes at a confidence floor),
    /// - performs a precision-weighted Bayesian blend of the new
    ///   observation against the prior belief rather than overwriting,
    ///   so a low-confidence observer's belief *lags* a changing
    ///   ground truth (the analytical payoff: belief error becomes a
    ///   function of intelligence, not just observation recency), and
    /// - tags blended entries [`BeliefSource::Inferred`] so the
    ///   cross-run report can distinguish "perfectly observed" from
    ///   "estimated under uncertainty".
    ///
    /// Own-faction facts (own forces, own controlled regions, own
    /// morale / resources) stay at `confidence = 1.0` —
    /// a faction always knows its own posture exactly. Deception
    /// entries erode toward truth as real observations blend in,
    /// modelling "the lie is being corrected by reality".
    #[serde(default)]
    pub intelligence_weighting: bool,
    /// Round-two: route kill-chain attribution through the defender's
    /// *belief* rather than ground truth.
    ///
    /// When `false` (default — preserves the legacy attribution path
    /// bit-identically), a phase that the defender detects is always
    /// attributed to the chain's true `attacker`, and the
    /// alliance-fracture accounting
    /// (`FractureCondition::AttributionThreshold`) reads that
    /// ground-truth attacker.
    ///
    /// When `true` (requires `enabled = true`), at the moment of
    /// detection the defender (the chain's `target`) draws a *believed
    /// attributed faction* from a weighted distribution: the true
    /// attacker is favoured in proportion to the defender's
    /// `intelligence`, every other candidate faction carries a small
    /// residual confusion mass, and any faction the defender already
    /// holds a [`BeliefSource::Deceived`] belief about (a planted
    /// false-flag region-control or force belief implicating it) is
    /// boosted. A low-intelligence or deceived defender can therefore
    /// misattribute the attack and then *act on the misattribution* —
    /// the fracture accounting credits the attribution confidence to
    /// the *believed* faction, so the defender can fracture from an
    /// innocent ally. The draw consumes one engine-RNG value per
    /// detection; it is gated entirely behind this flag, so scenarios
    /// that leave it `false` consume the RNG in the exact legacy order
    /// and remain bit-identical.
    #[serde(default)]
    pub believed_attribution: bool,
}

impl Default for BeliefModelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            force_decay_per_tick: default_force_decay(),
            region_decay_per_tick: default_region_decay(),
            scalar_decay_per_tick: default_scalar_decay(),
            prune_threshold: default_prune_threshold(),
            snapshot_interval: 0,
            intelligence_weighting: false,
            believed_attribution: false,
        }
    }
}

/// Observation confidence ceiling for a foreign fact given the
/// observer's `intelligence` stat (round-two intelligence weighting).
///
/// Maps `intelligence ∈ [0, 1]` to a confidence in `[0.3, 0.95]` —
/// even a maximally-capable observer never reaches `1.0` confidence
/// about an *opponent's* hidden state (the irreducible fog of war),
/// while a minimally-capable one still gleans something (a `0.3`
/// floor). Mirrors the legacy `build_world_view` mapping
/// (`intelligence * 0.6 + 0.2`) but with a higher floor and ceiling
/// because the persistent belief blends across ticks, so the
/// per-observation confidence should be a touch more generous.
///
/// Pure, total, deterministic; `NaN`/out-of-range `intelligence`
/// clamps into range (validation already constrains the field, this
/// is defensive).
pub fn observation_confidence(intelligence: f64) -> f64 {
    let intel = if intelligence.is_finite() {
        intelligence.clamp(0.0, 1.0)
    } else {
        0.5
    };
    (0.3 + 0.65 * intel).clamp(0.3, 0.95)
}

/// Relative weight the defender's believed-attribution distribution
/// places on the *true* attacker, given the defender's `intelligence`
/// (round-two believed-attribution rolls).
///
/// Maps `intelligence ∈ [0, 1]` to a weight in `[0.5, 0.98]`. Even a
/// maximally-capable defender retains a small chance of confusion
/// (irreducible attribution fog), while a minimally-capable one still
/// fingers the right attacker more often than not (a `0.5` floor)
/// *before* any false-flag deception is layered on top — deception is
/// what pushes a low-intelligence defender past the tipping point into
/// majority-misattribution. The complement `1 - weight` is split as
/// residual "confusion" mass across the other candidate factions.
///
/// Pure, total, deterministic; `NaN` / out-of-range `intelligence`
/// clamps into range (validation already constrains the field, this is
/// defensive). Mirrors the shape of [`observation_confidence`].
pub fn attribution_true_weight(intelligence: f64) -> f64 {
    let intel = if intelligence.is_finite() {
        intelligence.clamp(0.0, 1.0)
    } else {
        0.5
    };
    (0.5 + 0.48 * intel).clamp(0.5, 0.98)
}

fn default_force_decay() -> f64 {
    0.05
}

fn default_region_decay() -> f64 {
    0.02
}

fn default_scalar_decay() -> f64 {
    0.03
}

fn default_prune_threshold() -> f64 {
    0.05
}

/// Payload for an [`crate::events::EventEffect::DeceptionOp`] —
/// describes the false fact the source faction is planting in the
/// target faction's belief state.
///
/// Round-one ships four payload variants covering the most common
/// deception archetypes: false force-strength estimate, false
/// region control attribution, false morale read, false resource
/// read. Round-two will add stand-alone fabricated forces (no
/// underlying real force) and fabricated narratives that propagate
/// through the existing narrative store.
///
/// The `source` and `target` are carried on the parent event-effect
/// variant; this enum carries only the deceptive-content payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DeceptionPayload {
    /// Plant a false strength estimate for an existing force. The
    /// target's belief overlay flips to `confidence = 1.0` (the
    /// deception is seamless) but the [`BeliefForce::source`] is
    /// tagged `Deceived` for cross-run analytics. The force must
    /// belong to a known faction at scenario load.
    FalseForceStrength {
        force: ForceId,
        owner: FactionId,
        region: RegionId,
        false_strength: f64,
    },
    /// Plant a false controller for a region.
    FalseRegionControl {
        region: RegionId,
        false_controller: Option<FactionId>,
    },
    /// Plant a false morale belief about a faction.
    FalseFactionMorale {
        faction: FactionId,
        false_morale: f64,
    },
    /// Plant a false resource belief about a faction.
    FalseFactionResources {
        faction: FactionId,
        false_resources: f64,
    },
}

/// Payload for [`crate::events::EventEffect::IntelligenceShare`] —
/// truthful information transfer (not deception). The source and
/// target are on the parent variant; this payload describes what
/// piece of ground truth gets immediately materialized in the
/// target's belief at full confidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum IntelligencePayload {
    /// Transfer belief about a specific force unit. The target's
    /// belief is overwritten with the *current ground-truth* state
    /// of the force (location, strength) at full confidence. If the
    /// target later loses sight of the force, normal decay applies.
    ForceObservation { force: ForceId },
    /// Transfer belief about a region's current controller.
    RegionControl { region: RegionId },
    /// Transfer belief about a faction's current morale.
    FactionMorale { faction: FactionId },
    /// Transfer belief about a faction's current resources.
    FactionResources { faction: FactionId },
}

/// Validate a [`BeliefModelConfig`].
///
/// Returns `Err(reason)` if any field is non-finite or outside
/// `[0, 1]` (decay rates) / `[0, 1]` (prune threshold). Pure
/// function — no I/O, no allocation. Called from
/// `faultline_engine::validate` at scenario load.
pub fn validate_belief_model(config: &BeliefModelConfig) -> Result<(), String> {
    fn check_unit(value: f64, name: &str) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!("belief_model.{name} must be finite, got {value}"));
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(format!(
                "belief_model.{name} must be in [0, 1], got {value}"
            ));
        }
        Ok(())
    }
    check_unit(config.force_decay_per_tick, "force_decay_per_tick")?;
    check_unit(config.region_decay_per_tick, "region_decay_per_tick")?;
    check_unit(config.scalar_decay_per_tick, "scalar_decay_per_tick")?;
    check_unit(config.prune_threshold, "prune_threshold")?;
    // Cross-field guards: round-two sub-flags are silent no-ops unless
    // the master toggle is on, so reject the "authored but inert" shape
    // at load rather than letting it pass and confusing the analyst.
    if config.intelligence_weighting && !config.enabled {
        return Err("belief_model.intelligence_weighting = true requires \
             belief_model.enabled = true (it is a silent no-op otherwise)"
            .to_string());
    }
    if config.believed_attribution && !config.enabled {
        return Err("belief_model.believed_attribution = true requires \
             belief_model.enabled = true (it is a silent no-op otherwise)"
            .to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_disabled() {
        let cfg = BeliefModelConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.force_decay_per_tick, 0.05);
        assert_eq!(cfg.region_decay_per_tick, 0.02);
        assert_eq!(cfg.scalar_decay_per_tick, 0.03);
        assert_eq!(cfg.prune_threshold, 0.05);
        assert_eq!(cfg.snapshot_interval, 0);
        assert!(!cfg.intelligence_weighting);
    }

    #[test]
    fn observation_confidence_monotone_and_bounded() {
        let low = observation_confidence(0.0);
        let mid = observation_confidence(0.5);
        let high = observation_confidence(1.0);
        assert!((low - 0.3).abs() < 1e-9, "floor at 0.3, got {low}");
        assert!((high - 0.95).abs() < 1e-9, "ceiling at 0.95, got {high}");
        assert!(low < mid && mid < high, "monotone increasing in intel");
        // Out-of-range / non-finite clamps defensively.
        assert_eq!(observation_confidence(2.0), 0.95);
        assert_eq!(observation_confidence(-1.0), 0.3);
        assert!(observation_confidence(f64::NAN).is_finite());
    }

    #[test]
    fn attribution_true_weight_monotone_and_bounded() {
        let low = attribution_true_weight(0.0);
        let mid = attribution_true_weight(0.5);
        let high = attribution_true_weight(1.0);
        assert!((low - 0.5).abs() < 1e-9, "floor at 0.5, got {low}");
        assert!((high - 0.98).abs() < 1e-9, "ceiling at 0.98, got {high}");
        assert!(low < mid && mid < high, "monotone increasing in intel");
        // Out-of-range / non-finite clamps defensively.
        assert_eq!(attribution_true_weight(2.0), 0.98);
        assert_eq!(attribution_true_weight(-1.0), 0.5);
        assert!(attribution_true_weight(f64::NAN).is_finite());
    }

    #[test]
    fn validate_rejects_believed_attribution_without_enabled() {
        let cfg = BeliefModelConfig {
            enabled: false,
            believed_attribution: true,
            ..Default::default()
        };
        let err = validate_belief_model(&cfg).expect_err("must be rejected");
        assert!(
            err.contains("believed_attribution") && err.contains("enabled"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_intelligence_weighting_without_enabled() {
        let cfg = BeliefModelConfig {
            enabled: false,
            intelligence_weighting: true,
            ..Default::default()
        };
        assert!(validate_belief_model(&cfg).is_err());
    }

    #[test]
    fn validate_accepts_believed_attribution_with_enabled() {
        let cfg = BeliefModelConfig {
            enabled: true,
            believed_attribution: true,
            ..Default::default()
        };
        validate_belief_model(&cfg).expect("enabled + believed_attribution is valid");
    }

    #[test]
    fn fresh_belief_scalar() {
        let s = BeliefScalar::fresh(0.7, 5);
        assert_eq!(s.value, 0.7);
        assert_eq!(s.confidence, 1.0);
        assert_eq!(s.last_observed_tick, 5);
        assert_eq!(s.source, BeliefSource::DirectObservation);
    }

    #[test]
    fn validate_belief_model_accepts_defaults() {
        validate_belief_model(&BeliefModelConfig::default()).expect("defaults are valid");
    }

    #[test]
    fn validate_belief_model_rejects_negative_decay() {
        let cfg = BeliefModelConfig {
            force_decay_per_tick: -0.1,
            ..Default::default()
        };
        assert!(validate_belief_model(&cfg).is_err());
    }

    #[test]
    fn validate_belief_model_rejects_decay_above_one() {
        let cfg = BeliefModelConfig {
            region_decay_per_tick: 1.5,
            ..Default::default()
        };
        assert!(validate_belief_model(&cfg).is_err());
    }

    #[test]
    fn validate_belief_model_rejects_nan() {
        let cfg = BeliefModelConfig {
            scalar_decay_per_tick: f64::NAN,
            ..Default::default()
        };
        assert!(validate_belief_model(&cfg).is_err());
    }

    #[test]
    fn validate_belief_model_rejects_negative_prune_threshold() {
        let cfg = BeliefModelConfig {
            prune_threshold: -0.01,
            ..Default::default()
        };
        assert!(validate_belief_model(&cfg).is_err());
    }

    #[test]
    fn deception_payload_serde_roundtrip() {
        let payload = DeceptionPayload::FalseForceStrength {
            force: ForceId::from("redforce_1"),
            owner: FactionId::from("red"),
            region: RegionId::from("frontier"),
            false_strength: 250.0,
        };
        let s = serde_json::to_string(&payload).expect("serialize");
        let back: DeceptionPayload = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(payload, back);
    }

    #[test]
    fn intelligence_payload_serde_roundtrip() {
        let payload = IntelligencePayload::FactionMorale {
            faction: FactionId::from("blue"),
        };
        let s = serde_json::to_string(&payload).expect("serialize");
        let back: IntelligencePayload = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(payload, back);
    }
}
