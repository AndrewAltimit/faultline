//! Behavioral coupling for `Diplomacy` stance.
//!
//! Epic D round-three item 1 (also closes R3-2 round-two item 2):
//! the engine now consumes runtime diplomatic stance for combat
//! targeting and AI decision-making. Before this module landed,
//! `Faction.diplomacy` was authored in scenarios but unread —
//! `tick::find_contested_regions` treated every co-located faction
//! as a combatant regardless of stance, and the AI's
//! `compute_enemy_presence` counted every non-self faction as a
//! threat.
//!
//! ## Semantics
//!
//! - **Combat layer**: combat between A and B is skipped only when
//!   both sides view each other as `Allied` (mutual alliance).
//!   Cooperative pairs still fight if their forces collide — the
//!   relationship is "we cooperate but aren't sworn allies", and
//!   accidental engagement is plausible. Neutral and below preserve
//!   legacy combat behavior, so scenarios that don't author
//!   diplomacy see no change.
//! - **AI layer**: an `Allied` faction is excluded entirely from
//!   the threat presence map and from attack scoring. A `Cooperative`
//!   neighbor is retained but de-rated (0.3× threat / priority
//!   multiplier) — the AI de-prioritizes friendly targets without
//!   refusing to defend against them outright.
//!
//! ## Determinism
//!
//! Every helper is a pure function of `SimulationState` and
//! `Scenario`. No RNG, no allocation. Reads go through
//! [`fracture::current_stance`] so post-fracture overrides are
//! respected automatically — the alliance-fracture mechanism that
//! previously only logged transitions now actually flips combat /
//! AI behavior at the tick the rule fires.
//!
//! ## Why these thresholds, not others
//!
//! `Allied` is the strongest pacifist tier in the `Diplomacy` enum
//! (`War < Hostile < Neutral < Cooperative < Allied`); only it
//! fully blocks combat. The asymmetry between combat (Allied-only)
//! and AI (Allied + Cooperative-derated) follows the round-three
//! item-1 spec verbatim: "combat targeting respects
//! `Diplomacy::Allied`, AI de-prioritizes Cooperative neighbors."

use faultline_types::faction::Diplomacy;
use faultline_types::ids::FactionId;
use faultline_types::scenario::Scenario;

use crate::fracture::current_stance;
use crate::state::SimulationState;

/// Cooperative threat / priority multiplier in the AI layer.
///
/// 0.3 is a deliberate de-prioritization (Cooperative neighbors are
/// scored at 30% of their raw threat) without zeroing them out —
/// "less likely target, but still on the list" — matching the
/// round-three spec's distinction between "respect Allied" (combat
/// blocking) and "de-prioritize Cooperative" (soft AI penalty).
pub const COOPERATIVE_AI_FACTOR: f64 = 0.3;

/// True iff combat between `a` and `b` should be skipped this tick.
///
/// Mutual alliance is required: if A views B as `Allied` but B views
/// A as anything else, combat happens. This matches the natural
/// reading — alliance is reciprocal; one-sided declarations don't
/// bind the other party.
///
/// Reads `current_stance` in both directions so runtime overrides
/// from `fracture_phase` or `EventEffect::DiplomacyChange` are
/// respected automatically.
pub fn combat_blocked(
    state: &SimulationState,
    scenario: &Scenario,
    a: &FactionId,
    b: &FactionId,
) -> bool {
    let stance_ab = current_stance(state, scenario, a, b);
    let stance_ba = current_stance(state, scenario, b, a);
    matches!(stance_ab, Diplomacy::Allied) && matches!(stance_ba, Diplomacy::Allied)
}

/// Multiplier the AI applies when sizing `other`'s contribution to
/// `self_id`'s perceived threat or attack-priority.
///
/// - `Allied`: 0.0 — fully ignored. The AI doesn't defend against
///   sworn allies and never targets them for attack.
/// - `Cooperative`: `COOPERATIVE_AI_FACTOR` (0.3) — partial
///   discounting. Defense weighting and attack scoring both fall to
///   30%, putting Cooperative neighbors at the bottom of the
///   target list without removing them.
/// - All other stances (Neutral, Hostile, War): 1.0 — unchanged
///   from legacy behavior.
///
/// This is from `self_id`'s perspective only — i.e. the AI uses the
/// stance it has *declared* toward `other`, not the symmetric pair.
/// A faction that thinks it's Allied with someone who is secretly
/// hostile will fail to defend against them; that asymmetry is the
/// intended signal in scenarios modeling miscalibrated diplomacy.
pub fn ai_threat_multiplier(
    state: &SimulationState,
    scenario: &Scenario,
    self_id: &FactionId,
    other: &FactionId,
) -> f64 {
    match current_stance(state, scenario, self_id, other) {
        Diplomacy::Allied => 0.0,
        Diplomacy::Cooperative => COOPERATIVE_AI_FACTOR,
        Diplomacy::War | Diplomacy::Hostile | Diplomacy::Neutral => 1.0,
    }
}

// Behavioral coverage of combat_blocked / ai_threat_multiplier with
// real scenario state lives in the integration suite at
// `crates/faultline-engine/tests/diplomacy_behavior.rs`. We don't
// duplicate it here; constructing a `SimulationState` standalone
// would require either widening its public API with `Default` or
// hand-filling 18 fields.
