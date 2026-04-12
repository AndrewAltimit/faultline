/**
 * Bundled technology card library derived from the Locust ETRA
 * threat assessment (v2.0, 2026-04-11).
 *
 * Each card captures the aggregate statistical effect of a real-world
 * capability described in the ETRA. Parameters are derived from the
 * ETRA's feasibility assessments, cost-of-defense analysis (Section
 * 4.7), and technology readiness reference (Appendix A). No card
 * encodes implementation-level technical detail — only the operational
 * effect on the simulation.
 *
 * Card fields are directly compatible with the Faultline TOML schema;
 * see `crates/faultline-types/src/tech.rs` for the authoritative type
 * definitions.
 *
 * Provenance fields (`etra_ref`, `trl`, `profiles`) are metadata only
 * and are stripped before injection into a scenario.
 */

/**
 * @typedef {object} TechCardSpec
 * @property {string} id
 * @property {string} name
 * @property {string} description
 * @property {'OffensiveDrone'|'Surveillance'|'CounterDrone'|'ElectronicWarfare'|'Cyber'|'Communications'|'InformationWarfare'|'Concealment'|'Logistics'|{Custom:string}} category
 * @property {Array<object>} effects      // TechEffect[]
 * @property {number} cost_per_tick       // operational spend per tick
 * @property {number} deployment_cost     // acquisition cost
 * @property {number} [coverage_limit]
 * @property {Array<string>} countered_by
 * @property {boolean} is_offensive       // UI grouping only
 * @property {string} etra_ref            // ETRA section reference
 * @property {string} trl                 // technology readiness level range
 * @property {Array<string>} profiles     // ETRA threat actor profiles ("A", "B", "C", "D") or defender role
 * @property {string} rationale           // one-line explanation of the mapping from capability → game effect
 */

/** @type {Object<string, TechCardSpec>} */
export const TECH_LIBRARY = {
  // ====================================================================
  // OFFENSIVE — Drone platforms and operational capabilities
  // ====================================================================

  autonomous_drone_swarm: {
    id: 'autonomous_drone_swarm',
    name: 'Autonomous Drone Swarm (50–100 platforms)',
    description:
      'Edge-AI coordinated micro/mini drones with onboard target recognition. RF-silent during mission; defeats control-link jamming.',
    category: 'OffensiveDrone',
    cost_per_tick: 0.3,
    deployment_cost: 75.0,
    coverage_limit: 3,
    effects: [
      { type: 'CombatModifier', factor: 1.6 },
      { type: 'AttritionModifier', factor: 1.3 },
      { type: 'DetectionModifier', factor: 0.55 },
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.08 },
    ],
    countered_by: ['hpm_area_effect', 'multi_phenom_c_uas'],
    is_offensive: true,
    etra_ref: 'Section 3.1, 3.2; Appendix A (Edge AI, multi-drone coordination)',
    trl: '7–8 (2026) → 9 (2030)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Section 3.1 places single-attempt success at 5–15% with high consequence severity; encoded as a combat/attrition multiplier combined with a detection penalty and enemy-morale shock.',
  },

  micro_drone_swarm: {
    id: 'micro_drone_swarm',
    name: 'Micro-Drone Swarm (<15g, sub-RCS)',
    description:
      'Insect-scale drones below the 0.01 m² radar cross-section floor. Acoustically masked in crowd noise; essentially undetectable by deployed C-UAS.',
    category: 'OffensiveDrone',
    cost_per_tick: 0.1,
    deployment_cost: 25.0,
    coverage_limit: 2,
    effects: [
      { type: 'DetectionModifier', factor: 0.15 },
      { type: 'IntelGain', probability: 0.25 },
      { type: 'CombatModifier', factor: 1.1 },
    ],
    countered_by: ['distributed_ir_sensor_net'],
    is_offensive: true,
    etra_ref: 'Section 4.3 (The Micro-Drone Gap)',
    trl: '4–5 (2026) → 6–7 (2030)',
    profiles: ['B'],
    rationale:
      'Section 4.3: no deployed detector reliably finds micro-drones. Encoded as a near-zero detection factor plus modest intel and combat bonuses.',
  },

  rogue_ap_drone_mesh: {
    id: 'rogue_ap_drone_mesh',
    name: 'Perched Rogue-AP Drone Mesh',
    description:
      'Drones transit to rooftop perch positions, power down motors, and operate as stationary rogue Wi-Fi access points. Dormant-until-activated via human spotter.',
    category: { Custom: 'CyberPhysical' },
    cost_per_tick: 0.08,
    deployment_cost: 18.0,
    coverage_limit: 4,
    effects: [
      { type: 'IntelGain', probability: 0.35 },
      { type: 'CommsDisruption', factor: 0.6 },
      { type: 'DetectionModifier', factor: 0.2 },
    ],
    countered_by: ['wids_cuas_fusion', 'cert_pinning_mdm'],
    is_offensive: true,
    etra_ref: 'Section 3.6 (Scenario 6 — Cyber-Physical Network Exploitation)',
    trl: '8–9 (2026)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Sections 3.6/4.5 give 60–80% credential-capture success over a 4-week campaign. Encoded as high IntelGain + partial CommsDisruption; low detection because the drone is stationary during exploitation.',
  },

  covert_sensor_emplacement: {
    id: 'covert_sensor_emplacement',
    name: 'Drone-Delivered Covert Sensor Nodes',
    description:
      'Solar-sustained ESP32+LTE sensor packages emplaced on rooftops or ground-level utility panels. Operate indefinitely with no maintenance.',
    category: 'Surveillance',
    cost_per_tick: 0.02,
    deployment_cost: 4.0,
    coverage_limit: 10,
    effects: [
      { type: 'IntelGain', probability: 0.45 },
      { type: 'DetectionModifier', factor: 0.1 },
    ],
    countered_by: ['rooftop_inspection_program', 'device_verification_training'],
    is_offensive: true,
    etra_ref: 'Section 3.7 (Scenario 7 — Persistent Covert Sensor Emplacement)',
    trl: '9 (2026)',
    profiles: ['A', 'B', 'C', 'D'],
    rationale:
      'Section 3.7 places steady-state detection probability as "very low" and single-emplacement success at 85–95%. Modeled as the cheapest high-value intel card in the library.',
  },

  pattern_of_life_analysis: {
    id: 'pattern_of_life_analysis',
    name: 'Pattern-of-Life Analysis Cell',
    description:
      'Multi-source intelligence fusion — drone ISR, emplaced sensors, and open-source data — building persistent target profiles.',
    category: 'Surveillance',
    cost_per_tick: 0.15,
    deployment_cost: 10.0,
    coverage_limit: 2,
    effects: [
      { type: 'IntelGain', probability: 0.5 },
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.03 },
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.5 (Scenario 5 — Counter-Regime Persistent Surveillance)',
    trl: '8–9 (2026)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Section 3.5 flags pattern-of-life intel as the foundation of all targeted ops. Encoded as a high-probability IntelGain effect that amplifies other offensive cards.',
  },

  ew_jamming_drones: {
    id: 'ew_jamming_drones',
    name: 'EW Jamming Drone Payloads',
    description:
      'Drone-borne RF jammers targeting government and military communications. Validated in the Russia-Ukraine conflict.',
    category: 'ElectronicWarfare',
    cost_per_tick: 0.2,
    deployment_cost: 15.0,
    coverage_limit: 2,
    effects: [
      { type: 'CommsDisruption', factor: 0.75 },
      { type: 'CombatModifier', factor: 1.2 },
    ],
    countered_by: ['satellite_backup_comms', 'multi_phenom_c_uas'],
    is_offensive: true,
    etra_ref: 'Section 3.2 (Coup Facilitation); Appendix A',
    trl: '8 (2026) → 9 (2030)',
    profiles: ['A', 'B'],
    rationale:
      'Section 3.2: communications disruption for 2–6 hours is often decisive. Encoded as a heavy CommsDisruption effect with a modest combat bonus.',
  },

  remote_id_spoofing: {
    id: 'remote_id_spoofing',
    name: 'Remote-ID Spoofing Suite',
    description:
      'Fabricates Remote-ID broadcasts to make hostile drones appear as legitimate media or commercial platforms.',
    category: 'Concealment',
    cost_per_tick: 0.02,
    deployment_cost: 1.5,
    effects: [{ type: 'DetectionModifier', factor: 0.6 }],
    countered_by: ['adversarial_classification_ai'],
    is_offensive: true,
    etra_ref: 'Section 4.1 (table: Remote ID assumption)',
    trl: '7 (2026) → 8–9 (2030)',
    profiles: ['A', 'B', 'C', 'D'],
    rationale:
      'Section 4.1 notes open-source Remote-ID fabrication tools exist since 2023. Encoded as a 40% reduction in own-force detection probability.',
  },

  coercion_proof_campaign: {
    id: 'coercion_proof_campaign',
    name: 'Asymmetric Coercion Campaign',
    description:
      'Phased proof-of-capability operations — penetration without hostile action, public surveillance leaks, symbolic kinetic demos — designed to impose political cost.',
    category: 'InformationWarfare',
    cost_per_tick: 0.1,
    deployment_cost: 8.0,
    effects: [
      { type: 'CivilianSentiment', delta: -0.15 },
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.1 },
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.4 (Scenario 4 — Asymmetric Coercion Campaign)',
    trl: '9 (2026)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Section 3.4 calls this the most likely near-term application. Encoded as a sustained civilian-sentiment and enemy-morale drag rather than kinetic damage.',
  },

  swarm_delivered_cyber: {
    id: 'swarm_delivered_cyber',
    name: 'Swarm-Delivered Cyber-Physical Exploit',
    description:
      'Coordinated combination of perched rogue APs, credential harvest, and lateral network movement via compromised staff devices.',
    category: 'Cyber',
    cost_per_tick: 0.12,
    deployment_cost: 20.0,
    coverage_limit: 1,
    effects: [
      { type: 'IntelGain', probability: 0.4 },
      { type: 'CommsDisruption', factor: 0.5 },
      { type: 'CivilianSentiment', delta: -0.05 },
    ],
    countered_by: ['wids_cuas_fusion', 'cert_pinning_mdm'],
    is_offensive: true,
    etra_ref: 'Section 3.6; Appendix D Kill Chain Alpha (Phase 3 — Credential Harvest)',
    trl: '8–9 (2026)',
    profiles: ['A', 'B'],
    rationale:
      "Appendix D's credential-harvest phase delivers organizational network access; encoded as intel + partial comms disruption + modest civilian trust impact.",
  },

  insurgent_info_mesh: {
    id: 'insurgent_info_mesh',
    name: 'Insurgent Information Mesh',
    description:
      'Real-time drone-footage broadcast network enabling a revolutionary movement to dominate the narrative during civil unrest.',
    category: 'InformationWarfare',
    cost_per_tick: 0.1,
    deployment_cost: 6.0,
    effects: [
      { type: 'CivilianSentiment', delta: 0.2 },
      { type: 'MoraleEffect', target: 'Own', delta: 0.08 },
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.3 (Scenario 3 — Revolutionary Infrastructure Seizure)',
    trl: '8–9 (2026)',
    profiles: ['C'],
    rationale:
      'Section 3.3 identifies information dominance as the decisive factor for revolutionary movements. Encoded as positive civilian sentiment + own-morale boost.',
  },

  // ====================================================================
  // DEFENSIVE — Counter-capabilities
  // ====================================================================

  multi_phenom_c_uas: {
    id: 'multi_phenom_c_uas',
    name: 'Multi-Phenomenology C-UAS',
    description:
      'Radar + electro-optical + acoustic + passive-RF fusion tuned for autonomous, RF-silent targets. Minimum viable detection architecture for swarm threats.',
    category: 'CounterDrone',
    cost_per_tick: 0.5,
    deployment_cost: 300.0,
    coverage_limit: 1,
    effects: [
      { type: 'DetectionModifier', factor: 1.8 },
      { type: 'CombatModifier', factor: 0.7 },
    ],
    countered_by: ['micro_drone_swarm'],
    is_offensive: false,
    etra_ref: 'Section 4.1, 4.7 (row 4.1); Section 5.1 recommendation 1',
    trl: '7 (2026)',
    profiles: ['defender'],
    rationale:
      '$2M–$10M acquisition + $200K–$500K/year per ETRA Table 4.7. Encoded as a strong detection boost plus a combat damping factor for hostile drones.',
  },

  hpm_area_effect: {
    id: 'hpm_area_effect',
    name: 'HPM Area-Effect Counter-Swarm',
    description:
      'High-Power Microwave area-effect weapon (Epirus Leonidas class). Disables drone electronics over a wide footprint.',
    category: 'CounterDrone',
    cost_per_tick: 1.0,
    deployment_cost: 600.0,
    coverage_limit: 1,
    effects: [
      { type: 'AreaDenial', strength: 0.8 },
      { type: 'CounterTech', target: 'autonomous_drone_swarm', reduction: 0.6 },
    ],
    countered_by: ['micro_drone_swarm'],
    is_offensive: false,
    etra_ref: 'Section 4.4, 4.7 (row 4.4)',
    trl: '5–6 (2026) → 7–8 (2030)',
    profiles: ['defender'],
    rationale:
      '$5M–$20M per system plus certification bottleneck (Section 4.4). Encoded as strong AreaDenial + direct counter against autonomous_drone_swarm.',
  },

  adversarial_classification_ai: {
    id: 'adversarial_classification_ai',
    name: 'Adversarial-Robust Classification AI',
    description:
      'Behavioral drone classifier designed to resist adversarial optimization. Reduces false-positive rates enough to permit engagement over civilian environments.',
    category: 'CounterDrone',
    cost_per_tick: 0.3,
    deployment_cost: 150.0,
    effects: [
      { type: 'DetectionModifier', factor: 1.3 },
      { type: 'CounterTech', target: 'remote_id_spoofing', reduction: 0.5 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.2, 4.7 (row 4.2); Section 5.1 recommendation 3',
    trl: '3–4 (2026) → 5–6 (2030)',
    profiles: ['defender'],
    rationale:
      'Section 4.7 notes "no market solution" — pure R&D. Encoded as moderate DetectionModifier and a specific counter against Remote-ID spoofing.',
  },

  distributed_ir_sensor_net: {
    id: 'distributed_ir_sensor_net',
    name: 'Distributed IR / Lidar Micro-Drone Net',
    description:
      'IR sensor curtain + lidar inner-perimeter array aimed at closing the micro-drone detection gap.',
    category: 'CounterDrone',
    cost_per_tick: 0.15,
    deployment_cost: 120.0,
    coverage_limit: 1,
    effects: [
      { type: 'CounterTech', target: 'micro_drone_swarm', reduction: 0.7 },
      { type: 'DetectionModifier', factor: 1.2 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.3, 4.7 (row 4.3)',
    trl: '3 (2026) → 4–5 (2030)',
    profiles: ['defender'],
    rationale:
      'Section 4.7: $500K–$2M per installation, coverage limited to ~200m radius. Encoded as the dedicated counter to micro_drone_swarm.',
  },

  wids_cuas_fusion: {
    id: 'wids_cuas_fusion',
    name: 'WIDS + C-UAS Fusion Platform',
    description:
      'Correlates airspace detection with wireless intrusion monitoring. Catches perched rogue-AP drones that leave no flight signature.',
    category: 'Cyber',
    cost_per_tick: 0.1,
    deployment_cost: 40.0,
    effects: [
      { type: 'CounterTech', target: 'rogue_ap_drone_mesh', reduction: 0.55 },
      { type: 'CounterTech', target: 'swarm_delivered_cyber', reduction: 0.45 },
      { type: 'DetectionModifier', factor: 1.15 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.5, 4.7 (row 4.5)',
    trl: '1–2 (2026) → 3–4 (2030)',
    profiles: ['defender'],
    rationale:
      '$150K–$500K platform cost per Section 4.7. Lowest-TRL defender capability — modeled with moderate counter strength reflecting implementation maturity.',
  },

  rooftop_inspection_program: {
    id: 'rooftop_inspection_program',
    name: 'Rooftop & Exterior Inspection Program',
    description:
      'Baseline equipment inventory + quarterly physical inspection + monthly drone change-detection survey. Procedural counter to Scenario 7 emplacement.',
    category: { Custom: 'Procedural' },
    cost_per_tick: 0.04,
    deployment_cost: 3.0,
    effects: [
      { type: 'CounterTech', target: 'covert_sensor_emplacement', reduction: 0.6 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.6, 4.7 (row 4.6 — overhead surface inspection)',
    trl: '9 (procedural, no technology gap)',
    profiles: ['defender'],
    rationale:
      '$20K–$80K initial + $30K–$100K/year. ETRA flags this as "the cheapest mitigation closing the newest gap" — encoded as an inexpensive, high-effectiveness counter.',
  },

  device_verification_training: {
    id: 'device_verification_training',
    name: 'Exterior Device Verification Training',
    description:
      'Staff training + procedural change: verify unknown exterior devices with the labeled utility/vendor before accepting. Defeats physical social engineering.',
    category: { Custom: 'Procedural' },
    cost_per_tick: 0.01,
    deployment_cost: 0.8,
    effects: [
      { type: 'CounterTech', target: 'covert_sensor_emplacement', reduction: 0.3 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.6, 4.7 (row — exterior device verification)',
    trl: '9 (procedural)',
    profiles: ['defender'],
    rationale:
      '$5K–$15K training cost per ETRA. Section 4.6 notes a single procedural change "defeats the most effective concealment strategy available to the attacker."',
  },

  cert_pinning_mdm: {
    id: 'cert_pinning_mdm',
    name: 'Enforced MDM + Certificate Pinning',
    description:
      'Mobile device management enforcement across all devices with organizational access. Auto-connect disabled; certificate pinning on all internal apps.',
    category: 'Cyber',
    cost_per_tick: 0.06,
    deployment_cost: 15.0,
    effects: [
      { type: 'CounterTech', target: 'rogue_ap_drone_mesh', reduction: 0.4 },
      { type: 'CounterTech', target: 'swarm_delivered_cyber', reduction: 0.3 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.7 (row — cross-cutting: managed devices)',
    trl: '9 (2026)',
    profiles: ['defender'],
    rationale:
      '$50K–$200K platform cost. Does not protect personal devices; encoded as partial (not total) counter to rogue-AP and cyber-physical attacks.',
  },

  satellite_backup_comms: {
    id: 'satellite_backup_comms',
    name: 'Satellite & Optical Backup Comms',
    description:
      'Iridium PTT + Starlink mesh + optical free-space links at command posts. Immune to ground-based RF jamming.',
    category: 'Communications',
    cost_per_tick: 0.08,
    deployment_cost: 20.0,
    effects: [
      { type: 'InfraProtection', factor: 0.7 },
      { type: 'CounterTech', target: 'ew_jamming_drones', reduction: 0.65 },
      { type: 'CommsDisruption', factor: -0.5 },
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.7, 5.1 recommendation 4',
    trl: '9 (2026)',
    profiles: ['defender'],
    rationale:
      '$100K–$300K hardware + $30K–$80K/year. Encoded as strong counter to EW jamming plus infrastructure protection.',
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Return cards grouped by offensive / defensive. */
export function groupedCards() {
  const offensive = [];
  const defensive = [];
  for (const card of Object.values(TECH_LIBRARY)) {
    if (card.is_offensive) offensive.push(card);
    else defensive.push(card);
  }
  return { offensive, defensive };
}

/** Look up a card by id. */
export function getCard(id) {
  return TECH_LIBRARY[id] || null;
}

/**
 * Serialize a single tech card to TOML text ready to append to a
 * scenario. Strips library-only metadata fields.
 */
export function cardToToml(card) {
  const out = [];
  out.push(`[technology.${card.id}]`);
  out.push(`id = "${card.id}"`);
  out.push(`name = ${JSON.stringify(card.name)}`);
  out.push(`description = ${JSON.stringify(card.description)}`);
  out.push(`category = ${tomlCategory(card.category)}`);
  out.push(`cost_per_tick = ${card.cost_per_tick}`);
  out.push(`deployment_cost = ${card.deployment_cost}`);
  if (card.coverage_limit != null) {
    out.push(`coverage_limit = ${card.coverage_limit}`);
  }
  out.push(`countered_by = [${card.countered_by.map((c) => `"${c}"`).join(', ')}]`);
  out.push('terrain_modifiers = []');
  out.push('');
  for (const effect of card.effects) {
    out.push(`[[technology.${card.id}.effects]]`);
    const entries = Object.entries(effect);
    for (const [k, v] of entries) {
      if (typeof v === 'number') out.push(`${k} = ${v}`);
      else out.push(`${k} = ${JSON.stringify(v)}`);
    }
    out.push('');
  }
  return out.join('\n');
}

function tomlCategory(cat) {
  if (typeof cat === 'string') return `"${cat}"`;
  if (cat && typeof cat === 'object' && cat.Custom) {
    return `{ Custom = "${cat.Custom}" }`;
  }
  return '"Logistics"';
}

/**
 * Insert a tech card into an existing TOML string and optionally
 * grant it to a list of faction IDs via their `tech_access` lists.
 *
 * This is a best-effort string operation — it does not parse the TOML.
 * It avoids creating duplicates if the card id is already present and
 * appends the card block at the end of the text if no anchor is found.
 *
 * @param {string} tomlText
 * @param {TechCardSpec} card
 * @param {Array<string>} [grantFactions]
 * @returns {{ toml: string, added: boolean, granted: string[] }}
 */
export function insertCardIntoToml(tomlText, card, grantFactions = []) {
  if (tomlText.includes(`[technology.${card.id}]`)) {
    // Already present — only (optionally) add to tech_access lists.
    const granted = grantFactions.filter((fid) =>
      updateTechAccessFor(tomlText, fid, card.id).granted,
    );
    let t = tomlText;
    for (const fid of granted) {
      t = updateTechAccessFor(t, fid, card.id).text;
    }
    return { toml: t, added: false, granted };
  }

  const block = `\n${cardToToml(card)}\n`;

  // Anchor 1: insert after the last existing [technology.<something>] block.
  const techBlockRegex = /\[technology\.[a-zA-Z0-9_]+\][\s\S]*?(?=\n\[[^\]]+\]|\n*$)/g;
  let lastMatch = null;
  let m;
  while ((m = techBlockRegex.exec(tomlText)) !== null) {
    lastMatch = m;
  }
  let updated;
  if (lastMatch) {
    const end = lastMatch.index + lastMatch[0].length;
    updated = tomlText.slice(0, end) + '\n' + block + tomlText.slice(end);
  } else {
    // Anchor 2: insert right after [technology] header if present.
    const headerIdx = tomlText.indexOf('[technology]');
    if (headerIdx >= 0) {
      const eol = tomlText.indexOf('\n', headerIdx);
      const insertAt = eol >= 0 ? eol + 1 : tomlText.length;
      updated = tomlText.slice(0, insertAt) + block + tomlText.slice(insertAt);
    } else {
      // Anchor 3: append a brand new section at the end.
      updated = `${tomlText.trimEnd()}\n\n# -- Technology (from Drone Threat Library) ---------------------------------\n[technology]\n${block}`;
    }
  }

  // Grant to requested factions.
  const granted = [];
  for (const fid of grantFactions) {
    const res = updateTechAccessFor(updated, fid, card.id);
    if (res.granted) {
      updated = res.text;
      granted.push(fid);
    }
  }

  return { toml: updated, added: true, granted };
}

/**
 * Best-effort update of a faction's `tech_access` list to include
 * `cardId`. Returns the new text and whether it was added.
 */
export function updateTechAccessFor(tomlText, factionId, cardId) {
  // Find the faction block header.
  const header = `[factions.${factionId}]`;
  const start = tomlText.indexOf(header);
  if (start < 0) return { text: tomlText, granted: false };
  // Scan forward until the next `[factions.<other>]` or `[<other top-level>]`
  // that begins in column 0 with no leading dot-path, so we stay within
  // this faction's body and include its sub-tables (forces, faction_type).
  const bodyStartIdx = start + header.length;
  const tail = tomlText.slice(bodyStartIdx);
  const nextFactionRegex = /\n\[factions\.[a-zA-Z0-9_]+\]/;
  const nextTopLevelRegex = /\n\[[a-zA-Z_][a-zA-Z0-9_]*\]/;
  const nextFaction = tail.search(nextFactionRegex);
  const nextTop = tail.search(nextTopLevelRegex);
  const candidates = [nextFaction, nextTop].filter((n) => n >= 0);
  const rel = candidates.length ? Math.min(...candidates) : tail.length;
  const bodyEndIdx = bodyStartIdx + rel;

  const body = tomlText.slice(bodyStartIdx, bodyEndIdx);
  const taRegex = /\n(\s*)tech_access\s*=\s*\[([^\]]*)\]/;
  const match = body.match(taRegex);
  if (match) {
    const currentList = match[2];
    if (currentList.includes(`"${cardId}"`)) return { text: tomlText, granted: false };
    const trimmed = currentList.trim();
    const newList = trimmed.length === 0 ? `"${cardId}"` : `${trimmed}, "${cardId}"`;
    const newLine = `\n${match[1]}tech_access = [${newList}]`;
    const newBody = body.replace(taRegex, newLine);
    return {
      text: tomlText.slice(0, bodyStartIdx) + newBody + tomlText.slice(bodyEndIdx),
      granted: true,
    };
  }
  // No tech_access line present — insert one right after the header.
  const insertion = `\ntech_access = ["${cardId}"]`;
  return {
    text: tomlText.slice(0, bodyStartIdx) + insertion + tomlText.slice(bodyStartIdx),
    granted: true,
  };
}
