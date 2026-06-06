/**
 * Schema-aware field documentation for the TOML scenario editor.
 *
 * This module is the data + pure lookup logic behind the editor's hover
 * documentation feature. When the user hovers / focuses a scenario field
 * key in the editor textarea, the editor resolves the key under the
 * cursor (using the surrounding `[section]` header for disambiguation)
 * and renders the matching {@link FieldDoc} in a tooltip.
 *
 * The catalog is curated from `docs/scenario_schema.md` — the authoritative
 * human-readable reference, which in turn mirrors the doc comments on the
 * Rust types in `crates/faultline-types/src/`. It is kept in plain JS (not
 * shipped through a WASM export) for three reasons:
 *   1. The docs are static prose, not derived from a live `Scenario` value,
 *      so a WASM round-trip would add nothing.
 *   2. The lookup logic stays unit-testable headlessly under `node --test`,
 *      matching the existing `explain-panel.js` pattern.
 *   3. No Rust / WASM rebuild is required to extend or correct a doc entry.
 *
 * @typedef {object} FieldDoc
 * @property {string}  summary       One-or-two sentence plain-language meaning.
 * @property {string}  type          The TOML/Rust type (e.g. "f64", "string", "u32", "bool", "enum").
 * @property {string} [default]      Default when the field is omitted, if any.
 * @property {string} [range]        Valid range / accepted variants, if constrained.
 * @property {boolean} engineEffect  True if the field changes simulation behavior;
 *                                   false for purely descriptive / reporting metadata.
 * @property {string} [section]      The `[section]` (or section family) this key belongs to.
 *                                   Used to disambiguate identically-named keys.
 */

/**
 * The field-documentation catalog.
 *
 * Keyed by leaf field name. A handful of leaf names recur in more than one
 * section with different meanings (`id`, `name`, `description`, `threshold`,
 * `faction`, `region`, `delta`, `factor`, `effectiveness`, `confidence`,
 * `type`, `kind`); those are stored as an array of section-qualified
 * variants and {@link lookupFieldDoc} picks the best match using the
 * `[section]` header context. Single-meaning keys are stored as a lone
 * {@link FieldDoc} object.
 *
 * @type {Record<string, FieldDoc | FieldDoc[]>}
 */
export const FIELD_DOCS = {
  // ── scenario-level budgets ──────────────────────────────────────────
  attacker_budget: {
    summary:
      'Caps total attacker dollar spend across all kill-chain phases. A phase whose cost would push cumulative spend past this cap cannot activate and is marked Failed.',
    type: 'f64',
    section: '(top-level)',
    engineEffect: true,
  },
  defender_budget: {
    summary:
      'Caps total defender dollar spend across all kill chains. Once exceeded, a sticky 0.5× detection-probability multiplier applies to every subsequent phase for the rest of the run.',
    type: 'f64',
    section: '(top-level)',
    engineEffect: true,
  },

  // ── [meta] ──────────────────────────────────────────────────────────
  schema_version: {
    summary:
      'Faultline schema version this scenario was authored against. Drives the in-memory migration framework on load.',
    type: 'u32',
    default: '1',
    section: '[meta]',
    engineEffect: false,
  },
  author: {
    summary: 'Scenario author handle. Descriptive only.',
    type: 'string',
    section: '[meta]',
    engineEffect: false,
  },
  version: {
    summary:
      "Semver-style version for the scenario content itself, distinct from meta.schema_version (the format version).",
    type: 'string',
    section: '[meta]',
    engineEffect: false,
  },
  tags: {
    summary: 'Free-form tags for indexing. Descriptive only.',
    type: '[string]',
    section: '[meta]',
    engineEffect: false,
  },
  period: {
    summary: 'Free-form date or date-range label for a historical analogue (e.g. "2008-08-07 to 2008-08-12").',
    type: 'string',
    section: '[meta.historical_analogue]',
    engineEffect: false,
  },
  sources: {
    summary:
      'Open-source (OSINT) citations supporting a historical analogue. An empty list is rejected at load — an analogue without sources is a back-test against the author\'s recollection.',
    type: '[string]',
    section: '[meta.historical_analogue]',
    engineEffect: false,
  },
  observations: {
    summary:
      'One or more documented observations a historical analogue back-tests the Monte Carlo distribution against. Empty list is rejected at load.',
    type: '[Observation]',
    section: '[meta.historical_analogue]',
    engineEffect: false,
  },
  metric: [
    {
      summary:
        'What was historically observed for a calibration observation. Tagged enum: winner / win_rate / duration_ticks.',
      type: 'tagged enum',
      section: '[meta.historical_analogue]',
      engineEffect: false,
    },
    {
      summary:
        'Global metric an escalation/victory branch reads: Tension, InformationDominance, InstitutionalErosion, CoercionPressure, or PoliticalCost.',
      type: 'enum',
      section: '[kill_chains]',
      engineEffect: true,
    },
  ],

  // ── [map] ───────────────────────────────────────────────────────────
  population: {
    summary: 'Civilian population of a region. Feeds segment activation and displacement accounting.',
    type: 'u64',
    section: '[map.regions]',
    engineEffect: true,
  },
  urbanization: {
    summary: 'Fraction of the region that is urban.',
    type: 'f64',
    range: '[0, 1]',
    section: '[map.regions]',
    engineEffect: true,
  },
  initial_control: {
    summary: 'Faction id that starts controlling this region. Omit for neutral / uncontrolled.',
    type: 'string?',
    section: '[map.regions]',
    engineEffect: true,
  },
  strategic_value: {
    summary: 'Weight for AI target selection and victory checks — how much winning this region matters.',
    type: 'f64',
    range: '[0, 1]',
    section: '[map.regions]',
    engineEffect: true,
  },
  borders: {
    summary: 'Ids of regions adjacent to this one. Defines the movement / combat adjacency graph.',
    type: '[string]',
    section: '[map.regions]',
    engineEffect: true,
  },
  centroid: {
    summary: 'Optional geographic centroid { lat, lon } used for map rendering.',
    type: '{lat, lon}?',
    section: '[map.regions]',
    engineEffect: false,
  },
  infra_type: {
    summary:
      'Infrastructure category: PowerGrid, Telecommunications, TransportHub, GovernmentBuilding, MediaStation, WaterSystem, FuelDepot, Hospital, SupplyChain, Internet.',
    type: 'enum',
    section: '[map.infrastructure]',
    engineEffect: true,
  },
  criticality: [
    {
      summary: 'Weight for damage scoring — how painful losing this infrastructure node is.',
      type: 'f64',
      range: '[0, 1]',
      section: '[map.infrastructure]',
      engineEffect: true,
    },
    {
      summary: 'Author-supplied importance multiplier for a network node, surfaced alongside betweenness in critical-node ranking.',
      type: 'f64',
      range: '[0, 1]',
      section: '[networks]',
      engineEffect: true,
    },
  ],
  initial_status: {
    summary: 'Starting health of an infrastructure node (1.0 = fully intact).',
    type: 'f64',
    range: '[0, 1]',
    section: '[map.infrastructure]',
    engineEffect: true,
  },
  repairable: {
    summary: 'Ticks required to repair this node. Omit for permanent (unrepairable) damage.',
    type: 'u32?',
    section: '[map.infrastructure]',
    engineEffect: true,
  },
  terrain_type: {
    summary:
      'Terrain class: Urban, Suburban, Rural, Forest, Mountain, Desert, Coastal, Riverine, Arctic.',
    type: 'enum',
    section: '[[map.terrain]]',
    engineEffect: true,
  },
  movement_modifier: {
    summary: 'Per-region movement speed multiplier. Higher = faster movement through the region.',
    type: 'f64',
    section: '[[map.terrain]]',
    engineEffect: true,
  },
  defense_modifier: {
    summary: "Per-region defender bonus. Higher = stronger defender advantage. Read by combat.",
    type: 'f64',
    section: '[[map.terrain]]',
    engineEffect: true,
  },
  visibility: {
    summary: 'Per-region visibility used by the fog-of-war model.',
    type: 'f64',
    range: '[0, 1]',
    section: '[[map.terrain]]',
    engineEffect: true,
  },

  // ── [[environment.windows]] ─────────────────────────────────────────
  activation: {
    summary:
      'When an environmental window is active. Tagged enum: Always / TickRange{start,end} / Cycle{period,phase,duration}.',
    type: 'enum',
    section: '[[environment.windows]]',
    engineEffect: true,
  },
  applies_to: {
    summary: 'Terrain types this window affects. Empty = applies to every terrain.',
    type: '[TerrainType]',
    section: '[[environment.windows]]',
    engineEffect: true,
  },
  movement_factor: {
    summary: 'Multiplier on terrain.movement_modifier while this window is active.',
    type: 'f64',
    default: '1.0',
    section: '[[environment.windows]]',
    engineEffect: true,
  },
  defense_factor: {
    summary: 'Multiplier on terrain.defense_modifier while this window is active. Read by combat.',
    type: 'f64',
    default: '1.0',
    section: '[[environment.windows]]',
    engineEffect: true,
  },
  visibility_factor: {
    summary: 'Multiplier on terrain.visibility while this window is active.',
    type: 'f64',
    default: '1.0',
    section: '[[environment.windows]]',
    engineEffect: true,
  },
  detection_factor: {
    summary:
      'Global multiplier on every kill-chain phase detection roll while this window is active (not gated by applies_to).',
    type: 'f64',
    default: '1.0',
    section: '[[environment.windows]]',
    engineEffect: true,
  },

  // ── [factions.<id>] ─────────────────────────────────────────────────
  color: {
    summary: 'Hex color (#rrggbb) used to render this faction in the UI.',
    type: 'string',
    section: '[factions]',
    engineEffect: false,
  },
  tech_access: {
    summary: 'Tech card ids this faction is allowed to deploy.',
    type: '[string]',
    section: '[factions]',
    engineEffect: true,
  },
  initial_morale: {
    summary: 'Starting morale of the faction.',
    type: 'f64',
    range: '[0, 1]',
    section: '[factions]',
    engineEffect: true,
  },
  logistics_capacity: {
    summary: 'Cap on resource delivery per tick.',
    type: 'f64',
    section: '[factions]',
    engineEffect: true,
  },
  initial_resources: {
    summary: 'Starting resource pool for the faction.',
    type: 'f64',
    section: '[factions]',
    engineEffect: true,
  },
  resource_rate: {
    summary: 'Per-tick resource accrual. Attenuated by supply-network pressure when supply networks are declared.',
    type: 'f64',
    section: '[factions]',
    engineEffect: true,
  },
  command_resilience: {
    summary:
      'Attenuates the one-shot morale shock from a successful LeadershipDecapitation strike. 0.0 = full shock, 1.0 = morale fully preserved. No-op without a leadership cadre.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.0',
    section: '[factions]',
    engineEffect: true,
  },
  intelligence: {
    summary:
      'Scales fog-of-war visibility and, under intelligence-weighted belief mode, the confidence ceiling on foreign observations and believed-attribution draws.',
    type: 'f64',
    range: '[0, 1]',
    section: '[factions]',
    engineEffect: true,
  },
  diplomacy: {
    summary: 'Initial diplomatic stances toward other factions: [{ target_faction, stance }].',
    type: '[DiplomaticStance]',
    section: '[factions]',
    engineEffect: true,
  },
  doctrine: {
    summary:
      'Faction doctrine: Conventional, Guerrilla, Defensive, Disruption, CounterInsurgency, Blitzkrieg, or Adaptive.',
    type: 'enum',
    section: '[factions]',
    engineEffect: true,
  },
  posture: {
    summary: 'One-line summary of the faction\'s rules-of-engagement stance. Declarative — surfaced in Policy Implications.',
    type: 'string',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  ladder: {
    summary: 'Ordered low-to-high escalation rungs; each defines permitted / prohibited actions. Declarative (not enforced when picking actions).',
    type: '[EscalationRung]',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  de_escalation_floor: {
    summary: 'Tension at/above which the faction will not voluntarily de-escalate without an external trigger.',
    type: 'f64?',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  trigger_tension: {
    summary: 'Tension at/above which an escalation rung is authorized. None = always authorized.',
    type: 'f64?',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  permitted_actions: {
    summary: 'Free-text descriptions of capabilities this escalation rung permits.',
    type: '[string]',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  prohibited_actions: {
    summary: 'Explicit red lines for this escalation rung.',
    type: '[string]',
    section: '[factions.escalation_rules]',
    engineEffect: false,
  },
  ranks: {
    summary: 'Leadership cadre ranks, top-of-chain first. Must contain at least one entry. Drives LeadershipDecapitation.',
    type: '[LeadershipRank]',
    section: '[factions.leadership]',
    engineEffect: true,
  },
  succession_recovery_ticks: {
    summary: 'Number of ticks the morale-recovery ramp lasts after a decapitation. 0 disables the ramp (instant full effectiveness).',
    type: 'u32',
    section: '[factions.leadership]',
    engineEffect: true,
  },
  succession_floor: {
    summary: "Multiplier on the new rank's effectiveness on the strike tick; interpolates linearly to 1.0 over the recovery window.",
    type: 'f64',
    default: '0.5',
    section: '[factions.leadership]',
    engineEffect: true,
  },
  rules: {
    summary: 'Alliance-fracture rules: when a condition fires, this faction\'s stance toward a counterparty flips. At least one entry required.',
    type: '[FractureRule]',
    section: '[factions.alliance_fracture]',
    engineEffect: true,
  },
  counterparty: {
    summary: 'Faction whose stance flips when this fracture rule fires. Cannot equal the rule\'s owning faction.',
    type: 'FactionId',
    section: '[factions.alliance_fracture]',
    engineEffect: true,
  },
  new_stance: {
    summary: 'Diplomatic stance to flip to when an alliance-fracture rule fires.',
    type: 'Diplomacy',
    default: 'Hostile',
    section: '[factions.alliance_fracture]',
    engineEffect: true,
  },
  condition: [
    {
      summary: 'Trigger for an alliance-fracture rule (AttributionThreshold, MoraleFloor, TensionThreshold, EventFired, StrengthLossFraction).',
      type: 'FractureCondition',
      section: '[factions.alliance_fracture]',
      engineEffect: true,
    },
    {
      summary: 'Victory condition definition. Tagged enum: StrategicControl, MilitaryDominance, HoldRegions, InstitutionalCollapse, PeaceSettlement, NonKineticThreshold, Custom.',
      type: 'tagged enum',
      section: '[victory_conditions]',
      engineEffect: true,
    },
  ],
  queue_depth: {
    summary: 'Capacity threshold for a defender role\'s investigative queue. Saturation fires from this depth onward.',
    type: 'u32',
    section: '[factions.defender_capacities]',
    engineEffect: true,
  },
  service_rate: {
    summary: 'Mean queue items serviced per tick. Fractional rates accumulate (0.5 = one item every two ticks).',
    type: 'f64',
    section: '[factions.defender_capacities]',
    engineEffect: true,
  },
  overflow: {
    summary: 'Queue overflow behavior: DropNew, DropOldest, or Backlog.',
    type: 'enum',
    default: 'DropNew',
    section: '[factions.defender_capacities]',
    engineEffect: true,
  },
  saturated_detection_factor: {
    summary: 'Detection-roll multiplier for phases gated by this role while its queue is saturated. 1.0 = no penalty.',
    type: 'f64',
    default: '1.0',
    section: '[factions.defender_capacities]',
    engineEffect: true,
  },
  loyalty: {
    summary: "An institution's loyalty toward its parent faction. Below fracture_threshold it may defect.",
    type: 'f64',
    range: '[0, 1]',
    section: '[factions.faction_type.institutions]',
    engineEffect: true,
  },
  personnel: {
    summary: 'Headcount of an institution.',
    type: 'u64',
    section: '[factions.faction_type.institutions]',
    engineEffect: true,
  },
  fracture_threshold: {
    summary: 'Loyalty level below which an institution may defect from its parent faction.',
    type: 'f64?',
    section: '[factions.faction_type.institutions]',
    engineEffect: true,
  },
  institution_type: {
    summary:
      'Institution category: LawEnforcement, Intelligence, Judiciary, Legislature, Executive, NationalGuard, FederalAgency, FinancialRegulator, MediaRegulator, or Custom.',
    type: 'enum',
    section: '[factions.faction_type.institutions]',
    engineEffect: true,
  },
  unit_type: {
    summary:
      'Force unit class: Infantry, Mechanized, Armor, Artillery, AirSupport, Naval, SpecialOperations, CyberUnit, DroneSwarm, LawEnforcement, Militia, Logistics, AirDefense, ElectronicWarfare, or Custom.',
    type: 'enum',
    section: '[factions.forces]',
    engineEffect: true,
  },
  strength: {
    summary: 'Combat strength of a force unit.',
    type: 'f64',
    section: '[factions.forces]',
    engineEffect: true,
  },
  mobility: {
    summary: 'Movement speed of a force unit.',
    type: 'f64',
    section: '[factions.forces]',
    engineEffect: true,
  },
  upkeep: {
    summary: 'Resources consumed per tick to maintain a force unit. Not attenuated by supply pressure.',
    type: 'f64',
    section: '[factions.forces]',
    engineEffect: true,
  },
  morale_modifier: {
    summary:
      "Per-unit cohesion/training scalar folded into combat as (1.0 + morale_modifier). 0.0 = no change; 0.10–0.15 ≈ elite; negative = green/demoralized (floored so it can't invert combat).",
    type: 'f64',
    default: '0.0',
    section: '[factions.forces]',
    engineEffect: true,
  },
  capabilities: {
    summary: 'Special capabilities of a force unit (Garrison, Raid, Sabotage, Recon, Interdiction, AreaDenial, CounterUAS, EW, Cyber, InfoOps, Humanitarian).',
    type: '[UnitCapability]',
    section: '[factions.forces]',
    engineEffect: true,
  },
  force_projection: {
    summary: 'Optional long-range projection mode: Airlift{capacity}, Naval{range}, or StandoffStrike{range,damage}.',
    type: 'enum?',
    section: '[factions.forces]',
    engineEffect: true,
  },
  recruitment: {
    summary: 'Optional recruitment config: rate, population_threshold, unit_type, base_strength, cost.',
    type: 'table?',
    section: '[factions]',
    engineEffect: true,
  },
  base_strength: {
    summary: 'Combat strength of each unit produced by recruitment.',
    type: 'f64',
    section: '[factions.recruitment]',
    engineEffect: true,
  },
  population_threshold: {
    summary: 'Population level required for recruitment to proceed.',
    type: 'f64',
    section: '[factions.recruitment]',
    engineEffect: true,
  },

  // ── [technology.<id>] ───────────────────────────────────────────────
  category: {
    summary:
      'Tech card category: Surveillance, OffensiveDrone, CounterDrone, ElectronicWarfare, Cyber, Communications, InformationWarfare, Concealment, Logistics, or Custom = "...".',
    type: 'enum',
    section: '[technology]',
    engineEffect: true,
  },
  effects: [
    {
      summary:
        'Statistical effects a tech card applies while deployed (DetectionModifier, CombatModifier, InfraProtection, MoraleEffect, AreaDenial, CommsDisruption, AttritionModifier, CivilianSentiment, SupplyInterdiction, IntelGain, CounterTech).',
      type: '[TechEffect]',
      section: '[technology]',
      engineEffect: true,
    },
    {
      summary: 'Effects applied when an event fires (DamageInfra, MoraleShift, SpawnUnits, TensionShift, DeceptionOp, ...). See the schema for the full list.',
      type: '[EventEffect]',
      section: '[events]',
      engineEffect: true,
    },
  ],
  cost_per_tick: {
    summary: 'Resource drain while a tech card is deployed.',
    type: 'f64',
    section: '[technology]',
    engineEffect: true,
  },
  deployment_cost: {
    summary: 'One-shot resource cost to deploy a tech card.',
    type: 'f64',
    section: '[technology]',
    engineEffect: true,
  },
  countered_by: {
    summary: 'Tech card ids that suppress this card when the opponent deploys them.',
    type: '[string]',
    section: '[technology]',
    engineEffect: true,
  },
  terrain_modifiers: {
    summary: 'Per-terrain effectiveness overrides: [{ terrain, effectiveness }].',
    type: '[TerrainTechModifier]',
    section: '[technology]',
    engineEffect: true,
  },
  coverage_limit: {
    summary: 'Maximum simultaneous deployments of this tech card. Omit for unlimited.',
    type: 'u32?',
    section: '[technology]',
    engineEffect: true,
  },

  // ── [political_climate] ─────────────────────────────────────────────
  tension: {
    summary:
      'Internal political-tension state variable. Gates events, segment activation, and escalation branches. Not displayed in the UI.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate]',
    engineEffect: true,
  },
  institutional_trust: {
    summary: 'Population trust in institutions. Feeds InstitutionalCollapse victory checks and erosion mechanics.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate]',
    engineEffect: true,
  },
  media_landscape: {
    summary: 'Media environment scalars (fragmentation, disinformation_susceptibility, state_control, social_media_penetration, internet_availability).',
    type: 'table',
    section: '[political_climate]',
    engineEffect: true,
  },
  fragmentation: {
    summary: 'How siloed media consumption is.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.media_landscape]',
    engineEffect: true,
  },
  disinformation_susceptibility: {
    summary: "Population's exposure to false narratives.",
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.media_landscape]',
    engineEffect: true,
  },
  state_control: {
    summary: 'Government control of media.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.media_landscape]',
    engineEffect: true,
  },
  social_media_penetration: {
    summary: 'Social-media reach across the population.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.media_landscape]',
    engineEffect: true,
  },
  internet_availability: {
    summary: 'Internet availability across the population.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.media_landscape]',
    engineEffect: true,
  },
  population_segments: {
    summary: 'Population subgroups with sympathies, activation thresholds, and actions they take when activated.',
    type: '[PopulationSegment]',
    section: '[political_climate]',
    engineEffect: true,
  },
  global_modifiers: {
    summary: 'Scenario-wide climate modifiers (EconomicCrisis, NaturalDisaster, InternationalPressure, HealthCrisis, ElectionCycle).',
    type: '[ClimateModifier]',
    section: '[political_climate]',
    engineEffect: true,
  },
  fraction: {
    summary: 'Fraction of total population this segment represents.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  concentrated_in: {
    summary: 'Region ids where this population segment is concentrated.',
    type: '[string]',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  sympathies: {
    summary: 'Per-faction sympathy of the segment: [{ faction, sympathy }].',
    type: '[{faction, sympathy}]',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  activation_threshold: {
    summary: 'Tension level that triggers this segment to take its activation_actions.',
    type: 'f64',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  activation_actions: {
    summary: 'Actions the segment takes when activated (NonCooperation, Protest, Intelligence, MaterialSupport, ArmedResistance, Flee, Sabotage).',
    type: '[CivilianAction]',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  volatility: {
    summary: 'How readily this segment\'s activation state changes.',
    type: 'f64',
    range: '[0, 1]',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },
  activated: {
    summary: 'Whether the segment starts activated. Usually omit.',
    type: 'bool',
    default: 'false',
    section: '[political_climate.population_segments]',
    engineEffect: true,
  },

  // ── [events.<id>] ───────────────────────────────────────────────────
  earliest_tick: {
    summary: 'Earliest tick this event can fire. Omit for no lower bound.',
    type: 'u32?',
    section: '[events]',
    engineEffect: true,
  },
  latest_tick: {
    summary: 'Latest tick this event can fire. Omit for no upper bound.',
    type: 'u32?',
    section: '[events]',
    engineEffect: true,
  },
  conditions: {
    summary: 'Conditions that must all hold for an event to be eligible to fire (RegionControl, TensionAbove/Below, EventFired, TickAtLeast, ...).',
    type: '[EventCondition]',
    section: '[events]',
    engineEffect: true,
  },
  probability: {
    summary: 'Per-tick chance the event fires once eligible.',
    type: 'f64',
    range: '[0, 1]',
    section: '[events]',
    engineEffect: true,
  },
  repeatable: {
    summary: 'If false, the event fires at most once for the whole run.',
    type: 'bool',
    section: '[events]',
    engineEffect: true,
  },
  chain: {
    summary: 'Event id to trigger immediately after this one fires. Chains are validated for cycles at startup.',
    type: 'string?',
    section: '[events]',
    engineEffect: true,
  },
  defender_options: {
    summary: 'Declarative counterfactual defender responses surfaced in Policy Implications. Not auto-selected by the engine.',
    type: '[DefenderOption]',
    section: '[events]',
    engineEffect: false,
  },

  // ── [simulation] ────────────────────────────────────────────────────
  max_ticks: {
    summary: 'Hard cap on simulation length in ticks.',
    type: 'u32',
    section: '[simulation]',
    engineEffect: true,
  },
  tick_duration: {
    summary: 'Real-world duration of one tick: Hours = N, Days = N, or Weeks = N.',
    type: 'enum',
    section: '[simulation]',
    engineEffect: true,
  },
  monte_carlo_runs: {
    summary: 'Default number of runs for --monte-carlo mode.',
    type: 'u32',
    section: '[simulation]',
    engineEffect: false,
  },
  seed: {
    summary: 'Optional fixed RNG seed. Set explicitly for bit-identical runs; omit for per-batch reproducible sub-seeds.',
    type: 'u64?',
    section: '[simulation]',
    engineEffect: true,
  },
  fog_of_war: {
    summary: 'Enables the per-faction visibility model.',
    type: 'bool',
    section: '[simulation]',
    engineEffect: true,
  },
  attrition_model: {
    summary: 'Casualty model: LanchesterLinear, LanchesterSquare, Hybrid, or Stochastic { noise }.',
    type: 'enum',
    section: '[simulation]',
    engineEffect: true,
  },
  snapshot_interval: [
    {
      summary: 'Ticks between full state snapshots.',
      type: 'u32',
      section: '[simulation]',
      engineEffect: true,
    },
    {
      summary: 'Ticks between belief-shape snapshots per faction. 0 = no belief snapshot stream.',
      type: 'u32',
      default: '0',
      section: '[simulation.belief_model]',
      engineEffect: true,
    },
  ],
  noise: {
    summary: 'Relative standard deviation of casualty rolls under the Stochastic attrition model (0.1 ≈ ±10%).',
    type: 'f64',
    section: '[simulation.attrition_model]',
    engineEffect: true,
  },

  // ── [simulation.belief_model] ───────────────────────────────────────
  enabled: {
    summary:
      'Master toggle for the persistent belief-asymmetry mechanic. When false (default), the legacy ground-truth fast path is used and DeceptionOp/IntelligenceShare/AmbientIntel are no-ops.',
    type: 'bool',
    default: 'false',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  force_decay_per_tick: {
    summary: 'Per-tick confidence decay for force beliefs.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.05',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  region_decay_per_tick: {
    summary: 'Per-tick confidence decay for region-control beliefs.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.02',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  scalar_decay_per_tick: {
    summary: 'Per-tick confidence decay for scalar beliefs (faction morale, resources).',
    type: 'f64',
    range: '[0, 1]',
    default: '0.03',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  prune_threshold: {
    summary: 'Belief entries below this confidence are dropped from persistent state. 0.0 = never prune.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.05',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  intelligence_weighting: {
    summary:
      'When true, foreign observations are capped at an intelligence-derived confidence ceiling and Bayesian-blended with the prior; own-faction facts stay perfect. Requires enabled = true.',
    type: 'bool',
    default: 'false',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },
  believed_attribution: {
    summary:
      'When true, a detecting defender draws a believed attacker weighted by its intelligence and any planted false-flag belief, so attribution accounting fires against the believed faction. Requires enabled = true and ≥1 kill chain.',
    type: 'bool',
    default: 'false',
    section: '[simulation.belief_model]',
    engineEffect: true,
  },

  // ── [networks.<id>] ─────────────────────────────────────────────────
  owner: {
    summary: 'Optional owning faction of a network. Required when kind = "supply" (the pipeline needs a faction to attenuate).',
    type: 'FactionId?',
    section: '[networks]',
    engineEffect: true,
  },
  capacity: {
    summary: 'Static edge capacity (units/tick), multiplied by the runtime factor to give effective capacity each tick.',
    type: 'f64',
    range: '>= 0',
    section: '[networks]',
    engineEffect: true,
  },
  latency: {
    summary: 'Edge latency. Surfaced in the report; not yet consumed by metrics.',
    type: 'f64',
    range: '>= 0',
    section: '[networks]',
    engineEffect: false,
  },
  bandwidth: {
    summary: 'Edge peak-burst capacity, distinct from sustained capacity.',
    type: 'f64',
    range: '>= 0',
    section: '[networks]',
    engineEffect: true,
  },
  trust: {
    summary: 'Confidence an edge is not adversarially observed.',
    type: 'f64',
    range: '[0, 1]',
    section: '[networks]',
    engineEffect: true,
  },

  // ── [kill_chains.<id>] ──────────────────────────────────────────────
  attacker: {
    summary: 'Faction executing this kill chain.',
    type: 'FactionId',
    section: '[kill_chains]',
    engineEffect: true,
  },
  target: {
    summary: 'Faction targeted by this kill chain.',
    type: 'FactionId',
    section: '[kill_chains]',
    engineEffect: true,
  },
  entry_phase: {
    summary: 'Phase id where the chain begins executing.',
    type: 'phase id',
    section: '[kill_chains]',
    engineEffect: true,
  },
  phases: {
    summary: 'The phases of this kill chain, keyed by phase id. The phase graph must terminate.',
    type: 'table',
    section: '[kill_chains]',
    engineEffect: true,
  },
  prerequisites: {
    summary: 'Phase ids that must succeed before this phase can run.',
    type: '[phase_id]',
    default: '[]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  base_success_probability: {
    summary: 'Base probability this phase succeeds at completion, before prerequisite boosts.',
    type: 'f64',
    range: '[0, 1]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  min_duration: {
    summary: 'Minimum number of active ticks before the phase can complete.',
    type: 'u32',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  max_duration: {
    summary: 'Maximum number of active ticks for the phase.',
    type: 'u32',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  detection_probability_per_tick: {
    summary: 'Per-active-tick chance the defender detects this phase. Accumulates exposure over the phase\'s duration.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.0',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  prerequisite_success_boost: {
    summary: 'Additive success-probability boost applied per successful prerequisite phase.',
    type: 'f64',
    default: '0.0',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  attribution_difficulty: {
    summary: 'How hard the phase is to attribute. 0.0 = trivially attributable, 1.0 = opaque.',
    type: 'f64',
    range: '[0, 1]',
    default: '0.5',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  cost: {
    summary: 'Phase cost: attacker_dollars, defender_dollars, attacker_resources, optional confidence.',
    type: 'PhaseCost',
    default: 'zero',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  targets_domains: {
    summary: 'Defensive domains whose seams this phase exploits. ≥2 domains marks the phase cross-domain for seam scoring.',
    type: '[DefensiveDomain]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  outputs: {
    summary: 'Effects applied when the phase succeeds (IntelligenceGain, InfraDamage, TensionDelta, MoraleDelta, LeadershipDecapitation, non-kinetic accumulators, ...).',
    type: '[PhaseOutput]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  branches: {
    summary: 'Next-phase transitions, evaluated in declaration order; first match wins. A phase with no branches ends the chain.',
    type: '[PhaseBranch]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  parameter_confidence: {
    summary: 'Author self-assessment (High/Medium/Low) of how defensible this phase\'s base rates are. Low-rated phases get a dedicated report section.',
    type: 'enum?',
    section: '[kill_chains.phases]',
    engineEffect: false,
  },
  warning_indicators: {
    summary: 'IWI / IOC observables surfaced in the Countermeasure Analysis report section. Declarative.',
    type: '[WarningIndicator]',
    section: '[kill_chains.phases]',
    engineEffect: false,
  },
  defender_noise: {
    summary: 'Per-tick alert volume this phase generates against a named defender role\'s queue: [{ defender, role, items_per_tick }].',
    type: '[DefenderNoise]',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  gated_by_defender: {
    summary: 'When the named defender role\'s queue is saturated, multiplies this phase\'s detection roll by that role\'s saturated_detection_factor.',
    type: 'DefenderRoleRef',
    section: '[kill_chains.phases]',
    engineEffect: true,
  },
  attacker_dollars: {
    summary: 'Dollar cost charged whether the phase succeeds or fails — the attacker pays for the attempt. Accrues against attacker_budget.',
    type: 'f64',
    section: '[kill_chains.phases.cost]',
    engineEffect: true,
  },
  defender_dollars: {
    summary: 'Dollar cost charged only when the phase succeeds (cost of closing the gap a landed attack exploited). Accrues against defender_budget.',
    type: 'f64',
    section: '[kill_chains.phases.cost]',
    engineEffect: true,
  },
  attacker_resources: {
    summary: 'Scenario-resource units consumed from the attacker\'s pool for this phase.',
    type: 'f64',
    section: '[kill_chains.phases.cost]',
    engineEffect: true,
  },
  observable: {
    summary: 'Collection discipline needed to see a warning indicator: SIGINT, HUMINT, OSINT, GEOINT, MASINT, CYBINT, FININT, Physical, or Custom.',
    type: 'enum',
    section: '[kill_chains.phases.warning_indicators]',
    engineEffect: false,
  },
  detectability: {
    summary: 'Probability of catching the observable if the defender is actively looking for it.',
    type: 'f64',
    range: '[0, 1]',
    section: '[kill_chains.phases.warning_indicators]',
    engineEffect: false,
  },
  next_phase: {
    summary: 'Phase id this branch transitions to when its condition matches.',
    type: 'phase id',
    section: '[kill_chains.phases.branches]',
    engineEffect: true,
  },
  threshold: [
    {
      summary: 'Threshold value for an escalation / victory / fracture condition. Side and units depend on the enclosing condition.',
      type: 'f64',
      section: '(condition)',
      engineEffect: true,
    },
  ],
  direction: {
    summary: 'Comparison side for an escalation-threshold branch: Above (>= threshold) or Below (<= threshold).',
    type: 'enum',
    section: '[kill_chains.phases.branches]',
    engineEffect: true,
  },
  sustained_ticks: {
    summary: 'Consecutive end-of-tick snapshots the predicate must hold before an escalation branch fires. Built-in hysteresis. 0 = "currently true".',
    type: 'u32',
    section: '[kill_chains.phases.branches]',
    engineEffect: true,
  },

  // ── shared leaf keys (section-disambiguated) ────────────────────────
  id: {
    summary: 'Stable identifier for this entry. Must equal the table key it lives under.',
    type: 'string',
    section: '(any)',
    engineEffect: false,
  },
  name: {
    summary: 'Human-readable display name. Descriptive only.',
    type: 'string',
    section: '(any)',
    engineEffect: false,
  },
  description: {
    summary: 'Free-form description. Describe effects, not implementations. Descriptive only.',
    type: 'string',
    section: '(any)',
    engineEffect: false,
  },
  type: {
    summary: 'Tagged-enum discriminator selecting the variant of the enclosing value (e.g. map.source, attrition, branch condition, tech/event effect).',
    type: 'enum tag',
    section: '(any)',
    engineEffect: true,
  },
  kind: {
    summary: 'Tagged-enum discriminator (kind = "...") selecting a variant — used by deception/intel payloads, fracture conditions, calibration metrics, strategy-space domains, and network type.',
    type: 'enum tag',
    section: '(any)',
    engineEffect: true,
  },
  faction: {
    summary: 'A faction id referenced by this entry (the owning faction of a victory condition, a target of an effect, a sympathy, etc.).',
    type: 'FactionId',
    section: '(any)',
    engineEffect: true,
  },
  region: {
    summary: 'A region id referenced by this entry.',
    type: 'RegionId',
    section: '(any)',
    engineEffect: true,
  },
  effectiveness: [
    {
      summary: 'A leadership rank\'s multiplicative effectiveness scalar; top is conventionally 1.0 and successors lower.',
      type: 'f64',
      range: '[0, 1]',
      section: '[factions.leadership]',
      engineEffect: true,
    },
    {
      summary: 'An institution\'s operational effectiveness.',
      type: 'f64',
      range: '[0, 1]',
      section: '[factions.faction_type.institutions]',
      engineEffect: true,
    },
    {
      summary: 'Per-terrain effectiveness of a tech card.',
      type: 'f64',
      section: '[technology]',
      engineEffect: true,
    },
  ],
  delta: {
    summary: 'Signed change this effect applies to the targeted quantity (morale, tension, loyalty, resources, sympathy, an accumulator, ...).',
    type: 'f64',
    section: '(effect)',
    engineEffect: true,
  },
  factor: {
    summary: 'Multiplicative factor this effect / modifier applies to the targeted quantity.',
    type: 'f64',
    section: '(effect)',
    engineEffect: true,
  },
  stance: {
    summary: 'Diplomatic stance: War, Hostile, Neutral, Cooperative, or Allied.',
    type: 'enum',
    section: '[factions]',
    engineEffect: true,
  },
  confidence: {
    summary: 'Coarse author confidence tag (high / medium / low). Signals defensibility; does not affect simulation.',
    type: 'enum?',
    section: '(any)',
    engineEffect: false,
  },
  duration: {
    summary: 'Number of ticks: how long a HoldRegions victory must be sustained, or a Cycle window stays active.',
    type: 'u32',
    section: '(any)',
    engineEffect: true,
  },

  // ── [strategy_space] ────────────────────────────────────────────────
  path: {
    summary: 'Dotted parameter path a search decision-variable or robustness assignment targets (same syntax as --counterfactual / --sensitivity).',
    type: 'string',
    section: '[strategy_space]',
    engineEffect: false,
  },
  domain: {
    summary: 'Sampling domain for a strategy-space decision variable: continuous { low, high, steps } or discrete { values }.',
    type: 'Domain',
    section: '[strategy_space]',
    engineEffect: false,
  },
  low: {
    summary: 'Lower bound of a continuous domain or observation interval.',
    type: 'f64',
    section: '[strategy_space]',
    engineEffect: false,
  },
  high: {
    summary: 'Upper bound of a continuous domain or observation interval.',
    type: 'f64',
    section: '[strategy_space]',
    engineEffect: false,
  },
  steps: {
    summary: 'Number of evenly-spaced grid values a continuous domain emits in grid mode.',
    type: 'u32',
    section: '[strategy_space]',
    engineEffect: false,
  },
  values: {
    summary: 'The enumerated values of a discrete strategy-space domain. Non-empty.',
    type: '[f64]',
    section: '[strategy_space]',
    engineEffect: false,
  },
  objectives: {
    summary: 'Search objectives to optimize (maximize_win_rate, minimize_detection, minimize_attacker_cost, ...).',
    type: '[SearchObjective]',
    section: '[strategy_space]',
    engineEffect: false,
  },
  attacker_profiles: {
    summary: 'Named full attacker-side assignments evaluated against each defender posture in --robustness mode.',
    type: '[AttackerProfile]',
    section: '[strategy_space]',
    engineEffect: false,
  },
  assignments: {
    summary: 'Parameter path/value pairs an attacker profile applies before evaluating defender postures.',
    type: '[Assignment]',
    section: '[strategy_space.attacker_profiles]',
    engineEffect: false,
  },
};

/**
 * Normalize a raw TOML section header (the text inside the brackets) into a
 * canonical dotted family path: strip whitespace / quoting and elide the
 * inner-table instance ids that follow a known family root (e.g.
 * `factions.alpha.forces.tank` → `factions.forces`), so the header is
 * comparable to the `section` tags stored in {@link FIELD_DOCS}.
 *
 * We don't have the schema's id-vs-family distinction at parse time, so we
 * use a small set of known family roots and treat the token *after* each as
 * an instance id to be elided.
 *
 * @param {string} rawHeader  The text between `[` and `]` (or `[[` `]]`).
 * @returns {string} canonical dotted family path, e.g. "factions.forces".
 */
export function canonicalizeSection(rawHeader) {
  if (!rawHeader) return '';
  const parts = rawHeader
    .trim()
    .split('.')
    .map((p) => p.trim().replace(/^["']|["']$/g, ''))
    .filter((p) => p.length > 0);

  // Families whose *next* path token is an instance id we should elide.
  const instanceParents = new Set([
    'factions',
    'technology',
    'events',
    'kill_chains',
    'victory_conditions',
    'networks',
    'regions', // map.regions.<id>
    'infrastructure', // map.infrastructure.<id>
    'institutions', // ...institutions.<id>
    'defender_capacities',
    'forces',
    'phases',
    'nodes',
    'edges',
  ]);

  const out = [];
  for (let i = 0; i < parts.length; i++) {
    const tok = parts[i];
    out.push(tok);
    if (instanceParents.has(tok) && i + 1 < parts.length) {
      // Skip the instance id that follows this family token.
      i++;
    }
  }
  return out.join('.');
}

/**
 * Convert a stored `section` tag (e.g. "[map.regions]", "[[map.terrain]]",
 * "[factions.faction_type.institutions]") into the same canonical dotted
 * family path produced by {@link canonicalizeSection}, so the two can be
 * compared directly. Tags like "(any)", "(effect)", "(condition)",
 * "(top-level)" have no concrete section and return ''.
 *
 * @param {string} tag
 * @returns {string}
 */
function canonicalizeTag(tag) {
  if (!tag || tag.startsWith('(')) return '';
  // A stored tag is already a family path with no instance ids, so we only
  // strip the brackets and normalize whitespace — running the
  // instance-eliding logic here would wrongly collapse legitimate family
  // segments (e.g. "factions.faction_type.institutions" → "factions.institutions").
  return tag
    .replace(/^\[+/, '')
    .replace(/\]+$/, '')
    .trim()
    .split('.')
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .join('.');
}

/**
 * Score how well a doc variant's `section` tag matches the cursor's
 * canonical section path. Higher is better; a negative score disqualifies.
 *
 *   - A generic variant (canonical '') scores 0 — always an acceptable
 *     fallback.
 *   - A variant whose canonical tag is a prefix of (or equal to) the cursor
 *     section scores by the number of matching path segments — longer match
 *     wins, so the most specific applicable variant is chosen.
 *   - A section-specific variant that does not prefix-match scores -1.
 *
 * @param {string} tagCanon     canonicalized variant section (may be '')
 * @param {string} sectionCanon canonicalized cursor section (may be '')
 * @returns {number}
 */
function scoreVariant(tagCanon, sectionCanon) {
  if (tagCanon === '') return 0; // generic fallback
  if (sectionCanon === '') return -1; // specific variant, no section context
  const tagParts = tagCanon.split('.');
  const secParts = sectionCanon.split('.');
  // tag must be a prefix of the cursor section (the cursor can be deeper,
  // e.g. tag "kill_chains" matches section "kill_chains.phases.cost").
  for (let i = 0; i < tagParts.length; i++) {
    if (tagParts[i] !== secParts[i]) return -1;
  }
  return tagParts.length;
}

/**
 * Look up the documentation for a field key, disambiguating multi-meaning
 * keys using the section the cursor is in.
 *
 * @param {string} key        The bare TOML key the user hovered (e.g. "tension").
 * @param {string} [section]  The raw text inside the nearest `[section]`
 *                            header above the cursor (e.g. "factions.alpha").
 *                            Optional — when absent, the generic / first
 *                            variant is returned.
 * @returns {(FieldDoc & {key: string}) | null} The best-matching doc with the
 *   resolved `key` attached, or null if the key is undocumented.
 */
export function lookupFieldDoc(key, section) {
  if (!key) return null;
  const entry = FIELD_DOCS[key];
  if (!entry) return null;

  const sectionCanon = canonicalizeSection(section || '');

  if (!Array.isArray(entry)) {
    return { ...entry, key };
  }

  // Multi-variant key: pick the best-scoring applicable variant.
  let best = null;
  let bestScore = -Infinity;
  for (const variant of entry) {
    const score = scoreVariant(canonicalizeTag(variant.section), sectionCanon);
    if (score > bestScore) {
      bestScore = score;
      best = variant;
    }
  }
  // If every variant was disqualified and none was generic, fall back to the
  // first variant rather than returning nothing — a partial hint beats a
  // blank tooltip.
  if (!best) best = entry[0];
  return { ...best, key };
}

/**
 * Extract the bare TOML key at a character offset within a single line, plus
 * a small "is this actually a key?" guard so hovering a value or a comment
 * doesn't pop a misleading tooltip.
 *
 * We return the key token only when the offset lands on the key portion
 * (left of the first top-level `=`), or — on a line with no `=` (a key inside
 * a multi-line array, or a dotted-key fragment) — when the hovered word is
 * the line's first identifier token.
 *
 * @param {string} line    The full text of the line under the cursor.
 * @param {number} offset  0-based character index within `line`.
 * @returns {string | null} The bare key, or null if the offset isn't on a key.
 */
export function keyAtOffset(line, offset) {
  if (typeof line !== 'string' || line.length === 0) return null;
  // Ignore comment lines and section headers — those aren't field keys.
  const trimmedStart = line.replace(/^\s*/, '');
  if (trimmedStart.startsWith('#') || trimmedStart.startsWith('[')) return null;

  const isWordChar = (c) => /[A-Za-z0-9_]/.test(c);
  // Clamp offset into the line.
  let i = Math.max(0, Math.min(offset, line.length - 1));
  // If we're sitting just past the end of a word (offset === word end + 1),
  // step back one so the boundary scan still catches the word.
  if (i > 0 && !isWordChar(line[i]) && isWordChar(line[i - 1])) i -= 1;
  if (!isWordChar(line[i])) return null;

  let start = i;
  while (start > 0 && isWordChar(line[start - 1])) start--;
  let end = i;
  while (end < line.length - 1 && isWordChar(line[end + 1])) end++;
  const word = line.slice(start, end + 1);
  if (!word) return null;

  // Reject pure numbers.
  if (/^[0-9]+$/.test(word)) return null;

  const eq = line.indexOf('=');
  if (eq === -1) {
    // No assignment on this line. Only the first identifier token is a key.
    const firstWordMatch = trimmedStart.match(/^([A-Za-z_][A-Za-z0-9_]*)/);
    if (firstWordMatch && firstWordMatch[1] === word) return word;
    return null;
  }

  // Assignment present: the key is the token left of `=`. Accept the hovered
  // word only when it lies on the key side.
  if (end < eq) return word;
  return null;
}

/**
 * Walk backward from a line index to find the nearest enclosing TOML section
 * header, returning the text inside its brackets (single or double). Returns
 * '' when the cursor is above the first section header (top-level keys).
 *
 * @param {string[]} lines   The editor content split into lines.
 * @param {number}   lineIdx The index of the line the cursor is on.
 * @returns {string} The raw header text inside the brackets, or ''.
 */
export function enclosingSection(lines, lineIdx) {
  if (!Array.isArray(lines)) return '';
  for (let i = Math.min(lineIdx, lines.length - 1); i >= 0; i--) {
    const t = (lines[i] || '').trim();
    // Array-of-tables header [[a.b]] or table header [a.b].
    const m = t.match(/^\[\[?\s*([^\]]+?)\s*\]\]?\s*(#.*)?$/);
    if (m) return m[1];
  }
  return '';
}

/**
 * High-level resolver used by the editor's hover handler: given the full
 * editor text and a flat character offset into it, resolve the field key at
 * that offset (if any) and return its documentation.
 *
 * @param {string} text   The full editor content.
 * @param {number} offset Flat 0-based character offset into `text`.
 * @returns {(FieldDoc & {key: string}) | null}
 */
export function docAtOffset(text, offset) {
  if (typeof text !== 'string' || text.length === 0) return null;
  const clamped = Math.max(0, Math.min(offset, text.length));

  // Locate the line containing `offset` and the column within it.
  const upto = text.slice(0, clamped);
  const lineStart = upto.lastIndexOf('\n') + 1;
  const lineEndRel = text.indexOf('\n', clamped);
  const lineEnd = lineEndRel === -1 ? text.length : lineEndRel;
  const line = text.slice(lineStart, lineEnd);
  const col = clamped - lineStart;

  const key = keyAtOffset(line, col);
  if (!key) return null;

  const lineIdx = upto.split('\n').length - 1;
  const lines = text.split('\n');
  const section = enclosingSection(lines, lineIdx);

  return lookupFieldDoc(key, section);
}

/**
 * @returns {number} The number of distinct field keys documented in the
 *   catalog. Used by tests / diagnostics.
 */
export function documentedFieldCount() {
  return Object.keys(FIELD_DOCS).length;
}
