use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use faultline_types::events::{EventCondition, EventDefinition, EventEffect};
use faultline_types::ids::{EventId, FactionId, RegionId};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during event evaluation and firing.
#[derive(Debug, Error)]
pub enum EventError {
    #[error("event not found: {0}")]
    EventNotFound(EventId),

    #[error("invalid event definition: {0}")]
    InvalidDefinition(String),

    #[error("event chain cycle detected at: {0}")]
    ChainCycle(EventId),
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Minimal simulation state snapshot used to evaluate event conditions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimState {
    /// Current simulation tick.
    pub tick: u32,
    /// Global tension level in `[0.0, 1.0]`.
    pub tension: f64,
    /// Per-faction aggregate strength.
    pub faction_strengths: BTreeMap<FactionId, f64>,
    /// Per-faction morale.
    pub faction_morale: BTreeMap<FactionId, f64>,
    /// Which faction controls each region (if any).
    pub region_control: BTreeMap<RegionId, Option<FactionId>>,
    /// Set of event IDs that have already fired.
    pub fired_events: BTreeMap<EventId, bool>,
}

/// Holds event definitions and provides evaluation methods.
#[derive(Clone, Debug, Default)]
pub struct EventEvaluator {
    /// All known event definitions, keyed by their id.
    pub events: BTreeMap<EventId, EventDefinition>,
}

impl EventEvaluator {
    /// Create a new evaluator from a list of event definitions.
    pub fn new(definitions: Vec<EventDefinition>) -> Self {
        let events = definitions.into_iter().map(|e| (e.id.clone(), e)).collect();
        Self { events }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate whether all conditions for the given event are met in the
/// current simulation state.
///
/// Returns `true` only when every condition is satisfied.
pub fn evaluate_conditions(event: &EventDefinition, state: &SimState) -> bool {
    // Check tick window.
    if let Some(earliest) = event.earliest_tick
        && state.tick < earliest
    {
        return false;
    }
    if let Some(latest) = event.latest_tick
        && state.tick > latest
    {
        return false;
    }

    event.conditions.iter().all(|c| evaluate_single(c, state))
}

/// Fire an event, returning its effects if the probability check passes.
///
/// Uses the provided RNG to decide whether the event actually fires
/// based on its `probability` field. Returns `None` if the roll fails.
pub fn fire_event(event: &EventDefinition, rng: &mut impl rand::Rng) -> Option<Vec<EventEffect>> {
    let roll: f64 = rng.r#gen();
    if roll < event.probability {
        Some(event.effects.clone())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Evaluate a single event condition against the simulation state.
fn evaluate_single(condition: &EventCondition, state: &SimState) -> bool {
    match condition {
        EventCondition::TensionAbove { threshold } => state.tension > *threshold,
        EventCondition::TensionBelow { threshold } => state.tension < *threshold,
        EventCondition::FactionStrengthAbove { faction, threshold } => state
            .faction_strengths
            .get(faction)
            .is_some_and(|s| *s > *threshold),
        EventCondition::FactionStrengthBelow { faction, threshold } => state
            .faction_strengths
            .get(faction)
            .is_some_and(|s| *s < *threshold),
        EventCondition::MoraleAbove { faction, threshold } => state
            .faction_morale
            .get(faction)
            .is_some_and(|m| *m > *threshold),
        EventCondition::MoraleBelow { faction, threshold } => state
            .faction_morale
            .get(faction)
            .is_some_and(|m| *m < *threshold),
        EventCondition::RegionControl {
            region,
            faction,
            controlled,
        } => {
            let has_control = state
                .region_control
                .get(region)
                .and_then(|ctrl| ctrl.as_ref())
                .is_some_and(|f| f == faction);
            has_control == *controlled
        },
        EventCondition::EventFired { event, fired } => {
            let was_fired = state.fired_events.get(event).copied().unwrap_or(false);
            was_fired == *fired
        },
        EventCondition::TickAtLeast { tick: min_tick } => state.tick >= *min_tick,
        // Conditions that require data not in SimState default to
        // satisfied so they don't block events in the skeleton.
        EventCondition::InstitutionLoyaltyBelow { .. }
        | EventCondition::InfraStatusBelow { .. }
        | EventCondition::SegmentActivated { .. }
        | EventCondition::Expression { .. } => true,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use faultline_types::events::EventCondition;

    fn sample_event() -> EventDefinition {
        EventDefinition {
            id: EventId::from("uprising-01"),
            name: "Urban Uprising".into(),
            description: "Civilians rise up in the capital".into(),
            earliest_tick: Some(5),
            latest_tick: Some(50),
            conditions: vec![
                EventCondition::TensionAbove { threshold: 0.7 },
                EventCondition::TickAtLeast { tick: 10 },
            ],
            probability: 1.0,
            repeatable: false,
            effects: vec![EventEffect::TensionShift { delta: 0.1 }],
            chain: None,
        }
    }

    fn sample_state() -> SimState {
        SimState {
            tick: 15,
            tension: 0.8,
            faction_strengths: BTreeMap::new(),
            faction_morale: BTreeMap::new(),
            region_control: BTreeMap::new(),
            fired_events: BTreeMap::new(),
        }
    }

    #[test]
    fn conditions_pass_when_met() {
        let event = sample_event();
        let state = sample_state();
        assert!(evaluate_conditions(&event, &state));
    }

    #[test]
    fn conditions_fail_when_tension_low() {
        let event = sample_event();
        let mut state = sample_state();
        state.tension = 0.3;
        assert!(!evaluate_conditions(&event, &state));
    }

    #[test]
    fn conditions_fail_before_earliest_tick() {
        let event = sample_event();
        let mut state = sample_state();
        state.tick = 2;
        assert!(!evaluate_conditions(&event, &state));
    }

    #[test]
    fn fire_event_returns_effects_on_success() {
        let event = sample_event(); // probability = 1.0
        let mut rng = rand::thread_rng();
        let result = fire_event(&event, &mut rng);
        assert!(result.is_some());
        let effects = result.expect("just checked is_some");
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn fire_event_returns_none_on_zero_probability() {
        let mut event = sample_event();
        event.probability = 0.0;
        let mut rng = rand::thread_rng();
        let result = fire_event(&event, &mut rng);
        assert!(result.is_none());
    }

    #[test]
    fn evaluator_stores_definitions() {
        let evaluator = EventEvaluator::new(vec![sample_event()]);
        assert_eq!(evaluator.events.len(), 1);
        assert!(evaluator.events.contains_key(&EventId::from("uprising-01")));
    }

    #[test]
    fn condition_region_control_true() {
        let region = RegionId::from("capital");
        let faction = FactionId::from("gov");
        let event = EventDefinition {
            id: EventId::from("ctrl-test"),
            name: "Control Test".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::RegionControl {
                region: region.clone(),
                faction: faction.clone(),
                controlled: true,
            }],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
        };
        let mut state = sample_state();
        state.region_control.insert(region, Some(faction));
        assert!(
            evaluate_conditions(&event, &state),
            "condition should pass when faction controls the region"
        );
    }

    #[test]
    fn condition_region_control_false() {
        let region = RegionId::from("capital");
        let faction = FactionId::from("gov");
        let event = EventDefinition {
            id: EventId::from("ctrl-test-2"),
            name: "Control Test Negative".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::RegionControl {
                region: region.clone(),
                faction: faction.clone(),
                controlled: true,
            }],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
        };
        let mut state = sample_state();
        // Different faction controls the region.
        state
            .region_control
            .insert(region, Some(FactionId::from("rebel")));
        assert!(
            !evaluate_conditions(&event, &state),
            "condition should fail when a different faction controls"
        );
    }

    #[test]
    fn condition_faction_strength_above() {
        let faction = FactionId::from("mil");
        let event = EventDefinition {
            id: EventId::from("str-test"),
            name: "Strength Test".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::FactionStrengthAbove {
                faction: faction.clone(),
                threshold: 50.0,
            }],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
        };
        let mut state = sample_state();
        state.faction_strengths.insert(faction.clone(), 75.0);
        assert!(
            evaluate_conditions(&event, &state),
            "should pass when strength is above threshold"
        );

        state.faction_strengths.insert(faction, 30.0);
        assert!(
            !evaluate_conditions(&event, &state),
            "should fail when strength is below threshold"
        );
    }

    #[test]
    fn condition_morale_below() {
        let faction = FactionId::from("rebels");
        let event = EventDefinition {
            id: EventId::from("morale-test"),
            name: "Morale Test".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::MoraleBelow {
                faction: faction.clone(),
                threshold: 0.4,
            }],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
        };
        let mut state = sample_state();
        state.faction_morale.insert(faction.clone(), 0.2);
        assert!(
            evaluate_conditions(&event, &state),
            "should pass when morale is below threshold"
        );

        state.faction_morale.insert(faction, 0.6);
        assert!(
            !evaluate_conditions(&event, &state),
            "should fail when morale is above threshold"
        );
    }

    #[test]
    fn condition_event_fired() {
        let prior = EventId::from("prior-event");
        let event = EventDefinition {
            id: EventId::from("chain-test"),
            name: "Chain Test".into(),
            description: String::new(),
            earliest_tick: None,
            latest_tick: None,
            conditions: vec![EventCondition::EventFired {
                event: prior.clone(),
                fired: true,
            }],
            probability: 1.0,
            repeatable: false,
            effects: vec![],
            chain: None,
        };

        // Event was not fired.
        let state_not_fired = sample_state();
        assert!(
            !evaluate_conditions(&event, &state_not_fired),
            "should fail when required event has not fired"
        );

        // Event was fired.
        let mut state_fired = sample_state();
        state_fired.fired_events.insert(prior, true);
        assert!(
            evaluate_conditions(&event, &state_fired),
            "should pass when required event has fired"
        );
    }

    #[test]
    fn conditions_fail_after_latest_tick() {
        let event = sample_event(); // latest_tick = Some(50)
        let mut state = sample_state();
        state.tick = 51; // past latest
        state.tension = 0.9; // would otherwise pass
        assert!(
            !evaluate_conditions(&event, &state),
            "conditions should fail when tick exceeds latest_tick"
        );
    }
}
