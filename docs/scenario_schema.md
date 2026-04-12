# Scenario Schema Reference

A Faultline scenario is a TOML file describing a simulated conflict. This document is the authoritative reference for every section, field, and enum variant the engine accepts.

The canonical Rust definitions live in `crates/faultline-types/src/`. If this document and the source ever disagree, the source wins — but please file an issue so the docs can be fixed.

> **Sourcing requirement.** Every numeric parameter in a scenario must be derivable from publicly available open-source intelligence. See [LEGAL.md](../LEGAL.md) for details.

## Top-level layout

```toml
[meta]              # name, description, author, version, tags
[map]               # source, regions, infrastructure, terrain
[factions.<id>]     # one table per faction
[technology.<id>]   # one table per tech card (may be empty)
[political_climate] # tension, trust, media, segments, modifiers
[events.<id>]       # one table per event (may be empty)
[simulation]        # max_ticks, tick_duration, seed, attrition
[victory_conditions.<id>]  # one table per victory condition
```

All map keys (`<id>`) are strings. The engine uses `BTreeMap` everywhere, so iteration order is alphabetical and deterministic — pick IDs that sort sensibly if order matters for debugging.

---

## `[meta]`

Free-form descriptive metadata. None of these fields affect simulation outcomes.

| Field | Type | Description |
|---|---|---|
| `name` | string | Human-readable scenario name |
| `description` | string | What the scenario models. Multi-line OK |
| `author` | string | Scenario author handle |
| `version` | string | Semver-style version string |
| `tags` | `[string]` | Free-form tags for indexing |

---

## `[map]`

Defines the geography. Composed of a `source`, a set of named `regions`, optional `infrastructure` nodes, and per-region `terrain` overlays.

### `[map.source]`

A tagged enum (`type = "..."`):

```toml
[map.source]
type = "Grid"
width = 4
height = 3
```

| Variant | Required fields |
|---|---|
| `BuiltIn` | `name` (e.g. `"us_states"`) |
| `GeoJson` | `path` (relative to scenario file) |
| `Grid` | `width`, `height` (u32) |

### `[map.regions.<region_id>]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | Display name |
| `population` | u64 | Civilian population |
| `urbanization` | f64 | `[0,1]` |
| `initial_control` | string? | Faction id, or omit for neutral |
| `strategic_value` | f64 | `[0,1]` weight for AI and victory checks |
| `borders` | `[string]` | Ids of adjacent regions |
| `centroid` | `{lat,lon}`? | Optional geographic centroid |

### `[map.infrastructure.<infra_id>]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `region` | string | Region id this node sits in |
| `infra_type` | enum | See below |
| `criticality` | f64 | `[0,1]` weight for damage scoring |
| `initial_status` | f64 | `[0,1]` health (1.0 = intact) |
| `repairable` | u32? | Ticks to repair, or omit for permanent |

`infra_type` variants: `PowerGrid`, `Telecommunications`, `TransportHub`, `GovernmentBuilding`, `MediaStation`, `WaterSystem`, `FuelDepot`, `Hospital`, `SupplyChain`, `Internet`.

### `[[map.terrain]]`

An array of per-region terrain overlays. Each entry:

| Field | Type | Notes |
|---|---|---|
| `region` | string | Region id |
| `terrain_type` | enum | See below |
| `movement_modifier` | f64 | Higher = faster movement |
| `defense_modifier` | f64 | Higher = stronger defender bonus |
| `visibility` | f64 | `[0,1]`, used by fog of war |

`terrain_type` variants: `Urban`, `Suburban`, `Rural`, `Forest`, `Mountain`, `Desert`, `Coastal`, `Riverine`, `Arctic`.

---

## `[factions.<faction_id>]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `description` | string | |
| `color` | string | `#rrggbb` for the UI |
| `tech_access` | `[string]` | Tech card ids this faction may deploy |
| `initial_morale` | f64 | `[0,1]` |
| `logistics_capacity` | f64 | Cap on resource delivery per tick |
| `initial_resources` | f64 | Starting resource pool |
| `resource_rate` | f64 | Per-tick resource accrual |
| `command_resilience` | f64 | `[0,1]`, slows morale collapse |
| `intelligence` | f64 | `[0,1]`, scales fog-of-war visibility |
| `diplomacy` | `[DiplomaticStance]` | Initial relations |
| `doctrine` | enum | `Conventional`, `Guerrilla`, `Defensive`, `Disruption`, `CounterInsurgency`, `Blitzkrieg`, `Adaptive` |
| `recruitment` | table? | See `RecruitmentConfig` below |

### `[factions.<id>.faction_type]`

A tagged enum (`kind = "..."`):

| Variant | Extra fields |
|---|---|
| `Government` | `institutions` table (see below) |
| `Military` | `branch` (`Army`, `Navy`, `AirForce`, `Marines`, `SpaceForce`, `CoastGuard`, `Combined`, `Custom = "..."`) |
| `Insurgent` | — |
| `Civilian` | — |
| `PrivateMilitary` | — |
| `Foreign` | `is_proxy` (bool) |

### `[factions.<id>.faction_type.institutions.<institution_id>]`

Only valid when `kind = "Government"`.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `institution_type` | enum | `LawEnforcement`, `Intelligence`, `Judiciary`, `Legislature`, `Executive`, `NationalGuard`, `FederalAgency`, `FinancialRegulator`, `MediaRegulator`, `Custom = "..."` |
| `loyalty` | f64 | `[0,1]` toward its parent faction |
| `effectiveness` | f64 | `[0,1]` |
| `personnel` | u64 | |
| `fracture_threshold` | f64? | If loyalty drops below this, the institution may defect |

### `[factions.<id>.forces.<force_id>]`

A deployable force unit.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `unit_type` | enum | See below |
| `region` | string | Starting region id |
| `strength` | f64 | Combat strength |
| `mobility` | f64 | Movement speed |
| `upkeep` | f64 | Resources consumed per tick |
| `morale_modifier` | f64 | Added to faction morale baseline |
| `capabilities` | `[UnitCapability]` | See below |
| `force_projection` | table? | See below |

`unit_type` variants: `Infantry`, `Mechanized`, `Armor`, `Artillery`, `AirSupport`, `Naval`, `SpecialOperations`, `CyberUnit`, `DroneSwarm`, `LawEnforcement`, `Militia`, `Logistics`, `AirDefense`, `ElectronicWarfare`, `Custom = "..."`.

#### `force_projection`

Tagged enum (`mode = "..."`): `Airlift { capacity }`, `Naval { range }`, `StandoffStrike { range, damage }`.

#### `capabilities`

Each entry is a tagged enum (`type = "..."`):

| Variant | Fields |
|---|---|
| `Garrison` | — |
| `Raid` | — |
| `Sabotage` | `effectiveness` |
| `Recon` | `range`, `detection` |
| `Interdiction` | `range` |
| `AreaDenial` | `radius` |
| `CounterUAS` | `effectiveness` |
| `EW` | `jamming_range`, `effectiveness` |
| `Cyber` | `attack`, `defense` |
| `InfoOps` | `reach`, `persuasion` |
| `Humanitarian` | `capacity` |

### `[factions.<id>.recruitment]`

| Field | Type |
|---|---|
| `rate` | f64 |
| `population_threshold` | f64 |
| `unit_type` | enum (see above) |
| `base_strength` | f64 |
| `cost` | f64 |

### `diplomacy`

```toml
diplomacy = [
  { target_faction = "bravo", stance = "Hostile" },
]
```

`stance` variants: `War`, `Hostile`, `Neutral`, `Cooperative`, `Allied`.

---

## `[technology.<tech_id>]`

Tech cards are named bundles of statistical effects.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `description` | string | Should cite OSINT source |
| `category` | enum | `Surveillance`, `OffensiveDrone`, `CounterDrone`, `ElectronicWarfare`, `Cyber`, `Communications`, `InformationWarfare`, `Concealment`, `Logistics`, `Custom = "..."` |
| `effects` | `[TechEffect]` | See below |
| `cost_per_tick` | f64 | Resource drain while deployed |
| `deployment_cost` | f64 | One-shot cost |
| `countered_by` | `[string]` | Tech ids that suppress this one |
| `terrain_modifiers` | `[TerrainTechModifier]` | Terrain-specific effectiveness |
| `coverage_limit` | u32? | Max simultaneous deployments |

### `effects`

Each entry is a tagged enum (`type = "..."`):

| Variant | Fields |
|---|---|
| `DetectionModifier` | `factor` |
| `CombatModifier` | `factor` |
| `InfraProtection` | `factor` |
| `MoraleEffect` | `target` (`Own`, `Enemy`, `Civilian`, `All`), `delta` |
| `AreaDenial` | `strength` |
| `CommsDisruption` | `factor` |
| `AttritionModifier` | `factor` |
| `CivilianSentiment` | `delta` |
| `SupplyInterdiction` | `factor` |
| `IntelGain` | `probability` |
| `CounterTech` | `target` (tech id), `reduction` |

### `terrain_modifiers`

```toml
terrain_modifiers = [
  { terrain = "Urban", effectiveness = 0.6 },
  { terrain = "Forest", effectiveness = 0.3 },
]
```

---

## `[political_climate]`

| Field | Type | Notes |
|---|---|---|
| `tension` | f64 | `[0,1]` — internal engine variable, not displayed in UI |
| `institutional_trust` | f64 | `[0,1]` |
| `media_landscape` | table | See below |
| `population_segments` | `[PopulationSegment]` | See below |
| `global_modifiers` | `[ClimateModifier]` | See below |

### `[political_climate.media_landscape]`

All `[0,1]`:

| Field | Description |
|---|---|
| `fragmentation` | How siloed media consumption is |
| `disinformation_susceptibility` | Population's exposure to false narratives |
| `state_control` | Government control of media |
| `social_media_penetration` | |
| `internet_availability` | |

### `population_segments`

Array of inline tables, each:

| Field | Type | Notes |
|---|---|---|
| `id` | string | Segment id |
| `name` | string | |
| `fraction` | f64 | `[0,1]` of total population |
| `concentrated_in` | `[string]` | Region ids |
| `sympathies` | `[{faction, sympathy}]` | Per-faction `[0,1]` |
| `activation_threshold` | f64 | Tension level that triggers action |
| `activation_actions` | `[CivilianAction]` | Actions when activated |
| `volatility` | f64 | `[0,1]` |
| `activated` | bool | Default `false` — usually omit |

#### `activation_actions`

Tagged enum (`action = "..."`):

| Variant | Fields |
|---|---|
| `NonCooperation` | `effectiveness_reduction` |
| `Protest` | `intensity` |
| `Intelligence` | `target_faction`, `quality` |
| `MaterialSupport` | `target_faction`, `rate` |
| `ArmedResistance` | `target_faction`, `unit_strength` |
| `Flee` | `rate` |
| `Sabotage` | `target_infra_type` (optional), `probability` |

### `global_modifiers`

Tagged enum (`modifier = "..."`):

| Variant | Fields |
|---|---|
| `EconomicCrisis` | `severity` |
| `NaturalDisaster` | `region`, `severity` |
| `InternationalPressure` | `target_faction`, `intensity` |
| `HealthCrisis` | `severity` |
| `ElectionCycle` | `legitimacy_modifier` |

---

## `[events.<event_id>]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `description` | string | |
| `earliest_tick` | u32? | Earliest tick the event can fire |
| `latest_tick` | u32? | Latest tick the event can fire |
| `conditions` | `[EventCondition]` | All must hold |
| `probability` | f64 | `[0,1]` per-tick fire chance once eligible |
| `repeatable` | bool | If false, fires at most once |
| `effects` | `[EventEffect]` | Applied when the event fires |
| `chain` | string? | Event id to trigger immediately afterward |

Event chains are validated for cycles when the engine starts.

### `conditions`

Tagged enum (`condition = "..."`):

| Variant | Fields |
|---|---|
| `RegionControl` | `region`, `faction`, `controlled` |
| `TensionAbove` / `TensionBelow` | `threshold` |
| `FactionStrengthAbove` / `FactionStrengthBelow` | `faction`, `threshold` |
| `MoraleAbove` / `MoraleBelow` | `faction`, `threshold` |
| `InstitutionLoyaltyBelow` | `institution`, `threshold` |
| `InfraStatusBelow` | `infra`, `threshold` |
| `EventFired` | `event`, `fired` |
| `TickAtLeast` | `tick` |
| `SegmentActivated` | `segment` |
| `Expression` | `expr` (free-form expression) |

### `effects`

Tagged enum (`effect = "..."`):

| Variant | Fields |
|---|---|
| `DamageInfra` | `infra`, `damage` |
| `MoraleShift` | `faction`, `delta` |
| `LoyaltyShift` | `institution`, `delta` |
| `InstitutionDefection` | `institution`, `to_faction` |
| `SpawnUnits` | `faction`, `units` (array of `ForceUnit`) |
| `DestroyUnits` | `faction`, `region`, `damage` |
| `DiplomacyChange` | `faction_a`, `faction_b`, `new_stance` |
| `TensionShift` | `delta` |
| `SympathyShift` | `segment`, `faction`, `delta` |
| `TechAccess` | `faction`, `tech`, `grant` |
| `MediaEvent` | `narrative`, `credibility`, `reach`, `favors` (faction, optional) |
| `ResourceChange` | `faction`, `delta` |
| `Narrative` | `text` |

---

## `[simulation]`

| Field | Type | Notes |
|---|---|---|
| `max_ticks` | u32 | Hard cap on simulation length |
| `tick_duration` | enum | `Hours = N`, `Days = N`, or `Weeks = N` |
| `monte_carlo_runs` | u32 | Default for `--monte-carlo` mode |
| `seed` | u64? | Optional fixed RNG seed |
| `fog_of_war` | bool | Enable per-faction visibility model |
| `attrition_model` | enum | See below |
| `snapshot_interval` | u32 | Ticks between state snapshots |

### `attrition_model`

```toml
[simulation.attrition_model]
Stochastic = { noise = 0.1 }
```

Variants: `LanchesterLinear`, `LanchesterSquare`, `Hybrid`, `Stochastic { noise }`.

`Stochastic.noise` is the relative standard deviation of casualty rolls (`0.1` ≈ ±10%). The first three variants are deterministic given the same state.

---

## `[victory_conditions.<id>]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | |
| `faction` | string | Faction id this condition belongs to |
| `condition` | tagged enum | See below |

### `condition`

```toml
[victory_conditions.alpha_control.condition]
type = "StrategicControl"
threshold = 0.75
```

| Variant | Fields |
|---|---|
| `StrategicControl` | `threshold` (fraction of strategic value held) |
| `MilitaryDominance` | `enemy_strength_below` |
| `HoldRegions` | `regions` (array), `duration` (ticks) |
| `InstitutionalCollapse` | `trust_below` |
| `PeaceSettlement` | — |
| `Custom` | `variable`, `threshold`, `above` |

---

## Determinism guarantees

Given the same scenario file and the same `seed`, the engine produces bit-identical results on native and WASM. Two practical consequences:

- The engine uses `ChaCha8Rng` and `BTreeMap` everywhere — never `HashMap`.
- Floating-point operations are kept simple and platform-agnostic. Avoid relying on transcendentals you cannot reproduce.

If you depend on deterministic output, set `seed` explicitly. If `seed` is omitted, runs are still reproducible within a single Monte Carlo batch (the runner derives sub-seeds), but two batches may differ.

---

## See also

- [`scenarios/tutorial_symmetric.toml`](../scenarios/tutorial_symmetric.toml) — minimal working example
- [`scenarios/tutorial_asymmetric.toml`](../scenarios/tutorial_asymmetric.toml) — events, tech, segments, fog of war
- [`scenarios/us_institutional_fracture.toml`](../scenarios/us_institutional_fracture.toml) — full 4-faction example
- [`crates/faultline-types/src/`](../crates/faultline-types/src/) — canonical Rust definitions
- [LEGAL.md](../LEGAL.md) — sourcing and export-control policy
