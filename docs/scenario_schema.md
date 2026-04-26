# Scenario Schema Reference

A Faultline scenario is a TOML file describing a simulated conflict. This document is the authoritative reference for every section, field, and enum variant the engine accepts.

The canonical Rust definitions live in `crates/faultline-types/src/`. If this document and the source ever disagree, the source wins — but please file an issue so the docs can be fixed.

> **Sourcing requirement.** Every numeric parameter in a scenario must be derivable from publicly available open-source intelligence. See [LEGAL.md](../LEGAL.md) for details.

## Top-level layout

```toml
# Optional scenario-level budget caps — must appear before the first
# section header so TOML does not attach them to [meta].
attacker_budget = 250_000.0
defender_budget = 50_000_000.0

[meta]              # name, description, author, version, tags
[map]               # source, regions, infrastructure, terrain
[[environment.windows]]    # optional weather / time-of-day windows (Epic D)
[factions.<id>]     # one table per faction
[technology.<id>]   # one table per tech card (may be empty)
[political_climate] # tension, trust, media, segments, modifiers
[events.<id>]       # one table per event (may be empty)
[kill_chains.<id>]  # multi-phase campaigns (may be empty)
[simulation]        # max_ticks, tick_duration, seed, attrition
[victory_conditions.<id>]  # one table per victory condition
```

All map keys (`<id>`) are strings. The engine uses `BTreeMap` everywhere, so iteration order is alphabetical and deterministic — pick IDs that sort sensibly if order matters for debugging.

The `attacker_budget` / `defender_budget` scenario-level fields cap the total dollar spend accumulated across all kill-chain phases. Phases whose per-phase costs would exceed the budget cannot activate and are marked `Failed`. See [`[kill_chains.<id>]`](#kill_chainsid-multi-phase-campaigns) below.

---

## `[meta]`

Free-form descriptive metadata. None of these fields affect simulation outcomes.

| Field | Type | Description |
|---|---|---|
| `schema_version` | u32 | Faultline schema version this scenario was authored against. Defaults to `1` when absent. See [Schema evolution](#schema-evolution) |
| `name` | string | Human-readable scenario name |
| `description` | string | What the scenario models. Multi-line OK |
| `author` | string | Scenario author handle |
| `version` | string | Semver-style version string for the scenario itself (distinct from `schema_version`) |
| `tags` | `[string]` | Free-form tags for indexing |
| `confidence` | enum? | Optional coarse confidence tag (`high` / `medium` / `low`). Signals "publication-ready" vs. "conceptual sketch" to report readers |

### Schema evolution

`meta.schema_version` is the authoritative version of the scenario *format* this file was authored for. It is distinct from `version`, which is the author's own semver for the scenario's content.

The current schema version is **1**. The migration framework lives in `crates/faultline-types/src/migration.rs`; both the CLI and the WASM frontend route their scenario loading through it.

**For scenario authors:**

- The field defaults to `1` when omitted, so existing scenarios load unchanged.
- The CLI prints a warning when it loads a scenario authored against an older schema. Persist the upgraded form with:
  ```
  faultline scenarios/foo.toml --migrate --in-place
  ```
- A scenario authored against a *newer* version than the build supports is rejected with a clear error — upgrade Faultline rather than risk a silent partial parse.

**For schema changes (engine work):**

The policy is encoded in `migration.rs` and enforced by the `verify-migration` CI stage:

1. **Additive fields** (new optional field with a sensible default) ship as `#[serde(default)]` and do *not* require a schema bump. Existing scenarios continue to load.
2. **Breaking changes** (rename, remove, change semantics, change required-ness) require:
   - Bumping `CURRENT_SCHEMA_VERSION` in `migration.rs`.
   - Appending a `MigrationStep` whose `apply` function rewrites the `toml::Value` from the old shape to the new shape.
   - Updating bundled scenarios under `scenarios/` to the new version (run `--migrate --in-place` against each one).
   - Adding a fixture that pins the old shape so the migrator's correctness is testable in perpetuity.
3. CI runs `tools/ci/verify-migration.sh` on every PR, which migrates each bundled scenario and re-validates the result. Drift fails the build.

**Interaction with Epic Q (manifest hashes):** bumping `CURRENT_SCHEMA_VERSION` changes the canonical JSON shape of `Scenario`, which changes `scenario_hash` for every bundled scenario. Epic Q manifests emitted before a schema bump cannot be replayed against post-bump builds — `--verify` fails with a clear "scenario hash mismatch" error. This is the intended behavior; an analyst citing a stable Faultline run by manifest needs to pin the engine version anyway.

**`--migrate` formatting caveat:** the CLI's `--migrate` flag emits the canonical TOML form of the parsed scenario — keys are BTreeMap-sorted, multi-line strings are collapsed, and comments are stripped. Authorial formatting is not preserved. For scenarios where formatting matters, run `--migrate` to a temp file, diff it against the source, and apply the diff by hand rather than using `--migrate --in-place`.

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

## `[[environment.windows]]` (Epic D — weather, time-of-day)

Optional global timeline of environmental windows that modify per-region terrain effects and a global kill-chain detection multiplier. Empty by default — scenarios with no environmental modeling pay zero overhead and the engine's per-tick environment lookup collapses to a `1.0` multiplier.

```toml
[[environment.windows]]
id = "monsoon"
name = "Monsoon Storm"
activation = { type = "TickRange", start = 30, end = 60 }
applies_to = ["Mountain", "Forest"]
defense_factor = 1.4
visibility_factor = 0.5
detection_factor = 0.7

[[environment.windows]]
id = "night"
name = "Night Cycle"
activation = { type = "Cycle", period = 24, phase = 18, duration = 12 }
detection_factor = 0.6
```

| Field | Type | Notes |
|---|---|---|
| `id` | string | Stable identifier surfaced in reports |
| `name` | string | Human-readable label |
| `activation` | enum | When the window is active (see below) |
| `applies_to` | `[TerrainType]` | Empty = applies to every terrain. Filters the per-terrain factors only |
| `movement_factor` | f64 | Multiplier on `terrain.movement_modifier` (default 1.0) |
| `defense_factor` | f64 | Multiplier on `terrain.defense_modifier` (default 1.0) — read by combat |
| `visibility_factor` | f64 | Multiplier on `terrain.visibility` (default 1.0) |
| `detection_factor` | f64 | Multiplier applied **globally** to every kill-chain phase's per-tick detection probability (default 1.0). Not gated by `applies_to` since kill chains are faction-vs-faction without a region |

Multiple active windows compose multiplicatively, so a scenario can stack a daily night cycle with an event-driven storm window. Negative or non-finite factors are rejected at validation.

### `Activation` variants

| `type` | Extra fields | Notes |
|---|---|---|
| `Always` | — | Active on every tick |
| `TickRange` | `start` (u32), `end` (u32) | Active when `start <= tick <= end` (inclusive) |
| `Cycle` | `period`, `phase`, `duration` (u32) | Active when `(tick - phase) mod period < duration`. Useful for time-of-day cycles under hourly ticks (`period = 24`, `phase = 18`, `duration = 12` is night running 18:00–06:00) |

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
| `escalation_rules` | table? | _Optional._ Declarative doctrine / ROE ladder (see Epic B) |
| `defender_capacities` | table? | _Optional._ Per-role investigative-queue model (see Epic K) — keyed by `[factions.<id>.defender_capacities.<role_id>]` |
| `leadership` | table? | _Optional._ Leadership cadre with named ranks + succession (see Epic D) |

### `[factions.<id>.escalation_rules]` (Epic B)

Scenario-author-asserted escalation ladder. Purely declarative in the
current engine — surfaced in the **Policy Implications** report
section so analysts can see which counterfactuals implicitly require
crossing a doctrinal threshold. The engine does not currently enforce
the ladder when selecting actions.

| Field | Type | Notes |
|---|---|---|
| `posture` | string | One-line summary of the faction's ROE stance |
| `ladder` | `[EscalationRung]` | Ordered low-to-high; each rung defines permitted / prohibited actions |
| `de_escalation_floor` | f64? | Tension at/above which the faction will not voluntarily de-escalate without an external trigger |

Each `ladder` rung:

| Field | Type | Notes |
|---|---|---|
| `id` | string | e.g. `"grey_zone"`, `"kinetic"`, `"strategic"` |
| `name` | string | |
| `description` | string | |
| `trigger_tension` | f64? | Tension at/above which the rung is authorized; `None` = always authorized |
| `permitted_actions` | `[string]` | Free-text descriptions of permitted capabilities |
| `prohibited_actions` | `[string]` | Explicit red lines |

### `[factions.<id>.leadership]` (Epic D)

Optional leadership cadre — named ranks (top of chain first) plus succession parameters. Drives the `PhaseOutput::LeadershipDecapitation` mechanic: a successful decapitation phase advances the rank index, applies a one-shot morale shock, and caps the faction's runtime morale at the new rank's effectiveness × `succession_floor` during the recovery ramp. When the rank index passes the end of `ranks` the faction is "leaderless": effectiveness collapses to `0.0` and morale is floored there until the run ends. `None` / absent = legacy behavior; the faction has no decapitation surface and `LeadershipDecapitation` outputs against it are no-ops (other than incrementing the strike counter for analytics).

```toml
[factions.bravo.leadership]
succession_recovery_ticks = 6
succession_floor = 0.4

[[factions.bravo.leadership.ranks]]
id = "principal"
name = "Principal"
effectiveness = 1.0

[[factions.bravo.leadership.ranks]]
id = "deputy"
name = "Deputy"
effectiveness = 0.5
```

| Field | Type | Notes |
|---|---|---|
| `ranks` | `[LeadershipRank]` | Ordered top-of-chain first. Must contain at least one entry |
| `succession_recovery_ticks` | u32 | Number of ticks the recovery ramp lasts after a decapitation. `0` disables the ramp (a successor reaches full effectiveness immediately) |
| `succession_floor` | f64 | Multiplier on the new rank's effectiveness on the strike tick; linearly interpolates to `1.0` over `succession_recovery_ticks`. Default `0.5` |

Each `ranks` entry:

| Field | Type | Notes |
|---|---|---|
| `id` | string | Stable identifier (e.g. `"principal"`, `"deputy"`) |
| `name` | string | |
| `effectiveness` | f64 | `[0,1]` multiplicative scalar; top is conventionally `1.0` and successors lower |
| `description` | string | _Optional._ |

Phase-side hookup:

```toml
[[kill_chains.alpha.phases.alpha_strike.outputs]]
type = "LeadershipDecapitation"
target_faction = "bravo"
morale_shock = 0.2
```

The `morale_shock` is a one-time clamp-respecting drop applied on the strike tick before the leadership cap clamps morale further; `0.0` disables the shock and lets the leadership cap do all the work.

### `[factions.<id>.defender_capacities.<role_id>]` (Epic K)

Defender-side capacity model: a per-role investigative queue that
constrains how fast this faction can process incoming alerts / tips /
forensic work. When kill-chain phases reference roles via
`gated_by_defender` or `defender_noise`, the engine maintains
deterministic FIFO queues, services them at the declared per-tick
rate, and applies the `saturated_detection_factor` penalty when a
queue is at capacity. Empty / absent = legacy infinite-capacity
assumption.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the inner table key |
| `name` | string | |
| `description` | string | _Optional._ |
| `queue_depth` | u32 | Capacity threshold. Under `DropNew` / `DropOldest` the queue caps here; under `Backlog` the queue can grow past it but `is_saturated()` still fires from this depth onward |
| `service_rate` | f64 | Mean items serviced per tick; fractional rates accumulate (`0.5` = one item every two ticks) |
| `overflow` | `"DropNew"` / `"DropOldest"` / `"Backlog"` | Behavior when an enqueue would exceed `queue_depth`. Default: `"DropNew"` |
| `saturated_detection_factor` | f64 | Multiplier applied to phases gated by this role when the queue is saturated. `1.0` = no penalty; published SOC alert-fatigue literature reports `0.2`–`0.5`. Default: `1.0` |

Phase-side hookups:

```toml
# This phase floods the tier-1 queue while it's active.
[[kill_chains.flooded_soc.phases.noisy_enumeration.defender_noise]]
defender = "blue_soc"
role = "tier1_alerts"
items_per_tick = 60.0

# This phase's detection roll is suppressed while tier-1 is saturated.
gated_by_defender = { faction = "blue_soc", role = "tier1_alerts" }
```

Per-tick order is **arrive → assess → service**: a phase enqueues
its noise, the detection roll reads the post-arrival depth, and the
queue is serviced at end-of-tick. That ordering preserves saturation
through the current tick's detection rolls — which is what reproduces
the alert-fatigue effect when a sequential phase 2 inherits the
backlog phase 1 created.

References to undeclared `(faction, role)` pairs are rejected at load
time with `ScenarioError::UnknownDefenderRole`. See
`scenarios/alert_fatigue_soc.toml` for the bundled archetype.

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
| `defender_options` | `[DefenderOption]` | _Optional._ Declarative counterfactual defender responses surfaced in the **Policy Implications** report section |

Event chains are validated for cycles when the engine starts.

#### `DefenderOption` (Epic B)

Declarative alternative the defender could take if the event fires.
Not auto-selected by the engine — present so analysts can enumerate
the options and activate one via `--counterfactual` in a future
iteration.

| Field | Type | Notes |
|---|---|---|
| `key` | string | Stable identifier referenced by counterfactual overrides |
| `name` | string | |
| `description` | string | |
| `preparedness_cost` | f64 | Dollar cost of holding the response at readiness |
| `modifier_effects` | `[EventEffect]` | Effects that *replace* the event's default `effects` when the option is active. Empty = cancels the event. |

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

## `[kill_chains.<id>]` — multi-phase campaigns

A kill chain is an ordered, branching sequence of [`CampaignPhase`](#kill_chainsidphasesphase_id) entries modeling an adversary campaign against a target faction. Execution begins at `entry_phase`; subsequent phases are reached by resolving branches at phase completion. The phase graph must terminate — a phase with no branches ends the chain.

Kill chains are the primary analytical signal for structured defensive wargaming: Monte Carlo runs aggregate per-phase success / failure / detection probabilities, cost asymmetry ratios, attribution confidence, and defensive-domain seam exploitation into the scenario's feasibility matrix and Markdown report.

```toml
[kill_chains.alpha]
id = "alpha"
name = "Campaign Alpha — Intelligence-Led Pressure"
description = "Patient pressure-campaign archetype."
attacker = "insider_cell"
target = "federal_security"
entry_phase = "alpha_sensor_emplacement"
```

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the table key |
| `name` | string | Human-readable campaign name |
| `description` | string | Free-form; describe the archetype, not tradecraft |
| `attacker` | faction id | Faction executing the campaign |
| `target` | faction id | Faction being targeted |
| `entry_phase` | phase id | Phase graph entry point |
| `phases` | table | One entry per `[kill_chains.<id>.phases.<phase_id>]` |

### `[kill_chains.<id>.phases.<phase_id>]`

A single phase. Each active tick rolls independently for detection (accumulating exposure); at completion the phase rolls success against `base_success_probability` modified by intelligence gained from prerequisite phases.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Must equal the inner table key |
| `name` | string | |
| `description` | string | |
| `prerequisites` | `[phase_id]` | Prior phases that must succeed first (default: `[]`) |
| `base_success_probability` | f64 | In `[0, 1]` |
| `min_duration` | u32 | Minimum active ticks |
| `max_duration` | u32 | Maximum active ticks |
| `detection_probability_per_tick` | f64 | Per-active-tick detection roll (default: `0.0`) |
| `prerequisite_success_boost` | f64 | Additive boost applied per successful prerequisite (default: `0.0`) |
| `attribution_difficulty` | f64 | `0.0` = trivially attributable, `1.0` = opaque (default: `0.5`) |
| `cost` | `PhaseCost` | See below (default: zero) |
| `targets_domains` | `[DefensiveDomain]` | Domains whose seams this phase exploits |
| `outputs` | `[PhaseOutput]` | Effects applied on success |
| `branches` | `[PhaseBranch]` | Next-phase transitions |
| `parameter_confidence` | `"High"` / `"Medium"` / `"Low"` | Optional author self-assessment of how defensible this phase's base rates, detection probability, and attribution difficulty are. Omit for "unrated." Distinct from the Monte Carlo-derived confidence in the feasibility matrix, which reflects *sampling* stability — `parameter_confidence` reflects *parameter* defensibility. Phases tagged `Low` are listed in a dedicated section of the generated Markdown report. |
| `warning_indicators` | `[WarningIndicator]` | _Optional._ IWI / IOC entries surfaced in the **Countermeasure Analysis** report section (Epic B) |
| `defender_noise` | `[DefenderNoise]` | _Optional._ Per-tick alert volume this phase generates against a named defender role's queue (Epic K) |
| `gated_by_defender` | `DefenderRoleRef` | _Optional._ When the named role's queue is at capacity, multiplies this phase's per-tick detection roll by the role's `saturated_detection_factor` (Epic K) |

### `WarningIndicator` (Epic B)

Observable the defender could monitor for to catch this phase before
completion. Currently declarative — `detection_probability_per_tick`
still drives the detection roll. The section makes the monitoring
posture required to hit that detection rate concrete.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Stable identifier, e.g. `"beaconing_rf_emissions"` |
| `name` | string | |
| `description` | string | |
| `observable` | enum | Collection discipline required to see it (see below) |
| `detectability` | f64 | `[0, 1]` — probability of catching the observable *if* the defender is looking |
| `time_to_detect_ticks` | u32? | Expected latency from phase activation to reliable detection |
| `monitoring_cost_annual` | f64? | Annual dollar cost of a monitoring posture covering this observable |

`observable` enum values: `SIGINT`, `HUMINT`, `OSINT`, `GEOINT`,
`MASINT`, `CYBINT`, `FININT`, `Physical`, or `Custom = "..."`.

### `PhaseCost`

```toml
[kill_chains.alpha.phases.alpha_sensor_emplacement.cost]
attacker_dollars = 500.0
defender_dollars = 4_000_000.0
attacker_resources = 0.5
confidence = "High"  # optional
```

| Field | Type | Notes |
|---|---|---|
| `attacker_dollars` | f64 | Accumulated against scenario-level `attacker_budget` |
| `defender_dollars` | f64 | Accumulated against scenario-level `defender_budget` |
| `attacker_resources` | f64 | Scenario-resource units consumed from the attacker's pool |
| `confidence` | `"High"` / `"Medium"` / `"Low"` | Optional author self-assessment of cost defensibility — `High` for commodity-parts BOMs / published rate cards, `Low` for wide expert estimates. Omit for "unrated." Complements `parameter_confidence` on the phase itself. |

The ratio `mean defender spend / mean attacker spend` across a Monte Carlo batch is the **cost asymmetry ratio** surfaced in `CampaignSummary.cost_asymmetry_ratio`, the feasibility matrix, and the generated Markdown report.

### `PhaseOutput`

A tagged enum of effects applied when a phase completes successfully. One `[[kill_chains.<id>.phases.<phase_id>.outputs]]` array-of-tables entry per effect.

| Variant | Fields | Notes |
|---|---|---|
| `IntelligenceGain` | `amount` (f64) | Boosts subsequent phases beyond `prerequisite_success_boost` |
| `InfraDamage` | `region` (id), `factor` (f64) | Damages infrastructure in the named region |
| `TensionDelta` | `delta` (f64) | Changes `political_climate.tension` |
| `MoraleDelta` | `faction` (id), `delta` (f64) | Changes faction morale |
| `InformationDominance` | `delta` (f64) | Non-kinetic accumulator |
| `InstitutionalErosion` | `delta` (f64) | Non-kinetic accumulator; also erodes institution loyalty proportionally |
| `CoercionPressure` | `delta` (f64) | Non-kinetic accumulator |
| `PoliticalCost` | `delta` (f64) | Non-kinetic accumulator |
| `Custom` | `key` (string), `value` (f64) | Generic analytical metric |
| `LeadershipDecapitation` | `target_faction` (id), `morale_shock` (f64, default `0.0`) | Advances the target's leadership rank index by 1, applies a one-shot morale drop, and caps morale at the new rank's effectiveness during the recovery ramp (Epic D). No-op against factions without a `leadership` cadre |

Each entry is written as:

```toml
[[kill_chains.alpha.phases.alpha_pressure_disclosure.outputs]]
type = "CoercionPressure"
delta = 0.45
```

### `PhaseBranch`

Branches are evaluated in declaration order; the first matching branch wins. A phase with no branches terminates the chain.

```toml
[[kill_chains.alpha.phases.alpha_sensor_emplacement.branches]]
condition = { type = "OnSuccess" }
next_phase = "alpha_pattern_of_life"

[[kill_chains.alpha.phases.alpha_sensor_emplacement.branches]]
condition = { type = "OnDetection" }
next_phase = "alpha_abort"
```

| `condition.type` | Extra fields | Notes |
|---|---|---|
| `OnSuccess` | — | Phase succeeded |
| `OnFailure` | — | Phase failed outright |
| `OnDetection` | — | Defender detected the operation while active |
| `Probability` | `p` (f64) | Independent roll against `p` |
| `Always` | — | Terminal fallback |
| `EscalationThreshold` | `metric`, `threshold` (f64), `direction`, `sustained_ticks` (u32) | Fires when a global metric has stayed on the requested side of `threshold` for `sustained_ticks` consecutive end-of-tick snapshots. Hysteresis is built in — a single-tick spike will not flip the branch (Epic C) |
| `OrAny` | `conditions` (array) | Fires when **any** inner condition matches. Short-circuit left-to-right (Epic D) |

#### `EscalationThreshold` (Epic C)

```toml
[[kill_chains.alpha.phases.alpha_recon.branches]]
condition = { type = "EscalationThreshold", metric = "Tension", threshold = 0.7, direction = "Above", sustained_ticks = 3 }
next_phase = "alpha_escalate"

[[kill_chains.alpha.phases.alpha_recon.branches]]
condition = { type = "Always" }
next_phase = "alpha_de_escalate"
```

Reads from a rolling history of escalation-relevant metrics that the engine captures at the end of every tick. The branch fires only when the predicate has held continuously across the requested window — useful for "if tension stays elevated for N ticks, the operation gets called off (or escalated)." The engine sizes the history buffer to the longest `sustained_ticks` any branch in the scenario asks for; scenarios with no `EscalationThreshold` branches pay zero overhead. The history-buffer walker recurses through `OrAny`, so an `EscalationThreshold` nested inside an `OrAny` correctly registers its window.

#### `OrAny` (Epic D)

```toml
[[kill_chains.alpha.phases.alpha_recon.branches]]
condition = { type = "OrAny", conditions = [
  { type = "OnDetection" },
  { type = "EscalationThreshold", metric = "Tension", threshold = 0.7, direction = "Above", sustained_ticks = 3 },
] }
next_phase = "alpha_abort"
```

Lets a single branch fire on any of several equivalent triggers without duplicating the `next_phase`. Inner conditions are evaluated short-circuit left-to-right. Any condition (including `OrAny` itself) may be nested. Engine validation rejects an empty `conditions` array — an empty OR is ambiguous between "vacuously false" and "unfilled author template" and would silently never match.

| Field | Type | Notes |
|---|---|---|
| `metric` | enum | One of `Tension` (reads `political_climate.tension`), `InformationDominance`, `InstitutionalErosion`, `CoercionPressure`, `PoliticalCost` (the four non-kinetic accumulators) |
| `threshold` | f64 | The metric's threshold value. Tension and the non-kinetic accumulators live in `[0, 1]`; `InformationDominance` is in `[-1, 1]` |
| `direction` | enum | `Above` (metric must be `>= threshold`) or `Below` (metric must be `<= threshold`) |
| `sustained_ticks` | u32 | Number of consecutive end-of-tick snapshots the predicate must hold over. `0` is treated as "currently on the right side." If the run hasn't been observed long enough (`sustained_ticks > current_tick`), the condition is false |

### `DefensiveDomain`

Categories of defensive discipline used for doctrinal-seam scoring. A phase with ≥2 domains in `targets_domains` is counted as cross-domain; the share of successful cross-domain phases appears in `MonteCarloSummary.seam_scores`.

Variants: `PhysicalSecurity`, `NetworkSecurity`, `CounterUAS`, `ExecutiveProtection`, `CivilianEmergency`, `SignalsIntelligence`, `InsiderThreat`, `SupplyChainSecurity`, `Custom(<string>)`.

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
| `NonKineticThreshold` | `metric`, `threshold` |
| `Custom` | `variable`, `threshold`, `above` |

`NonKineticThreshold.metric` accepts the same identifiers as the non-kinetic accumulators emitted by kill-chain `PhaseOutput`: `InformationDominance`, `InstitutionalErosion`, `CoercionPressure`, `PoliticalCost`. The condition fires when the target metric crosses `threshold`.

---

## `[strategy_space]` (Epic H — strategy search)

Optional. Declares which scenario parameters are *decision variables* for the `--search` CLI mode. Each variable names a parameter via the same dotted path layer used by `--counterfactual` and `--sensitivity`, plus a domain to sample from. When present, `faultline-cli ... --search` walks the declared space and reports best-by-objective + Pareto-frontier results; when absent, the field is omitted from the serialized scenario entirely so legacy bundles stay byte-identical.

```toml
[strategy_space]

[[strategy_space.variables]]
path = "faction.alpha.initial_morale"
owner = "alpha"          # optional; surfaces grouping in the report

[strategy_space.variables.domain]
kind = "continuous"
low = 0.5
high = 0.9
steps = 4                # grid mode emits 4 evenly-spaced values; ignored in random mode

[[strategy_space.variables]]
path = "kill_chain.exfil.phase.move.detection_probability_per_tick"

[strategy_space.variables.domain]
kind = "discrete"
values = [0.05, 0.10, 0.20]

[[strategy_space.objectives]]
metric = "maximize_win_rate"
faction = "alpha"

[[strategy_space.objectives]]
metric = "minimize_detection"
```

`Domain` variants:

| `kind` | Fields | Random sampling | Grid sampling |
|---|---|---|---|
| `continuous` | `low`, `high`, `steps` | uniform draw in `[low, high)` | `steps` evenly-spaced values inclusive of both endpoints; `steps == 1` uses the midpoint |
| `discrete` | `values` (non-empty array of `f64`) | uniform pick | enumerates each value |

Validation rejects: empty `path`, duplicate paths, `low > high`, `steps == 0`, empty discrete `values`, non-finite bounds, unknown `owner` factions, and unknown `MaximizeWinRate.faction` references — all at scenario load time.

Built-in `SearchObjective` variants (round one):

| `metric` | Direction | Argument | Source field |
|---|---|---|---|
| `maximize_win_rate` | max | `faction` | `MonteCarloSummary.win_rates[faction]` |
| `minimize_detection` | min | — | `max(CampaignSummary.detection_rate)` over chains |
| `minimize_attacker_cost` | min | — | sum of `CampaignSummary.mean_attacker_spend` |
| `maximize_cost_asymmetry` | max | — | `max(CampaignSummary.cost_asymmetry_ratio)` over chains |
| `minimize_duration` | min | — | `MonteCarloSummary.average_duration` |

Pareto frontier semantics: a trial is *dominated* iff some other trial is at least as good on every objective (direction-aware) and strictly better on at least one. Returned `pareto_indices` are sorted ascending. `best_by_objective` ties resolve by lowest trial index for reproducibility.

The search layer uses two independent seeds — `search_seed` drives assignment sampling, `mc_config.seed` drives the inner Monte Carlo evaluation — so search-then-evaluate is bit-identical under fixed inputs and trial-to-trial deltas reflect parameter changes only, not sampling noise. See `crates/faultline-stats/src/search.rs` for the determinism contract.

---

## Determinism guarantees

Given the same scenario file and the same `seed`, the engine produces bit-identical results on native and WASM. Two practical consequences:

- The engine uses `ChaCha8Rng` and `BTreeMap` everywhere — never `HashMap`.
- Floating-point operations are kept simple and platform-agnostic. Avoid relying on transcendentals you cannot reproduce.

If you depend on deterministic output, set `seed` explicitly. If `seed` is omitted, runs are still reproducible within a single Monte Carlo batch (the runner derives sub-seeds), but two batches may differ.

---

## See also

**Tutorial scenarios:**
- [`scenarios/tutorial_symmetric.toml`](../scenarios/tutorial_symmetric.toml) — minimal working example; two equal factions on a 2×2 grid, pure Lanchester attrition
- [`scenarios/tutorial_asymmetric.toml`](../scenarios/tutorial_asymmetric.toml) — events, tech cards, population segments, fog of war

**Full multi-faction scenarios:**
- [`scenarios/us_institutional_fracture.toml`](../scenarios/us_institutional_fracture.toml) — 4-faction institutional crisis across 8 US macro-regions
- [`scenarios/europe_eastern_flank.toml`](../scenarios/europe_eastern_flank.toml) — NATO / Russia / Ukraine on the bundled Europe map; drone-swarm tech cards
- [`scenarios/drone_swarm_destabilization.toml`](../scenarios/drone_swarm_destabilization.toml) — multi-phase autonomous drone swarm campaign exercising sensor emplacement through coercion
- [`scenarios/capabilities_demo.toml`](../scenarios/capabilities_demo.toml) — sandbox exercising every tech card in the bundled Drone Threat Library

**Kill-chain wargames:**
- [`scenarios/compound_kill_chains.toml`](../scenarios/compound_kill_chains.toml) — three concurrent archetypal red-team campaigns (intelligence-led pressure, non-lethal capability demonstration, cyber-physical convergence) against a notional integrated defender
- [`scenarios/persistent_covert_surveillance.toml`](../scenarios/persistent_covert_surveillance.toml) — six-phase long-dwell commodity-sensor campaign against a notional federal protective posture
- [`scenarios/europe_energy_sabotage.toml`](../scenarios/europe_energy_sabotage.toml) — four-phase cross-border campaign against European ENTSO-E / Baltic subsea energy corridors

**Source references:**
- [`crates/faultline-types/src/`](../crates/faultline-types/src/) — canonical Rust definitions
- [LEGAL.md](../LEGAL.md) — sourcing and export-control policy
