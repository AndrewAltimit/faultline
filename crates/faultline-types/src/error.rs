use crate::ids::{
    DefenderRoleId, EventId, FactionId, InfraId, InstitutionId, KillChainId, PhaseId, RegionId,
    TechCardId, VictoryId,
};

/// Errors arising from scenario validation.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("duplicate faction id: {0}")]
    DuplicateFaction(FactionId),

    #[error("duplicate region id: {0}")]
    DuplicateRegion(RegionId),

    #[error("duplicate infrastructure id: {0}")]
    DuplicateInfra(InfraId),

    #[error("duplicate event id: {0}")]
    DuplicateEvent(EventId),

    #[error("duplicate victory condition id: {0}")]
    DuplicateVictory(VictoryId),

    #[error("unknown faction referenced: {0}")]
    UnknownFaction(FactionId),

    #[error("unknown region referenced: {0}")]
    UnknownRegion(RegionId),

    #[error("unknown infrastructure referenced: {0}")]
    UnknownInfra(InfraId),

    #[error("unknown tech card referenced: {0}")]
    UnknownTechCard(TechCardId),

    #[error("unknown event referenced: {0}")]
    UnknownEvent(EventId),

    #[error("unknown institution referenced: {0}")]
    UnknownInstitution(InstitutionId),

    #[error("region {region} borders non-existent region {neighbor}")]
    InvalidBorder {
        region: RegionId,
        neighbor: RegionId,
    },

    #[error("infrastructure {infra} references unknown region {region}")]
    InfraRegionMismatch { infra: InfraId, region: RegionId },

    #[error("force unit {force} in faction {faction} references unknown region {region}")]
    ForceRegionMismatch {
        force: String,
        faction: FactionId,
        region: RegionId,
    },

    #[error("value out of range for {field}: {value} (expected {expected})")]
    ValueOutOfRange {
        field: String,
        value: f64,
        expected: String,
    },

    #[error("empty scenario: {0}")]
    EmptyScenario(String),

    #[error("deserialization failed: {0}")]
    DeserializationError(String),

    #[error("event chain cycle detected starting at: {0}")]
    EventChainCycle(EventId),

    #[error("kill chain phase references unknown defender role: faction={faction} role={role}")]
    UnknownDefenderRole {
        faction: FactionId,
        role: DefenderRoleId,
    },

    #[error(
        "defender role {role} on faction {faction} has queue_depth = 0; \
         a zero-capacity queue is permanently saturated and silently \
         applies the saturated_detection_factor penalty before any noise \
         arrives"
    )]
    ZeroDefenderQueueDepth {
        faction: FactionId,
        role: DefenderRoleId,
    },

    #[error(
        "defender role table key {key} on faction {faction} does not match \
         its inner id field {id}"
    )]
    DefenderRoleIdMismatch {
        faction: FactionId,
        key: DefenderRoleId,
        id: DefenderRoleId,
    },

    #[error(
        "defender role {role} on faction {faction} has negative service_rate \
         {value}; service_rate must be >= 0 (a queue cannot drain at a \
         negative rate)"
    )]
    NegativeServiceRate {
        faction: FactionId,
        role: DefenderRoleId,
        value: f64,
    },

    #[error(
        "defender role {role} on faction {faction} has \
         saturated_detection_factor {value} outside [0.0, 1.0]; the factor \
         is a multiplier on detection probability — values < 0 or > 1 are \
         physically meaningless"
    )]
    SaturatedDetectionFactorOutOfRange {
        faction: FactionId,
        role: DefenderRoleId,
        value: f64,
    },

    #[error(
        "kill chain {chain} phase {phase} declares defender_noise with \
         items_per_tick = {value}; the inverse-transform Poisson sampler \
         used here loses precision for means above ~700 (exp(-mean) \
         underflows to 0 in f64). Cap at 700.0 or split across multiple \
         noise streams."
    )]
    DefenderNoiseRateTooHigh {
        chain: KillChainId,
        phase: PhaseId,
        value: f64,
    },

    #[error(
        "kill chain {chain} phase {phase} declares defender_noise with \
         items_per_tick = {value}; rate must be >= 0 (a queue cannot \
         receive a negative number of arrivals per tick)"
    )]
    NegativeDefenderNoiseRate {
        chain: KillChainId,
        phase: PhaseId,
        value: f64,
    },

    #[error(
        "kill chain {chain} phase {phase} declares an OrAny branch \
         condition with an empty `conditions` vector; an empty OR is \
         ambiguous (vacuously false vs. an unfilled author template) \
         and would silently never fire — supply at least one inner \
         condition or remove the branch"
    )]
    EmptyOrAnyBranch { chain: KillChainId, phase: PhaseId },

    #[error(
        "network {network} edge {edge} references unknown node {node}; \
         every edge endpoint must be a declared node in the same network"
    )]
    UnknownNetworkNode {
        network: crate::ids::NetworkId,
        edge: crate::ids::EdgeId,
        node: crate::ids::NodeId,
    },

    #[error(
        "network {network} declares edge {edge} as a self-loop ({node} -> {node}); \
         self-loops have no analytical use and are almost always an authoring typo"
    )]
    NetworkSelfLoop {
        network: crate::ids::NetworkId,
        edge: crate::ids::EdgeId,
        node: crate::ids::NodeId,
    },

    #[error(
        "{kind} table key {key} on network {network} does not match its inner id field {id}; \
         the engine reads only the key, so a mismatch would silently lose the inner-id value"
    )]
    NetworkIdMismatch {
        network: crate::ids::NetworkId,
        kind: &'static str,
        key: String,
        id: String,
    },

    #[error("event {event} effect {effect} references unknown network {network}")]
    UnknownNetwork {
        event: EventId,
        effect: String,
        network: crate::ids::NetworkId,
    },

    #[error(
        "event {event} effect {effect} on network {network} references unknown {kind} {target}"
    )]
    UnknownNetworkTarget {
        event: EventId,
        effect: String,
        network: crate::ids::NetworkId,
        kind: String,
        target: String,
    },

    #[error("{0}")]
    Custom(String),
}
