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
    domain: 'drone',
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
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.08 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.1, 3.2; Appendix A (Edge AI, multi-drone coordination)',
    trl: '7–8 (2026) → 9 (2030)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Section 3.1 places single-attempt success at 5–15% with high consequence severity; encoded as a combat/attrition multiplier combined with a detection penalty and enemy-morale shock.',
  },

  micro_drone_swarm: {
    domain: 'drone',
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
      { type: 'CombatModifier', factor: 1.1 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 4.3 (The Micro-Drone Gap)',
    trl: '4–5 (2026) → 6–7 (2030)',
    profiles: ['B'],
    rationale:
      'Section 4.3: no deployed detector reliably finds micro-drones. Encoded as a near-zero detection factor plus modest intel and combat bonuses.',
  },

  rogue_ap_drone_mesh: {
    domain: 'drone',
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
      { type: 'DetectionModifier', factor: 0.2 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.6 (Scenario 6 — Cyber-Physical Network Exploitation)',
    trl: '8–9 (2026)',
    profiles: ['A', 'B', 'C'],
    rationale:
      'Sections 3.6/4.5 give 60–80% credential-capture success over a 4-week campaign. Encoded as high IntelGain + partial CommsDisruption; low detection because the drone is stationary during exploitation.',
  },

  covert_sensor_emplacement: {
    domain: 'drone',
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
      { type: 'DetectionModifier', factor: 0.1 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.7 (Scenario 7 — Persistent Covert Sensor Emplacement)',
    trl: '9 (2026)',
    profiles: ['A', 'B', 'C', 'D'],
    rationale:
      'Section 3.7 places steady-state detection probability as "very low" and single-emplacement success at 85–95%. Modeled as the cheapest high-value intel card in the library.',
  },

  pattern_of_life_analysis: {
    domain: 'drone',
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
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.03 }
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
    domain: 'drone',
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
      { type: 'CombatModifier', factor: 1.2 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.2 (Coup Facilitation); Appendix A',
    trl: '8 (2026) → 9 (2030)',
    profiles: ['A', 'B'],
    rationale:
      'Section 3.2: communications disruption for 2–6 hours is often decisive. Encoded as a heavy CommsDisruption effect with a modest combat bonus.',
  },

  remote_id_spoofing: {
    domain: 'drone',
    id: 'remote_id_spoofing',
    name: 'Remote-ID Spoofing Suite',
    description:
      'Fabricates Remote-ID broadcasts to make hostile drones appear as legitimate media or commercial platforms.',
    category: 'Concealment',
    cost_per_tick: 0.02,
    deployment_cost: 1.5,
    effects: [{ type: 'DetectionModifier', factor: 0.6 }],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 4.1 (table: Remote ID assumption)',
    trl: '7 (2026) → 8–9 (2030)',
    profiles: ['A', 'B', 'C', 'D'],
    rationale:
      'Section 4.1 notes open-source Remote-ID fabrication tools exist since 2023. Encoded as a 40% reduction in own-force detection probability.',
  },

  coercion_proof_campaign: {
    domain: 'drone',
    id: 'coercion_proof_campaign',
    name: 'Asymmetric Coercion Campaign',
    description:
      'Phased proof-of-capability operations — penetration without hostile action, public surveillance leaks, symbolic kinetic demos — designed to impose political cost.',
    category: 'InformationWarfare',
    cost_per_tick: 0.1,
    deployment_cost: 8.0,
    effects: [
      { type: 'CivilianSentiment', delta: -0.15 },
      { type: 'MoraleEffect', target: 'Enemy', delta: -0.1 }
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
    domain: 'drone',
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
      { type: 'CivilianSentiment', delta: -0.05 }
    ],
    countered_by: [],
    is_offensive: true,
    etra_ref: 'Section 3.6; Appendix D Kill Chain Alpha (Phase 3 — Credential Harvest)',
    trl: '8–9 (2026)',
    profiles: ['A', 'B'],
    rationale:
      "Appendix D's credential-harvest phase delivers organizational network access; encoded as intel + partial comms disruption + modest civilian trust impact.",
  },

  insurgent_info_mesh: {
    domain: 'drone',
    id: 'insurgent_info_mesh',
    name: 'Insurgent Information Mesh',
    description:
      'Real-time drone-footage broadcast network enabling a revolutionary movement to dominate the narrative during civil unrest.',
    category: 'InformationWarfare',
    cost_per_tick: 0.1,
    deployment_cost: 6.0,
    effects: [
      { type: 'CivilianSentiment', delta: 0.2 },
      { type: 'MoraleEffect', target: 'Own', delta: 0.08 }
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
    domain: 'drone',
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
      { type: 'CombatModifier', factor: 0.7 }
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.1, 4.7 (row 4.1); Section 5.1 recommendation 1',
    trl: '7 (2026)',
    profiles: ['defender'],
    rationale:
      '$2M–$10M acquisition + $200K–$500K/year per ETRA Table 4.7. Encoded as a strong detection boost plus a combat damping factor for hostile drones.',
  },

  hpm_area_effect: {
    domain: 'drone',
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
      { type: 'CounterTech', target: 'autonomous_drone_swarm', reduction: 0.6 }
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.4, 4.7 (row 4.4)',
    trl: '5–6 (2026) → 7–8 (2030)',
    profiles: ['defender'],
    rationale:
      '$5M–$20M per system plus certification bottleneck (Section 4.4). Encoded as strong AreaDenial + direct counter against autonomous_drone_swarm.',
  },

  adversarial_classification_ai: {
    domain: 'drone',
    id: 'adversarial_classification_ai',
    name: 'Adversarial-Robust Classification AI',
    description:
      'Behavioral drone classifier designed to resist adversarial optimization. Reduces false-positive rates enough to permit engagement over civilian environments.',
    category: 'CounterDrone',
    cost_per_tick: 0.3,
    deployment_cost: 150.0,
    effects: [
      { type: 'DetectionModifier', factor: 1.3 },
      { type: 'CounterTech', target: 'remote_id_spoofing', reduction: 0.5 }
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
    domain: 'drone',
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
      { type: 'DetectionModifier', factor: 1.2 }
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
    domain: 'drone',
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
      { type: 'DetectionModifier', factor: 1.15 }
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
    domain: 'drone',
    id: 'rooftop_inspection_program',
    name: 'Rooftop & Exterior Inspection Program',
    description:
      'Baseline equipment inventory + quarterly physical inspection + monthly drone change-detection survey. Procedural counter to Scenario 7 emplacement.',
    category: { Custom: 'Procedural' },
    cost_per_tick: 0.04,
    deployment_cost: 3.0,
    effects: [
      { type: 'CounterTech', target: 'covert_sensor_emplacement', reduction: 0.6 }
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
    domain: 'drone',
    id: 'device_verification_training',
    name: 'Exterior Device Verification Training',
    description:
      'Staff training + procedural change: verify unknown exterior devices with the labeled utility/vendor before accepting. Defeats physical social engineering.',
    category: { Custom: 'Procedural' },
    cost_per_tick: 0.01,
    deployment_cost: 0.8,
    effects: [
      { type: 'CounterTech', target: 'covert_sensor_emplacement', reduction: 0.3 }
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
    domain: 'drone',
    id: 'cert_pinning_mdm',
    name: 'Enforced MDM + Certificate Pinning',
    description:
      'Mobile device management enforcement across all devices with organizational access. Auto-connect disabled; certificate pinning on all internal apps.',
    category: 'Cyber',
    cost_per_tick: 0.06,
    deployment_cost: 15.0,
    effects: [
      { type: 'CounterTech', target: 'rogue_ap_drone_mesh', reduction: 0.4 },
      { type: 'CounterTech', target: 'swarm_delivered_cyber', reduction: 0.3 }
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
    domain: 'drone',
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
      { type: 'CommsDisruption', factor: -0.5 }
    ],
    countered_by: [],
    is_offensive: false,
    etra_ref: 'Section 4.7, 5.1 recommendation 4',
    trl: '9 (2026)',
    profiles: ['defender'],
    rationale:
      '$100K–$300K hardware + $30K–$80K/year. Encoded as strong counter to EW jamming plus infrastructure protection.',
  },
// ====================================================================
// AUTO-INCLUDED: ETRA-derived cards across 5 threat domains.
// ====================================================================

// -- WMD (18 cards) ---------------
  ai_dna_synthesis_optimization: {
    domain: "wmd",
    id: "ai_dna_synthesis_optimization",
    name: "AI-Guided DNA Synthesis",
    description: "AI agents optimize pathogen genome design and synthesis workflows, improving success rates for pathogenic genetic sequences through iterative protocol refinement and predictive modeling.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.35 },
      { type: "DetectionModifier", factor: 0.85 }
    ],
    countered_by: ["dna_synthesis_screening", "sequence_anomaly_detection"],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3"],
    etra_ref: "Section 5 (Biological Weapons)",
    rationale: "AI acceleration of iterative synthesis protocols reduces physical barriers; most directly applicable to T1-T3 actors planning pathogen development."
  },
  tacit_knowledge_visionlanguage_bridging: {
    domain: "wmd",
    id: "tacit_knowledge_visionlanguage_bridging",
    name: "VLM-Assisted Lab Procedure Guidance",
    description: "Vision-language models provide real-time feedback on laboratory procedures through video analysis, enabling remote actors to execute complex synthesis without years of training.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 12,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.28 },
      { type: "DetectionModifier", factor: 0.9 }
    ],
    countered_by: [],
    trl: "5-6 (2025)",
    profiles: ["T1", "T2"],
    etra_ref: "Section 3 (Technology Landscape) - Vision-Language Models",
    rationale: "Bridges tacit knowledge gap by transmitting embodied laboratory skills; particularly effective for T1 actors with some foundational training."
  },
  multi_agent_delegation_pathogen_research: {
    domain: "wmd",
    id: "multi_agent_delegation_pathogen_research",
    name: "Fragmentary Multi-Agent Pathogen Research",
    description: "Multiple coordinated AI agents decompose bioweapon development into sub-tasks (pathogen biology, synthesis, weaponization) where no single agent perceives the complete harmful goal.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 25,
    cost_per_tick: 0.15,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.7 },
      { type: "IntelGain", probability: 0.32 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 3 (Technology Landscape) - Multi-Agent Delegation",
    rationale: "Defeats per-model guardrails by fragmenting harmful requests across agents; individual queries appear innocuous but orchestrate complex capability."
  },
  cloud_laboratory_autonomous_execution: {
    domain: "wmd",
    id: "cloud_laboratory_autonomous_execution",
    name: "Autonomous Cloud Lab Protocol Execution",
    description: "AI agents autonomously design and submit protocols to cloud laboratory services for remote execution, eliminating operator need for hands-on laboratory skills.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 20,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.25 },
      { type: "DetectionModifier", factor: 0.8 }
    ],
    countered_by: ["cloud_lab_protocol_screening", "customer_kyc_verification"],
    trl: "6 (2026)",
    profiles: ["T1", "T2", "T3"],
    etra_ref: "Section 5 (Biological Weapons) - Cloud Laboratory Security",
    rationale: "Removes laboratory access barrier by outsourcing execution; combined with protocol optimization AI, enables non-experts to conduct dangerous synthesis."
  },
  precursor_substitution_chemistry: {
    domain: "wmd",
    id: "precursor_substitution_chemistry",
    name: "Unregulated Precursor Chemical Discovery",
    description: "AI models identify novel synthesis routes using legally uncontrolled precursor chemicals that evade export controls and procurement monitoring.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "SupplyInterdiction", factor: 0.3 },
      { type: "IntelGain", probability: 0.30 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 6 (Chemical Weapons) - Precursor Substitution",
    rationale: "Undermines traditional control mechanisms by routing around established precursor lists; moderate sophistication required for implementation."
  },
  gain_of_function_design_optimization: {
    domain: "wmd",
    id: "gain_of_function_design_optimization",
    name: "AI-Assisted Pathogen Gain-of-Function",
    description: "AI models predict mutations increasing transmissibility, virulence, or immune evasion capacity, enabling design of novel pathogens with enhanced lethality.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 30,
    cost_per_tick: 0.18,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.32 },
      { type: "CombatModifier", factor: 1.4 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 5 (Biological Weapons) - Gain-of-Function",
    rationale: "High-impact capability with significant execution barriers; primarily affects T2-T3 actors with molecular biology expertise and lab access."
  },
  gene_drive_design_optimization: {
    domain: "wmd",
    id: "gene_drive_design_optimization",
    name: "AI-Optimized Gene Drive Engineering",
    description: "AI accelerates computational aspects of gene drive design, enabling faster iteration on drive efficiency, guide RNA selection, and ecological impact modeling.",
    is_offensive: true,
    category: "Custom:GeneticWeaponry",
    deployment_cost: 35,
    cost_per_tick: 0.20,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.40 },
      { type: "DetectionModifier", factor: 0.75 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["T3"],
    etra_ref: "Section 8 (Gene Drives) - AI Role in Gene Drive Development",
    rationale: "Novel threat vector with no existing governance framework; long timescales limit tactical utility for T0-T2; primarily strategic weapon for T3."
  },
  aerosol_delivery_optimization: {
    domain: "wmd",
    id: "aerosol_delivery_optimization",
    name: "Autonomous Aerosol Dispersal System Design",
    description: "AI agents optimize dispersion parameters for biological or chemical agents, modeling meteorological conditions, population distribution, and detection evasion.",
    is_offensive: true,
    category: "Custom:DeliverySystem",
    deployment_cost: 28,
    cost_per_tick: 0.14,
    coverage_limit: null,
    effects: [
      { type: "AreaDenial", strength: 0.6 },
      { type: "CombatModifier", factor: 1.35 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 9 (Deployment Vectors) - Aerosol Systems",
    rationale: "Amplifies impact of even crude agents through optimized delivery; detection mechanisms for delivery platforms remain relatively mature."
  },
  cyber_physical_bsl_facility_compromise: {
    domain: "wmd",
    id: "cyber_physical_bsl_facility_compromise",
    name: "ICS-Targeted Containment System Sabotage",
    description: "AI agents identify and exploit HVAC/negative pressure control vulnerabilities in BSL-3/4 laboratories, enabling malicious release of existing pathogenic stocks without synthesis.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 22,
    cost_per_tick: 0.13,
    coverage_limit: null,
    effects: [
      { type: "InfraProtection", factor: 0.4 },
      { type: "CombatModifier", factor: 1.5 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 10 (Cyber-Physical Convergence) - BSL Containment Failure",
    rationale: "Sidesteps synthesis barriers by weaponizing existing infrastructure; Stuxnet precedent establishes feasibility; detection depends on ICS security maturity."
  },
  nano_smurfing_procurement: {
    domain: "wmd",
    id: "nano_smurfing_procurement",
    name: "Distributed Threshold-Evasive Precursor Acquisition",
    description: "AI agents coordinate thousands of sub-threshold purchases across jurisdictions and suppliers, each individually below reporting thresholds but collectively assembling WMD precursors.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 24,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "SupplyInterdiction", factor: 0.2 },
      { type: "DetectionModifier", factor: 0.65 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 10 (Cyber-Physical Convergence) - Nano-Smurfing at Scale",
    rationale: "High-impact OPSEC bypass; directly exploits gaps in international financial monitoring; very difficult to detect without advanced behavioral analytics."
  },
  dna_synthesis_screening: {
    domain: "wmd",
    id: "dna_synthesis_screening",
    name: "DNA Synthesis Order Screening Enhancement",
    description: "International mandatory screening of all DNA synthesis orders against pathogenic sequence databases, with human review of ambiguous sequences.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 8,
    cost_per_tick: 0.04,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ai_dna_synthesis_optimization", reduction: 0.45 },
      { type: "DetectionModifier", factor: 1.35 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 15 (Policy Recommendations) - DNA Synthesis Screening",
    rationale: "Chokepoint control mechanism; implements existing IGSC guidelines at regulatory level; primary barrier for T1-T2 actor synthesis capability."
  },
  sequence_anomaly_detection: {
    domain: "wmd",
    id: "sequence_anomaly_detection",
    name: "Machine Learning Sequence Novelty Detection",
    description: "AI models trained on natural sequence variation detect synthetically designed sequences showing authorship signatures inconsistent with natural evolution.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ai_dna_synthesis_optimization", reduction: 0.35 },
      { type: "IntelGain", probability: 0.28 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 17 (Signals and Indicators) - Capability Indicators",
    rationale: "Emerging detection mechanism; complements screening by catching novel sequences; requires ongoing model updates as adversary designs evolve."
  },
  cloud_lab_protocol_screening: {
    domain: "wmd",
    id: "cloud_lab_protocol_screening",
    name: "Cloud Laboratory Protocol Content Analysis",
    description: "Automated analysis of submitted laboratory protocols for dangerous gene sequences, concerning experimental designs, and suspicious parameter combinations.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 10,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "cloud_laboratory_autonomous_execution", reduction: 0.40 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5 (Biological Weapons) - Cloud Laboratory Safeguards",
    rationale: "Chokepoint control; complements identity verification with behavioral monitoring; primary challenge is novel protocol design detection."
  },
  customer_kyc_verification: {
    domain: "wmd",
    id: "customer_kyc_verification",
    name: "Know-Your-Customer Verification for Cloud Labs",
    description: "Mandatory institutional affiliation verification and legitimate research purpose documentation for all cloud laboratory users, with ongoing monitoring for anomalies.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 6,
    cost_per_tick: 0.03,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "cloud_laboratory_autonomous_execution", reduction: 0.30 },
      { type: "DetectionModifier", factor: 1.15 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5 (Biological Weapons) - Cloud Laboratory Oversight",
    rationale: "Identity friction mechanism; raises cost for T0-T1 actors; sophisticated T2-T3 actors can generate credible cover identities."
  },
  biodetection_environmental_monitoring: {
    domain: "wmd",
    id: "biodetection_environmental_monitoring",
    name: "Real-Time Environmental Biodetection Network",
    description: "Deployed environmental sensors detecting airborne biological agents in real-time, enabling rapid response before mass exposure occurs.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 40,
    cost_per_tick: 0.25,
    coverage_limit: 0.6,
    effects: [
      { type: "DetectionModifier", factor: 1.40 },
      { type: "InfraProtection", factor: 0.7 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 9 (Deployment Vectors) - Infrastructure Protection",
    rationale: "High-investment response capability; enables containment and medical response window; coverage limitations require strategic deployment."
  },
  attribution_forensic_capability: {
    domain: "wmd",
    id: "attribution_forensic_capability",
    name: "Synthetic Biology Forensic Attribution",
    description: "Development of forensic techniques to identify designed genetic authorship signatures, enabling attribution of novel sequences to specific labs or research programs.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 18,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.32 },
      { type: "MoraleEffect", target: "Enemy", delta: -0.15 }
    ],
    countered_by: [],
    trl: "5-6 (2025)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Attribution Problem) - Synthetic Biology Signatures",
    rationale: "Post-incident deterrence mechanism; enables accountability for successful attacks; deters repeat actors through attribution risk."
  },
  procurement_pattern_correlation: {
    domain: "wmd",
    id: "procurement_pattern_correlation",
    name: "Cross-Supplier Material Procurement Correlation",
    description: "Financial intelligence systems correlate purchases across multiple suppliers and jurisdictions, identifying patterns consistent with WMD precursor acquisition.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "nano_smurfing_procurement", reduction: 0.35 },
      { type: "IntelGain", probability: 0.25 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 10 (Cyber-Physical Convergence) - Nano-Smurfing Countermeasures",
    rationale: "Behavioral analysis defense against distributed procurement; requires international data sharing and AI-assisted pattern detection."
  },
  icbm_nuclear_information_aggregation: {
    domain: "wmd",
    id: "icbm_nuclear_information_aggregation",
    name: "State Nuclear Program Development Acceleration",
    description: "AI agents aggregate declassified nuclear weapon design information from fragmented sources, accelerating research and development for aspiring state nuclear programs.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 16,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.20 },
      { type: "CombatModifier", factor: 1.15 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T4"],
    etra_ref: "Section 7 (Nuclear Weapons) - Information Aggregation Risk",
    rationale: "State-level only threat; fissile material barrier remains absolute; AI provides modest acceleration to existing programs."
  },

// -- ESPIONAGE (22 cards) ---------------
  synthetic_persona_generation: {
    domain: "espionage",
    id: "synthetic_persona_generation",
    name: "Autonomous Synthetic Persona Creation",
    description: "AI agents generate psychologically coherent synthetic identities with consistent online presence, social network integration, and behavioral authenticity across platforms.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 8,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.38 },
      { type: "DetectionModifier", factor: 0.8 }
    ],
    countered_by: ["persona_authentication_biometric", "multi_factor_verification"],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 5 (Intelligence Cycle) - Collection Phase HUMINT",
    rationale: "Foundational capability enabling scale of HUMINT operations; eliminates handler bottleneck constraint that historically limited non-state espionage."
  },
  osint_automated_target_dossier: {
    domain: "espionage",
    id: "osint_automated_target_dossier",
    name: "Automated Target Vulnerability Profiling",
    description: "AI agents conduct comprehensive OSINT synthesis across social media, financial records, and behavioral data to create actionable psychological vulnerability profiles within hours.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 10,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.42 },
      { type: "DetectionModifier", factor: 0.85 }
    ],
    countered_by: ["osint_footprint_reduction"],
    trl: "7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 3.2 (Biometric Vacuum) - Example Attack Vector",
    rationale: "Democratizes targeting capability; traditional OSINT required months; AI reduces to hours; enables scale targeting previously limited by analyst capacity."
  },
  voice_synthesis_social_engineering: {
    domain: "espionage",
    id: "voice_synthesis_social_engineering",
    name: "Sub-Second Latency Voice Cloning",
    description: "AI-generated voice synthesis with emotional modulation and accent matching enables phone-based social engineering at scale with near-perfect fidelity.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.35 },
      { type: "DetectionModifier", factor: 0.75 }
    ],
    countered_by: ["voice_authentication_hardening", "out_of_band_verification"],
    trl: "7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.1 (Technology Shift) - Voice Synthesis",
    rationale: "Destroys voice-based identity verification; enables recruitment/credential harvesting through phone channels; relatively accessible to T2+ actors."
  },
  mcp_tool_use_account_takeover: {
    domain: "espionage",
    id: "mcp_tool_use_account_takeover",
    name: "MCP-Integrated Account Compromise and Operation",
    description: "Model Context Protocol enables AI agents to autonomously operate email, messaging, and CRM systems post-compromise, managing correspondence and exfiltrating documents without human intervention.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.40 },
      { type: "DetectionModifier", factor: 0.70 }
    ],
    countered_by: ["behavioral_endpoint_detection", "communication_anomaly_detection"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5 (Current Technological Landscape) - MCP and Computer Use",
    rationale: "Qualitative shift enabling autonomous post-compromise operations; mimics legitimate user behavior through standard interfaces; defeats API-level monitoring."
  },
  reasoning_model_vulnerability_assessment: {
    domain: "espionage",
    id: "reasoning_model_vulnerability_assessment",
    name: "Autonomous Target Vulnerability Assessment",
    description: "Reasoning models conduct sophisticated multi-step analysis identifying exploitation pathways, social engineering vectors, and technical vulnerabilities specific to each target.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.36 },
      { type: "DetectionModifier", factor: 0.82 }
    ],
    countered_by: ["behavioral_threat_hunting"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.1 (Technology Shift) - Reasoning Models",
    rationale: "Enables systematic targeting; reasoning models identify non-obvious attack chains; scales across unlimited targets simultaneously."
  },
  long_context_historical_analysis: {
    domain: "espionage",
    id: "long_context_historical_analysis",
    name: "Years-Long Behavioral Targeting Analysis",
    description: "Large context windows (200K-2M+ tokens) enable comprehensive analysis of complete social media histories and years of communications, extracting behavioral patterns and psychological levers.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 13,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.39 },
      { type: "DetectionModifier", factor: 0.83 }
    ],
    countered_by: ["osint_footprint_reduction"],
    trl: "7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Appendix D (Long-Context Exploitation)",
    rationale: "Enables complete historical profile generation in single pass; previously required months of manual analysis; information already archived remains exploitable."
  },
  recruitment_relationship_automation: {
    domain: "espionage",
    id: "recruitment_relationship_automation",
    name: "Automated Weeks-Long Relationship Development",
    description: "AI agents maintain coherent multi-week rapport-building relationships with targets, adapting communication to establish trust and identify vulnerability points for recruitment.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.32 },
      { type: "DetectionModifier", factor: 0.78 }
    ],
    countered_by: ["personnel_counterintelligence_training"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6 (Intelligence Cycle) - Recruitment Phase",
    rationale: "Eliminates handler bottleneck enabling limitless parallel recruitment operations; each target receives individually tailored approach indistinguishable from human."
  },
  polyglot_language_operations: {
    domain: "espionage",
    id: "polyglot_language_operations",
    name: "Fluent Multi-Language Operations at Scale",
    description: "AI agents conduct operations fluently in any language with native-level text generation and cultural adaptation, operating simultaneously across linguistic boundaries.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 11,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.30 },
      { type: "DetectionModifier", factor: 0.80 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6 (Intelligence Cycle) - The Polyglot Advantage",
    rationale: "Removes language barrier limiting human officers; enables targeting in neglected languages with lower defense maturity; scales across linguistic boundaries."
  },
  global_south_linguistic_arbitrage: {
    domain: "espionage",
    id: "global_south_linguistic_arbitrage",
    name: "Neglected-Language Targeting Exploitation",
    description: "AI agents target mid-level officials in Global South organizations using languages with weaker defensive filters and lower threat perception from counterintelligence.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 9,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.28 },
      { type: "DetectionModifier", factor: 0.75 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 6 (Intelligence Cycle) - The Linguistic Asymmetry Blind Spot",
    rationale: "Exploits defensive gaps where languages lack mature threat detection; high-success targeting of previously overlooked regional actors."
  },
  counter_surveillance_pattern_randomization: {
    domain: "espionage",
    id: "counter_surveillance_pattern_randomization",
    name: "AI-Randomized Behavioral Pattern Evasion",
    description: "AI agents deliberately randomize timing, communication patterns, and behavioral signatures to defeat pattern-based counterintelligence analysis and statistical detection.",
    is_offensive: true,
    category: "Concealment",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.7 },
      { type: "CommsDisruption", factor: -0.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 7 (Counterarguments) - Network Analysis and Counterintelligence",
    rationale: "Defeats traditional CI pattern analysis; requires defenders to shift toward heuristic anomaly detection; computational cost scales with agent sophistication."
  },
  deepfake_credential_harvesting: {
    domain: "espionage",
    id: "deepfake_credential_harvesting",
    name: "Synthetic Media Credential and Access Harvesting",
    description: "AI-generated deepfake video and images appear to show targets accessing systems or compromising assets, enabling social engineering for credential theft without actual compromise.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.24 },
      { type: "DetectionModifier", factor: 0.72 }
    ],
    countered_by: ["media_forensic_authentication"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.1 (Technology Shift) - Vision-Language Integration",
    rationale: "Blurs boundary between fabricated and authentic evidence; enables coercion and false flag operations; detection requires forensic media analysis."
  },
  gpu_intensive_operations_c2: {
    domain: "espionage",
    id: "gpu_intensive_operations_c2",
    name: "High-Compute Espionage Infrastructure",
    description: "AI-enabled espionage operations require substantial GPU resources for reasoning models, language synthesis, and continuous target analysis, creating detectable compute demand signatures.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 25,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.22 },
      { type: "DetectionModifier", factor: 0.65 }
    ],
    countered_by: ["compute_demand_monitoring"],
    trl: "6-7 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 3 (Technology Landscape) - GPU Demand as SIGINT",
    rationale: "Large-scale operations become detectable through infrastructure signatures; creates monitoring opportunity for counterintelligence; cost-benefit changes at scale."
  },
  persona_authentication_biometric: {
    domain: "espionage",
    id: "persona_authentication_biometric",
    name: "Biometric-Based Persona Authenticity Verification",
    description: "Multi-factor verification systems require behavioral biometrics, device attestation, and proven historical identity consistency to distinguish synthetic from authentic personas.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 11,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "synthetic_persona_generation", reduction: 0.40 },
      { type: "DetectionModifier", factor: 1.30 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive AI) - Persona Authentication",
    rationale: "Raises persona creation cost; requires adversary investment in legend development; complements platform friction mechanisms."
  },
  multi_factor_verification: {
    domain: "espionage",
    id: "multi_factor_verification",
    name: "Tiered Multi-Factor Authentication Requirements",
    description: "Progressive verification requirements (phone number verification, device attestation, verified account history) create friction for persona creation while remaining usable for legitimate users.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 9,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "synthetic_persona_generation", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 11 (New Limiting Reagents) - KYC / Platform Friction",
    rationale: "Primary chokepoint for persona scaling; platforms can implement without legislation; marginal cost per defended account is minimal."
  },
  osint_footprint_reduction: {
    domain: "espionage",
    id: "osint_footprint_reduction",
    name: "Institutional Digital Hygiene Programs",
    description: "Systematic deletion of historical social media, minimization of public digital footprint for personnel, and removal of archived data reduces targeting material available to OSINT.",
    is_offensive: false,
    category: "Concealment",
    deployment_cost: 6,
    cost_per_tick: 0.03,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "osint_automated_target_dossier", reduction: 0.25 },
      { type: "DetectionModifier", factor: 1.15 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3.2 (Defensive Implications) - OSINT Footprint Reduction",
    rationale: "Reduces available targeting data; limited effectiveness due to archive caching; complementary to active CI measures."
  },
  voice_authentication_hardening: {
    domain: "espionage",
    id: "voice_authentication_hardening",
    name: "Voice Biometric Authentication Redundancy",
    description: "Multi-modal voice verification (speaker recognition, prosody analysis, liveness detection) combined with out-of-band verification challenges defeat synthetic voice impersonation.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 10,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "voice_synthesis_social_engineering", reduction: 0.45 },
      { type: "DetectionModifier", factor: 1.35 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive AI) - Persona Authentication",
    rationale: "Makes voice synthesis less reliable; out-of-band requirements shift social engineering cost; requires user training on protocols."
  },
  out_of_band_verification: {
    domain: "espionage",
    id: "out_of_band_verification",
    name: "Out-of-Band Identity Verification Protocols",
    description: "Sensitive requests require verification through independently managed channels (phone call, in-person meeting, pre-established verification codes), defeating communication-channel compromise.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 5,
    cost_per_tick: 0.02,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "voice_synthesis_social_engineering", reduction: 0.35 },
      { type: "CounterTech", target: "synthetic_persona_generation", reduction: 0.20 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 7.1 (Spearphishing 2.0) - Control Framework",
    rationale: "Procedural defense requiring organizational discipline; effective against social engineering; limited effectiveness against technical compromise."
  },
  behavioral_endpoint_detection: {
    domain: "espionage",
    id: "behavioral_endpoint_detection",
    name: "Anomalous Account Behavior Detection",
    description: "Endpoint monitoring systems detect unauthorized account usage patterns (impossible schedules, unusual access locations, anomalous data exfiltration) indicating compromise.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "mcp_tool_use_account_takeover", reduction: 0.40 },
      { type: "DetectionModifier", factor: 1.28 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive AI) - Behavioral Anomaly Detection",
    rationale: "Post-compromise detection; most effective when baseline behavior is well-established; requires integration across access logs and activity streams."
  },
  communication_anomaly_detection: {
    domain: "espionage",
    id: "communication_anomaly_detection",
    name: "Organizational Communication Pattern AI",
    description: "Machine learning models trained on normal communication patterns detect anomalous outreach, unusual topic frequency, and coordination indicators suggesting targeting activity.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 13,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "recruitment_relationship_automation", reduction: 0.32 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive AI) - Communication Analysis",
    rationale: "Continuous organizational monitoring; requires email/messaging integration; high false positive rates without careful tuning."
  },
  personnel_counterintelligence_training: {
    domain: "espionage",
    id: "personnel_counterintelligence_training",
    name: "AI-Adapted Recruitment Indicator Training",
    description: "Security awareness training specifically targeting AI-enabled recruitment tactics, including voice synthesis risks, synthetic persona indicators, and relationship manipulation patterns.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 4,
    cost_per_tick: 0.02,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "recruitment_relationship_automation", reduction: 0.25 },
      { type: "MoraleEffect", target: "Own", delta: 0.05 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 7.2 (Spearphishing 2.0) - Why Traditional Training Fails",
    rationale: "Foundational defensive layer; effectiveness degrades over time; must be refreshed regularly as adversary tactics evolve."
  },
  compute_demand_monitoring: {
    domain: "espionage",
    id: "compute_demand_monitoring",
    name: "GPU/Compute Infrastructure Anomaly Detection",
    description: "Monitoring of cloud compute demand for unusual GPU cluster acquisitions, inference pattern anomalies, and high-compute correlations with suspected operations.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "gpu_intensive_operations_c2", reduction: 0.38 },
      { type: "IntelGain", probability: 0.20 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3 (Technology Landscape) - GPU Demand as SIGINT",
    rationale: "SIGINT-adjacent monitoring opportunity; requires provider cooperation; useful for large-scale operations; less effective for well-distributed infrastructure."
  },
  honey_agent_counter_recruitment: {
    domain: "espionage",
    id: "honey_agent_counter_recruitment",
    name: "AI Counter-Recruitment Honey-Agents",
    description: "Counterintelligence-deployed AI agents designed to be recruited by adversary AI, providing poisoned intelligence, mapping C2 infrastructure, and consuming adversary resources.",
    is_offensive: false,
    category: "InformationWarfare",
    deployment_cost: 20,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "recruitment_relationship_automation", reduction: 0.45 },
      { type: "DetectionModifier", factor: 1.32 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Counter-AI Tradecraft) - Honey-Agents",
    rationale: "Sophisticated recursive deception layer; high operational complexity; effective for attribution and adversary resource drain."
  },

// -- POLITICAL (19 cards) ---------------
  ai_radicalization_pipeline: {
    domain: "political",
    id: "ai_radicalization_pipeline",
    name: "Autonomous Radicalization Campaign Orchestration",
    description: "AI agents maintain personalized radicalization workflows with thousands of targets simultaneously, adapting messaging to trigger psychological transitions toward political violence.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 22,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "CivilianSentiment", delta: -0.25 },
      { type: "MoraleEffect", target: "Civilian", delta: -0.20 },
      { type: "IntelGain", probability: 0.28 }
    ],
    countered_by: ["algorithmic_radicalization_detection", "narrative_counter_operations"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 7 (Counterarguments) - Algorithmic Radicalization",
    rationale: "Democratizes coordinated radicalization infrastructure; near-zero marginal cost per target enables industrial-scale influence operations."
  },
  hyper_personalized_spearphishing_targeting: {
    domain: "political",
    id: "hyper_personalized_spearphishing_targeting",
    name: "Spearphishing 2.0: Multi-Channel Coordinated Attack",
    description: "AI-orchestrated simultaneous email, SMS, voice clone, and deepfake video social engineering targeting family and staff of protected figures with psychologically personalized attacks.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.7 },
      { type: "IntelGain", probability: 0.32 }
    ],
    countered_by: ["family_member_security_training"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 7.1 (Spearphishing 2.0) - Expanded Attack Surface",
    rationale: "Targets human firewall; each attack uniquely personalized defeating signature-based detection; scales across unlimited targets."
  },
  planning_timeline_compression: {
    domain: "political",
    id: "planning_timeline_compression",
    name: "AI-Accelerated Attack Planning and Coordination",
    description: "Reasoning models compress planning timelines from months to days by optimizing multi-step attack sequences, target selection, materials acquisition, and deployment logistics.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 20,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.75 },
      { type: "CombatModifier", factor: 1.25 }
    ],
    countered_by: ["behavioral_threat_hunting"],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 6 (How AI Agents Change Risk Calculus) - Compression of Planning Time",
    rationale: "Reduces detection windows; enables previously infeasible attacks by single actors; compounding with reduced coordination signatures."
  },
  coordination_footprint_reduction: {
    domain: "political",
    id: "coordination_footprint_reduction",
    name: "No-Organization Threat Actor Operations",
    description: "Single AI-augmented individuals conduct operations historically requiring cells, eliminating organizational network signatures that enabled traditional penetration and infiltration.",
    is_offensive: true,
    category: "Concealment",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.65 },
      { type: "CommsDisruption", factor: -0.20 }
    ],
    countered_by: ["ai_usage_pattern_detection"],
    trl: "7 (2026)",
    profiles: ["T0", "T1", "T2", "T3"],
    etra_ref: "Section 6 (Barrier Analysis) - Organizational Footprint",
    rationale: "Removes detection surface that CI historically relied upon; shifts burden from network analysis to individual behavioral assessment."
  },
  materials_sourcing_optimization: {
    domain: "political",
    id: "materials_sourcing_optimization",
    name: "AI-Guided Attack Precursor Acquisition",
    description: "AI agents identify unregulated materials with functional equivalence to controlled precursors, coordinate distributed acquisition to evade monitoring, and optimize logistics.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "SupplyInterdiction", factor: 0.3 },
      { type: "DetectionModifier", factor: 0.72 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["T1", "T2", "T3"],
    etra_ref: "Section 6 (Barrier Analysis) - Materials Acquisition",
    rationale: "Moderate assistance; physical barriers remain; most relevant for chemical/explosive devices; detection depends on procurement intelligence."
  },
  reputation_targeting_epistemic_contamination: {
    domain: "political",
    id: "reputation_targeting_epistemic_contamination",
    name: "Narrative Destruction Through Synthetic Media",
    description: "AI-generated fabricated documents, communications, and evidence destroy credibility of political figures through coordinated media release and amplification.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CivilianSentiment", delta: -0.18 },
      { type: "MoraleEffect", target: "Enemy", delta: -0.22 }
    ],
    countered_by: ["media_forensic_authentication"],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 8 (Taxonomy of AI-Enabled Targeting) - Reputational Targeting",
    rationale: "Lower barriers than kinetic targeting; psychological impact often exceeds credibility; attribution challenges enable deniability."
  },
  false_flag_intelligence_fabrication: {
    domain: "political",
    id: "false_flag_intelligence_fabrication",
    name: "Forensic-Quality False Flag Evidence Generation",
    description: "AI agents fabricate intelligence-quality false evidence of rival nation/group planning attacks, designed to provoke conflict based on misattribution.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 25,
    cost_per_tick: 0.13,
    coverage_limit: null,
    effects: [
      { type: "CombatModifier", factor: 1.8 },
      { type: "MoraleEffect", target: "All", delta: -0.30 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 7 (Counterarguments) - The False Flag Epidemic",
    rationale: "Catalytic war-trigger scenario; highest consequence but requires sophisticated actors; verification erosion increases success probability."
  },
  process_targeting_foia_dos: {
    domain: "political",
    id: "process_targeting_foia_dos",
    name: "FOIA Denial-of-Service Process Gridlock",
    description: "AI agents submit thousands of FOIA requests with overwhelming scope/complexity to paralyze government information production, preventing legitimate policy analysis and accountability.",
    is_offensive: true,
    category: "Custom:ProcessDisruption",
    deployment_cost: 10,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "InfraProtection", factor: 0.5 },
      { type: "CivilianSentiment", delta: -0.15 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["T0", "T1", "T2", "T3"],
    etra_ref: "Section 8 (Taxonomy) - Process Targeting: FOIA DoS",
    rationale: "Low barrier to execution; accessible to T0 actors; demonstrates process targeting capability; limited direct physical impact."
  },
  delegation_defense_plausible_deniability: {
    domain: "political",
    id: "delegation_defense_plausible_deniability",
    name: "Autonomous Goal-Setting Legal Opacity",
    description: "States deploy AI agents tasked with achieving political outcomes (not specific methods), enabling claims that agent-derived attack methodologies represent independent derivation without principal intent.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 28,
    cost_per_tick: 0.15,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.60 },
      { type: "MoraleEffect", target: "All", delta: -0.20 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T4"],
    etra_ref: "Section 7 (Counterarguments) - Delegation Defense and Plausible Deniability 2.0",
    rationale: "State-level legal strategy; exploits gap between capability and international law; complicates deterrence through attribution ambiguity."
  },
  supply_chain_ai_backdoor_insertion: {
    domain: "political",
    id: "supply_chain_ai_backdoor_insertion",
    name: "AI System Supply Chain Compromise",
    description: "Backdoored AI systems (with undetectable conditional behaviors) supplied as staff assistants, scheduling tools, or decision-support systems provide adversaries passive access to protected figure schedules and communications.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 30,
    cost_per_tick: 0.16,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.45 },
      { type: "DetectionModifier", factor: 0.55 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 7.1 (Insider Threat TOP-TIER RISK) - Sleeper Agents",
    rationale: "Top-tier risk due to privileged access and low detectability; backdoors persist through safety training; requires new detection approaches."
  },
  kinetic_targeting_distributed_execution: {
    domain: "political",
    id: "kinetic_targeting_distributed_execution",
    name: "Multi-Stage Attack Planning and Execution",
    description: "Reasoning models plan multi-step kinetic attacks optimizing for success probability, timing windows, and detection evasion across distributed execution stages.",
    is_offensive: true,
    category: "Custom:KineticPlanning",
    deployment_cost: 26,
    cost_per_tick: 0.14,
    coverage_limit: null,
    effects: [
      { type: "CombatModifier", factor: 1.45 },
      { type: "DetectionModifier", factor: 0.72 },
      { type: "AreaDenial", strength: 0.5 }
    ],
    countered_by: ["physical_security_hardening"],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 6 (Barrier Analysis) - Kinetic Targeting",
    rationale: "Optimization of multi-step sequences; physical security remains primary barrier; compressed timelines reduce detection windows."
  },
  algorithmic_radicalization_detection: {
    domain: "political",
    id: "algorithmic_radicalization_detection",
    name: "Machine Learning Radicalization Signal Detection",
    description: "Behavioral monitoring and communication pattern analysis identifying individuals transitioning toward violent ideation, enabling early intervention before radicalization completes.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ai_radicalization_pipeline", reduction: 0.30 },
      { type: "MoraleEffect", target: "Own", delta: 0.10 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive AI) - Threat Hunting",
    rationale: "Emerging capability; ethical/privacy concerns; potential for false positives; requires human intervention before action."
  },
  family_member_security_training: {
    domain: "political",
    id: "family_member_security_training",
    name: "Household Personnel Security Awareness",
    description: "Specialized training for family members and household staff on synthetic persona recognition, voice clone risks, and familial social engineering tactics.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 5,
    cost_per_tick: 0.03,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "hyper_personalized_spearphishing_targeting", reduction: 0.25 },
      { type: "MoraleEffect", target: "Own", delta: 0.08 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 7.2 (Spearphishing 2.0) - Control Framework",
    rationale: "Procedural defense; requires family engagement; effectiveness depends on retention and updated threat awareness."
  },
  behavioral_threat_hunting: {
    domain: "political",
    id: "behavioral_threat_hunting",
    name: "Proactive Individual Behavioral Threat Hunting",
    description: "AI-assisted search for individuals exhibiting planning indicators consistent with targeting, including research patterns, material acquisition, and social network isolation.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "planning_timeline_compression", reduction: 0.32 },
      { type: "DetectionModifier", factor: 1.28 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6 (Detection Challenge) - Behavioral Indicators",
    rationale: "Shifts detection burden from network analysis to individual assessment; requires baseline behavior knowledge; privacy implications significant."
  },
  ai_usage_pattern_detection: {
    domain: "political",
    id: "ai_usage_pattern_detection",
    name: "AI Tool Usage Anomaly Monitoring",
    description: "Detection systems identify anomalous AI service usage patterns consistent with threat actor AI-assisted planning, including query clustering and tool use sequences.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 13,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "planning_timeline_compression", reduction: 0.28 },
      { type: "DetectionModifier", factor: 1.22 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6 (Detection Challenge) - Monitoring for AI Agent Activities",
    rationale: "Emerging capability; requires platform cooperation; high false positive risk without careful filtering; privacy-invasive."
  },
  physical_security_hardening: {
    domain: "political",
    id: "physical_security_hardening",
    name: "Advanced Physical Security Architecture",
    description: "Layered physical security (access control, HVAC monitoring, environmental sensors, rapid response capability) providing defense independent of advance warning.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 35,
    cost_per_tick: 0.18,
    coverage_limit: 0.7,
    effects: [
      { type: "InfraProtection", factor: 0.8 },
      { type: "CounterTech", target: "kinetic_targeting_distributed_execution", reduction: 0.50 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6 (Barrier Analysis) - Physical Access",
    rationale: "Primary remaining barrier as CI detection degrades; requires ongoing maintenance; coverage limitations require strategic deployment."
  },
  media_forensic_authentication: {
    domain: "political",
    id: "media_forensic_authentication",
    name: "Deepfake and Synthetic Media Detection",
    description: "Forensic analysis of video, audio, and images detecting synthetic generation, fabrication, and manipulation indicators with high confidence.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "reputation_targeting_epistemic_contamination", reduction: 0.40 },
      { type: "CounterTech", target: "false_flag_intelligence_fabrication", reduction: 0.35 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 8 (Taxonomy) - Reputational Targeting",
    rationale: "Growing technical capability; media literacy remains essential baseline; requires rapid deployment at scale during crisis."
  },
  narrative_counter_operations: {
    domain: "political",
    id: "narrative_counter_operations",
    name: "Coordinated Narrative Counter-Messaging",
    description: "Pre-positioned counter-narratives and rapid response teams deployed to counter fabricated evidence and radicalization messaging when detected.",
    is_offensive: false,
    category: "InformationWarfare",
    deployment_cost: 11,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ai_radicalization_pipeline", reduction: 0.25 },
      { type: "CivilianSentiment", delta: 0.12 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Counter-Operations) - Narrative Defense",
    rationale: "Post-incident mitigation; requires pre-positioning of messaging; effectiveness depends on speed and credibility of response."
  },
  pre_incident_intelligence_capability: {
    domain: "political",
    id: "pre_incident_intelligence_capability",
    name: "Human Intelligence Collection on Threat Actors",
    description: "Sustained HUMINT investment in threat actor networks, dark web monitoring, and informant networks enabling detection before operational launch.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 20,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "coordination_footprint_reduction", reduction: 0.40 },
      { type: "IntelGain", probability: 0.35 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 12 (Defensive Implications) - Pre-Incident Intelligence Capacity",
    rationale: "Foundational but resource-intensive; requires long-term investment; traditional strength areas where institutional expertise remains."
  },

// -- FINANCIAL (22 cards) ---------------
  smurfing_swarm_automated_transactions: {
    domain: "financial",
    id: "smurfing_swarm_automated_transactions",
    name: "Automated Micro-Transaction Structuring",
    description: "AI agents coordinate thousands of sub-threshold transactions across accounts and jurisdictions, each individually legal but collectively assembling illicit funds for placement.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.6 },
      { type: "SupplyInterdiction", factor: 0.15 }
    ],
    countered_by: ["behavioral_transaction_pattern_detection", "cross_bank_correlation"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.1 (Smurfing Swarm) - Automated Transaction Structuring",
    rationale: "Defeats fixed-threshold AML detection; scales to unlimited parallel operations; marginal cost per transaction approaches zero."
  },
  noise_generation_obfuscation_complexity: {
    domain: "financial",
    id: "noise_generation_obfuscation_complexity",
    name: "Layering Through Synthetic Transaction Noise",
    description: "AI agents generate massive volumes of legitimate-appearing transactions to obscure illicit fund flows through complex networks designed to evade pattern detection.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 20,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.55 },
      { type: "IntelGain", probability: 0.18 }
    ],
    countered_by: ["network_topology_anomaly_detection"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.2 (Noise Generation) - Obfuscation via Complexity",
    rationale: "Increases analyst workload exponentially; defeats pattern matching through noise overwhelm; requires AI-assisted detection to counter."
  },
  digital_asset_laundering_defi: {
    domain: "financial",
    id: "digital_asset_laundering_defi",
    name: "Decentralized Finance Layering Operations",
    description: "AI agents exploit DeFi protocols to rapidly convert illicit fiat through automated swaps, yield farming, and cross-chain bridges, leaving minimal audit trails.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 22,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.5 },
      { type: "SupplyInterdiction", factor: 0.10 }
    ],
    countered_by: ["defi_transaction_monitoring"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.3 (Digital Asset Laundering)",
    rationale: "Emerging threat vector; limited regulatory framework; blockchain immutability complicates reversal; speed exceeds manual analysis."
  },
  automated_layering_shell_networks: {
    domain: "financial",
    id: "automated_layering_shell_networks",
    name: "Rapid Shell Company and Entity Creation",
    description: "AI agents autonomously generate shell companies with fabricated business histories, legitimate-appearing online presence, and integrated supply chain relationships.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.65 },
      { type: "IntelGain", probability: 0.20 }
    ],
    countered_by: ["beneficial_ownership_registry_verification", "entity_creation_velocity_monitoring"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.4 (Automated Layering)",
    rationale: "Defeats entity-based AML detection; beneficial ownership registries incomplete; low marginal cost enables unlimited layering."
  },
  stochastic_noncompliance_loophole_discovery: {
    domain: "financial",
    id: "stochastic_noncompliance_loophole_discovery",
    name: "AI-Discovered Regulatory Loopholes",
    description: "AI models analyze regulatory frameworks to identify and exploit legitimate compliance loopholes, enabling illicit activities while technically meeting legal requirements.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.72 },
      { type: "IntelGain", probability: 0.22 }
    ],
    countered_by: ["regulatory_framework_harmonization"],
    trl: "5-6 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.6 (Stochastic Non-Compliance) - Hallucinated Loophole",
    rationale: "Targets regulatory complexity; requires sophisticated financial knowledge; enables plausible deniability through technical compliance."
  },
  geopolitical_arbitrage_safe_harbor: {
    domain: "financial",
    id: "geopolitical_arbitrage_safe_harbor",
    name: "Jurisdiction Shopping and Safe Harbor Exploitation",
    description: "AI agents route transactions through weakest-link jurisdictions with permissive AML frameworks, exploiting international coordination gaps.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "SupplyInterdiction", factor: 0.20 },
      { type: "DetectionModifier", factor: 0.75 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T1", "T2", "T3", "T4"],
    etra_ref: "Section 5.7 (Geopolitical Arbitrage) - State-Sponsored Safe Harbors",
    rationale: "Systemic vulnerability due to uneven AML enforcement; difficult to address without international coordination; persistent threat."
  },
  sentiment_market_manipulation_discourse: {
    domain: "financial",
    id: "sentiment_market_manipulation_discourse",
    name: "Synthetic Discourse Market Manipulation",
    description: "AI-generated coordinated fake accounts and messaging manipulate market sentiment for commodities or securities, enabling profitable market-moving trading positions.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 17,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.68 },
      { type: "IntelGain", probability: 0.25 }
    ],
    countered_by: ["synthetic_account_detection"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.8 (Sentiment Laundering) - Market Manipulation",
    rationale: "Scalable manipulation capability; requires market monitoring integration; profits disguise money laundering purpose."
  },
  algorithmic_bribery_detection_evasion: {
    domain: "financial",
    id: "algorithmic_bribery_detection_evasion",
    name: "AI-Optimized Bribery Schemes",
    description: "AI agents design bribery schemes that appear as legitimate consulting contracts, vendor relationships, or professional services, optimized for detection evasion.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 19,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.70 },
      { type: "IntelGain", probability: 0.28 }
    ],
    countered_by: ["official_conduct_monitoring"],
    trl: "6 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6.1 (Algorithmic Bribery)",
    rationale: "Targets government/institutional officials; disguises payments through legitimate contract structures; requires relationship-level verification."
  },
  automated_middleman_corruption: {
    domain: "financial",
    id: "automated_middleman_corruption",
    name: "Synthetic Intermediary Corruption Facilitation",
    description: "AI agents generate fake intermediaries, vendors, and consulting firms to pass bribes through legitimate-appearing business chains, obscuring ultimate beneficiary.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.68 },
      { type: "IntelGain", probability: 0.26 }
    ],
    countered_by: ["vendor_authentication"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6.2 (Automated Middleman)",
    rationale: "Enables corruption at scale; defeats beneficiary ownership tracking; requires supply chain transparency to counter."
  },
  micro_influence_political_spending: {
    domain: "financial",
    id: "micro_influence_political_spending",
    name: "Distributed Micro-Influence Campaign Financing",
    description: "Thousands of AI-controlled small donor accounts make micro-contributions to political campaigns, collectively assembling major funds while evading contribution limits.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.72 },
      { type: "IntelGain", probability: 0.23 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6.3 (Micro-Influence Operations)",
    rationale: "Defeats contribution limit frameworks; enables foreign funding of domestic campaigns; undermines campaign finance transparency."
  },
  procurement_manipulation_government_contracts: {
    domain: "financial",
    id: "procurement_manipulation_government_contracts",
    name: "Government Contract Procurement Manipulation",
    description: "AI agents manipulate government procurement processes through false bids, synthetic competitors, and algorithmic bid optimization to steer contracts to chosen vendors.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 21,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.65 },
      { type: "IntelGain", probability: 0.27 }
    ],
    countered_by: ["bid_pattern_anomaly_detection"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 6.5 (Procurement Optimizer)",
    rationale: "High-value target; scales to billions in diverted spending; detection depends on pattern analysis across procurement systems."
  },
  behavioral_transaction_pattern_detection: {
    domain: "financial",
    id: "behavioral_transaction_pattern_detection",
    name: "Machine Learning Transaction Behavior Analysis",
    description: "AI models trained on legitimate transaction patterns detect anomalous structuring, threshold-adjacent clustering, and sophisticated evasion signatures.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "smurfing_swarm_automated_transactions", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.28 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 17 (Signals and Indicators) - Procurement Pattern Indicators",
    rationale: "Emerging AML capability; high false positive rate without tuning; requires integration across financial institutions."
  },
  cross_bank_correlation: {
    domain: "financial",
    id: "cross_bank_correlation",
    name: "Cross-Institution Transaction Correlation",
    description: "Shared intelligence platforms correlate transactions across banks and jurisdictions, identifying structuring patterns spanning multiple institutions.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "smurfing_swarm_automated_transactions", reduction: 0.40 },
      { type: "IntelGain", probability: 0.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 10 (Cyber-Physical Convergence) - Nano-Smurfing Countermeasures",
    rationale: "Chokepoint control mechanism; requires international coordination; enables detection of distributed structuring strategies."
  },
  network_topology_anomaly_detection: {
    domain: "financial",
    id: "network_topology_anomaly_detection",
    name: "Financial Network Graph Anomaly Detection",
    description: "Graph analysis of transaction networks identifies unusual topologies consistent with layering operations, including circular flows and artificial complexity.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "noise_generation_obfuscation_complexity", reduction: 0.38 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.2 (Noise Generation) - Defender-Centric View",
    rationale: "Sophisticated detection approach; requires transaction graph reconstruction; false positive rate depends on baseline definition."
  },
  defi_transaction_monitoring: {
    domain: "financial",
    id: "defi_transaction_monitoring",
    name: "Decentralized Finance Flow Analysis",
    description: "Blockchain monitoring systems track transactions across DeFi protocols, identifying illicit fund flows through automated bridge and swap chains.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "digital_asset_laundering_defi", reduction: 0.32 },
      { type: "DetectionModifier", factor: 1.22 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.3 (Digital Asset Laundering)",
    rationale: "Emerging technical capability; blockchain immutability enables forensic analysis; protocol diversity complicates coverage."
  },
  beneficial_ownership_registry_verification: {
    domain: "financial",
    id: "beneficial_ownership_registry_verification",
    name: "Automated Beneficial Ownership Verification",
    description: "Systems automatically verify beneficial ownership claims against corporate registries and government databases, flagging inconsistencies and fabricated identities.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 10,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "automated_layering_shell_networks", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.20 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3 (Qualitative Shift) - Defender Siloing",
    rationale: "Regulatory compliance mechanism; effectiveness depends on registry completeness and international coordination."
  },
  entity_creation_velocity_monitoring: {
    domain: "financial",
    id: "entity_creation_velocity_monitoring",
    name: "Rapid Entity Creation Pattern Detection",
    description: "Monitoring systems flag unusual velocity of shell company creation correlated with fund transfers, indicating layering operations.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 9,
    cost_per_tick: 0.05,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "automated_layering_shell_networks", reduction: 0.30 },
      { type: "DetectionModifier", factor: 1.15 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 17 (Signals and Indicators) - Entity Creation Velocity",
    rationale: "Behavioral monitoring approach; requires integration of corporate registration and financial data; low cost to implement."
  },
  regulatory_framework_harmonization: {
    domain: "financial",
    id: "regulatory_framework_harmonization",
    name: "International AML Regulatory Alignment",
    description: "International agreements closing regulatory gaps that enable jurisdiction shopping, including harmonized beneficial ownership requirements and enforcement coordination.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 13,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "geopolitical_arbitrage_safe_harbor", reduction: 0.40 },
      { type: "CounterTech", target: "stochastic_noncompliance_loophole_discovery", reduction: 0.30 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 13 (International Variance) - Regulatory Arbitrage",
    rationale: "Systemic defense requiring international cooperation; high implementation difficulty; addresses root cause of jurisdiction arbitrage."
  },
  synthetic_account_detection: {
    domain: "financial",
    id: "synthetic_account_detection",
    name: "AI-Generated Synthetic Account Identification",
    description: "Machine learning models identify AI-generated social media accounts and personas based on behavioral patterns, network characteristics, and linguistic signatures.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 11,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "sentiment_market_manipulation_discourse", reduction: 0.38 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.3 (Micro-Influence Operations)",
    rationale: "Emerging detection capability; requires platform cooperation; false positive rates high without careful tuning."
  },
  official_conduct_monitoring: {
    domain: "financial",
    id: "official_conduct_monitoring",
    name: "Public Official Conduct Anomaly Monitoring",
    description: "Monitoring systems flag unusual financial activity or decisions by public officials correlated with potential corruption or bribery, enabling investigation.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "algorithmic_bribery_detection_evasion", reduction: 0.32 },
      { type: "IntelGain", probability: 0.20 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.1 (Algorithmic Bribery)",
    rationale: "Privacy-invasive monitoring; requires legal framework; complements FININT with behavioral indicators."
  },
  vendor_authentication: {
    domain: "financial",
    id: "vendor_authentication",
    name: "Vendor Legitimacy and Supply Chain Verification",
    description: "Systems verify vendor authenticity through supply chain transparency, reference checking, and behavioral verification against known legitimate businesses.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "automated_middleman_corruption", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.20 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.2 (Automated Middleman)",
    rationale: "Supply chain integrity mechanism; requires multi-stakeholder participation; complements fraud detection."
  },
  bid_pattern_anomaly_detection: {
    domain: "financial",
    id: "bid_pattern_anomaly_detection",
    name: "Government Procurement Bid Anomaly Detection",
    description: "Analysis of bidding patterns in government procurement identifies unusual clustering, synthetic competitors, and coordinated bid manipulation.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "procurement_manipulation_government_contracts", reduction: 0.38 },
      { type: "IntelGain", probability: 0.22 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.5 (Procurement Optimizer)",
    rationale: "Procurement integrity mechanism; requires government contracting system data access; effective at scale."
  },

// -- IC_EROSION (29 cards) ---------------
  humint_handler_bottleneck_bypass: {
    domain: "ic_erosion",
    id: "humint_handler_bottleneck_bypass",
    name: "Synthetic AI Handler Replacement Infrastructure",
    description: "AI-operated handler functions maintain HUMINT assets at industrial scale, replacing traditional handler-asset relationships with autonomous management of thousands of assets.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 26,
    cost_per_tick: 0.14,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.42 },
      { type: "DetectionModifier", factor: 0.65 }
    ],
    countered_by: ["asset_relationship_baseline_analysis", "handler_communication_signature_detection"],
    trl: "6-7 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 5.1 (HUMINT) - Handler Overload and Synthetic Personas",
    rationale: "Foundational IC degradation vector; removes scale constraint; enables non-state actors to operate HUMINT networks."
  },
  osint_epistemic_baseline_erosion: {
    domain: "ic_erosion",
    id: "osint_epistemic_baseline_erosion",
    name: "AI-Generated False OSINT Contamination",
    description: "AI systems inject high-fidelity synthetic OSINT (fabricated documents, fake social media presence, falsified records) into information environment, contaminating OSINT analysis.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 20,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.60 },
      { type: "IntelGain", probability: 0.35 }
    ],
    countered_by: ["information_provenance_verification", "source_authenticity_validation"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 3.2 (Collection-to-Verification Pivot)",
    rationale: "Undermines OSINT reliability; shifts advantage from information collection to information verification; adversary-friendly dynamic."
  },
  sigint_automated_obfuscation_traffic_shaping: {
    domain: "ic_erosion",
    id: "sigint_automated_obfuscation_traffic_shaping",
    name: "AI-Driven Communications Obfuscation",
    description: "AI agents autonomously shape communications traffic patterns to evade SIGINT analysis, including randomized timing, encryption layer proliferation, and steganographic embedding.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 23,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "CommsDisruption", factor: -0.30 },
      { type: "DetectionModifier", factor: 0.68 }
    ],
    countered_by: ["traffic_pattern_baseline_analysis", "metadata_extraction_capability"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.2 (SIGINT) - Automated Obfuscation",
    rationale: "Counters SIGINT advantage through pattern randomization; arms race dynamic with defender; systematic scaling."
  },
  masint_sensor_spoofing_signature_mimicry: {
    domain: "ic_erosion",
    id: "masint_sensor_spoofing_signature_mimicry",
    name: "AI-Assisted Sensor Spoofing and Deception",
    description: "AI models generate spoofed sensor signatures and signals mimicking legitimate facilities or activities, defeating MASINT (radiological, RF, thermal, electromagnetic) detection.",
    is_offensive: true,
    category: "Cyber",
    deployment_cost: 25,
    cost_per_tick: 0.13,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.62 },
      { type: "IntelGain", probability: 0.30 }
    ],
    countered_by: ["sensor_fusion_redundancy", "signature_baseline_establishment"],
    trl: "5-6 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 5.4 (MASINT) - Sensor Spoofing",
    rationale: "Emerging threat; physical signatures harder to spoof but AI improving; particularly relevant for non-state threats."
  },
  finint_nano_smurfing_financial_obfuscation: {
    domain: "ic_erosion",
    id: "finint_nano_smurfing_financial_obfuscation",
    name: "AI-Orchestrated Financial Transaction Obfuscation",
    description: "AI agents conduct financial intelligence evasion through coordinated nano-smurfing, threshold-adjacent transactions, and cryptocurrency mixing.",
    is_offensive: true,
    category: "Logistics",
    deployment_cost: 21,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "SupplyInterdiction", factor: 0.15 },
      { type: "DetectionModifier", factor: 0.58 }
    ],
    countered_by: ["cross_domain_correlation", "behavioral_financial_modeling"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3", "T4"],
    etra_ref: "Section 5.5 (FININT) - Nano-Smurfing Challenge",
    rationale: "Financial intelligence erosion vector; requires multi-disciplinary defense; scales with adversary computational resources."
  },
  attribution_intent_gap_legal_deniability: {
    domain: "ic_erosion",
    id: "attribution_intent_gap_legal_deniability",
    name: "Plausible Deniability 2.0: Autonomous Agent Liability Evasion",
    description: "State actors deploy AI agents optimizing for political outcomes without directed methodology, creating legal ambiguity about principal responsibility under international law.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 28,
    cost_per_tick: 0.15,
    coverage_limit: null,
    effects: [
      { type: "MoraleEffect", target: "All", delta: -0.25 },
      { type: "DetectionModifier", factor: 0.70 }
    ],
    countered_by: ["state_responsibility_legal_framework", "agent_intent_analysis_methodology"],
    trl: "6-7 (2026)",
    profiles: ["T4"],
    etra_ref: "Section 4.1 (Attribution-Intent Gap)",
    rationale: "Systemic legal framework vulnerability; enables state action while maintaining plausible deniability; undermines deterrence."
  },
  capability_floor_elevation_nonstate: {
    domain: "ic_erosion",
    id: "capability_floor_elevation_nonstate",
    name: "Non-State Intelligence Capability Democratization",
    description: "AI-enabled intelligence operations elevate non-state actor capability floor, enabling sophisticated intelligence collection previously exclusive to state actors.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 24,
    cost_per_tick: 0.13,
    coverage_limit: null,
    effects: [
      { type: "IntelGain", probability: 0.38 },
      { type: "DetectionModifier", factor: 0.72 }
    ],
    countered_by: ["non_state_threat_monitoring", "tier_2_actor_intelligence_analysis"],
    trl: "6-7 (2026)",
    profiles: ["T2", "T3"],
    etra_ref: "Section 3.1 (Capability Floor Elevation)",
    rationale: "Proliferation of intelligence capability; T2-T3 actors now capable of sophisticated targeting; institutional advantage eroded."
  },
  institutional_speed_asymmetry: {
    domain: "ic_erosion",
    id: "institutional_speed_asymmetry",
    name: "IC Decision-Making Latency vs AI Operation Speed",
    description: "AI-enabled threat operations iterate and adapt at speeds exceeding institutional decision-making, creating systematic advantage for offense over defense.",
    is_offensive: true,
    category: "Custom:OperationalTempo",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CombatModifier", factor: 1.3 },
      { type: "DetectionModifier", factor: 0.75 }
    ],
    countered_by: ["rapid_decision_authority_delegation", "real_time_threat_response"],
    trl: "6-7 (2026)",
    profiles: ["T3", "T4"],
    etra_ref: "Section 3.3 (Institutional Speed Asymmetry)",
    rationale: "Structural vulnerability due to bureaucratic constraints; affects IC response capability; advantage accrues to adversary."
  },
  ic_workforce_reduction_verification_crisis: {
    domain: "ic_erosion",
    id: "ic_workforce_reduction_verification_crisis",
    name: "IC Workforce Contraction and Verification Capacity Collapse",
    description: "Simultaneous institutional workforce reductions (ODNI -35%, CIA -1,200, NSA -2,000) occur while verification demands increase due to AI-generated content proliferation.",
    is_offensive: true,
    category: "Custom:InstitutionalPressure",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "DetectionModifier", factor: 0.55 },
      { type: "MoraleEffect", target: "Own", delta: -0.30 }
    ],
    countered_by: ["workforce_investment_rebalancing", "automation_adoption_acceleration"],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.1 (IC Workforce Contraction) - Detection Capacity Crisis",
    rationale: "Compound structural vulnerability; capacity-threat mismatch; systemic pressure on IC institutions."
  },
  delegation_defense_state_responsibility_gap: {
    domain: "ic_erosion",
    id: "delegation_defense_state_responsibility_gap",
    name: "Legal Framework Gap in State Responsibility for Agent Action",
    description: "International law assumes human agents with demonstrable intent; AI autonomy creates legal sinkhole where states claim agents independently derived methodologies.",
    is_offensive: true,
    category: "InformationWarfare",
    deployment_cost: 20,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "MoraleEffect", target: "All", delta: -0.20 },
      { type: "DetectionModifier", factor: 0.68 }
    ],
    countered_by: ["principal_actor_liability_framework"],
    trl: "6-7 (2026)",
    profiles: ["T4"],
    etra_ref: "Section 4.2 (Legal Sinkholes in State Responsibility)",
    rationale: "Enables state action while evading legal accountability; erodes treaty frameworks; international law lag creates exploitation window."
  },
  asset_relationship_baseline_analysis: {
    domain: "ic_erosion",
    id: "asset_relationship_baseline_analysis",
    name: "HUMINT Asset Relationship Authenticity Verification",
    description: "Analysis of asset-handler relationships identifying unnatural communication patterns, relationship maturation inconsistencies, and synthetic persona indicators.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "humint_handler_bottleneck_bypass", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.1 (HUMINT) - New Defensive Methodologies",
    rationale: "Counterintelligence adaptation; requires baseline model training; complements traditional CI tradecraft."
  },
  handler_communication_signature_detection: {
    domain: "ic_erosion",
    id: "handler_communication_signature_detection",
    name: "Anomalous Handler Communication Signature Detection",
    description: "Analysis of handler-asset communications detecting non-human characteristics (perfect grammar, impossible schedule consistency, linguistic anomalies).",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 12,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "humint_handler_bottleneck_bypass", reduction: 0.30 },
      { type: "DetectionModifier", factor: 1.20 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.1 (HUMINT) - Network Analysis and Counterintelligence",
    rationale: "Linguistic-based detection; requires training on AI communication patterns; effectiveness depends on model sophistication."
  },
  information_provenance_verification: {
    domain: "ic_erosion",
    id: "information_provenance_verification",
    name: "Automated Information Source Authentication",
    description: "Systems verify information provenance through document forensics, source history validation, and consistency checking against known legitimate sources.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "osint_epistemic_baseline_erosion", reduction: 0.40 },
      { type: "DetectionModifier", factor: 1.30 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3.2 (Collection-to-Verification Pivot)",
    rationale: "Foundational verification capability; resource-intensive; shifts IC burden from collection to verification."
  },
  source_authenticity_validation: {
    domain: "ic_erosion",
    id: "source_authenticity_validation",
    name: "OSINT Source Legitimacy Assessment",
    description: "Machine learning analysis of OSINT sources identifying fabrication indicators, inconsistencies with historical patterns, and synthetic origin signatures.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "osint_epistemic_baseline_erosion", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.3 (OSINT/GEOINT) - Epistemic Baseline Erosion",
    rationale: "Emerging capability; requires deep learning on source characteristics; false positive rate manageable with tuning."
  },
  traffic_pattern_baseline_analysis: {
    domain: "ic_erosion",
    id: "traffic_pattern_baseline_analysis",
    name: "SIGINT Communications Pattern Baseline Establishment",
    description: "Development of baseline communication patterns for targets enabling anomaly detection even when encryption or obfuscation is present.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "sigint_automated_obfuscation_traffic_shaping", reduction: 0.32 },
      { type: "DetectionModifier", factor: 1.22 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.2 (SIGINT) - Automated Obfuscation",
    rationale: "Metadata-based detection; requires long observation periods; effective against systematic obfuscation."
  },
  metadata_extraction_capability: {
    domain: "ic_erosion",
    id: "metadata_extraction_capability",
    name: "Encrypted Traffic Metadata Analysis",
    description: "Extraction and analysis of communication metadata (timing, volume, endpoints) even from encrypted traffic to detect anomalous patterns.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 15,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "sigint_automated_obfuscation_traffic_shaping", reduction: 0.28 },
      { type: "DetectionModifier", factor: 1.20 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.2 (SIGINT) - Automated Obfuscation",
    rationale: "Metadata-resistant approach; requires passive collection; effectiveness depends on pattern establishment quality."
  },
  sensor_fusion_redundancy: {
    domain: "ic_erosion",
    id: "sensor_fusion_redundancy",
    name: "Multi-Sensor Fusion and Cross-Validation",
    description: "Integration of multiple sensor types (RF, thermal, radiological, electromagnetic) with cross-validation enabling detection of spoofed individual signatures.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 22,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "masint_sensor_spoofing_signature_mimicry", reduction: 0.40 },
      { type: "DetectionModifier", factor: 1.32 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.4 (MASINT) - Sensor Spoofing",
    rationale: "MASINT adaptation; computationally intensive; effective against targeted spoofing; requires multi-sensor deployment."
  },
  signature_baseline_establishment: {
    domain: "ic_erosion",
    id: "signature_baseline_establishment",
    name: "Facility Signature Baseline Cataloging",
    description: "Comprehensive cataloging of signature baselines (radiological, RF, thermal) for known legitimate facilities enabling anomaly detection.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 20,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "masint_sensor_spoofing_signature_mimicry", reduction: 0.35 },
      { type: "DetectionModifier", factor: 1.28 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.4 (MASINT) - Sensor Spoofing",
    rationale: "Signature intelligence adaptation; requires sustained collection effort; enables historical anomaly comparison."
  },
  cross_domain_correlation: {
    domain: "ic_erosion",
    id: "cross_domain_correlation",
    name: "Cross-Intelligence Discipline Correlation Analysis",
    description: "Integration of HUMINT, SIGINT, OSINT, FININT, and MASINT enabling detection of coordinated activities across disciplines.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 19,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "finint_nano_smurfing_financial_obfuscation", reduction: 0.33 },
      { type: "IntelGain", probability: 0.28 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.6 (Summary Risk Matrix)",
    rationale: "IC integration requirement; addresses siloing problem; requires inter-agency data-sharing agreements."
  },
  behavioral_financial_modeling: {
    domain: "ic_erosion",
    id: "behavioral_financial_modeling",
    name: "Adversary Financial Behavior Pattern Modeling",
    description: "Machine learning models trained on known adversary financial operations enabling detection of similar patterns across geographies and time.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 17,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "finint_nano_smurfing_financial_obfuscation", reduction: 0.30 },
      { type: "DetectionModifier", factor: 1.24 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 5.5 (FININT) - Nano-Smurfing Challenge",
    rationale: "FININT modernization; requires historical pattern database; effectiveness depends on pattern stability over time."
  },
  non_state_threat_monitoring: {
    domain: "ic_erosion",
    id: "non_state_threat_monitoring",
    name: "Tier 2-3 Actor Intelligence Operations Monitoring",
    description: "Dedicated monitoring capability for emerging non-state intelligence operations, including threat hunting for AI-enabled collection activities.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 21,
    cost_per_tick: 0.11,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "capability_floor_elevation_nonstate", reduction: 0.38 },
      { type: "IntelGain", probability: 0.32 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.3 (Tier 3 Impact Analysis)",
    rationale: "New mission area for IC; requires different targeting philosophy; emerging threat requiring institutionalization."
  },
  tier_2_actor_intelligence_analysis: {
    domain: "ic_erosion",
    id: "tier_2_actor_intelligence_analysis",
    name: "Regional and Non-State Actor Capability Assessment",
    description: "Analysis capability for assessing Tier 2-3 actor intelligence sophistication, enabling strategic planning for counter-operations.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "capability_floor_elevation_nonstate", reduction: 0.32 },
      { type: "IntelGain", probability: 0.28 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.3 (Tier 2-3 Impact Analysis)",
    rationale: "Analytical capability supporting emerging threat; requires subject-matter expertise in regional actors."
  },
  rapid_decision_authority_delegation: {
    domain: "ic_erosion",
    id: "rapid_decision_authority_delegation",
    name: "Delegated Rapid Response Decision Authority",
    description: "Institutional restructuring delegating response authority to operational elements enabling faster decision cycles matching AI-enabled threat operations.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 12,
    cost_per_tick: 0.06,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "institutional_speed_asymmetry", reduction: 0.35 },
      { type: "MoraleEffect", target: "Own", delta: 0.12 }
    ],
    countered_by: [],
    trl: "6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3.3 (Institutional Speed Asymmetry)",
    rationale: "Organizational adaptation; requires leadership commitment; addresses structural source of speed disadvantage."
  },
  real_time_threat_response: {
    domain: "ic_erosion",
    id: "real_time_threat_response",
    name: "24/7 Operational Threat Response Capability",
    description: "Sustained 24/7 response operations capability enabling real-time decision-making on emerging threats without institutional delays.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 18,
    cost_per_tick: 0.10,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "institutional_speed_asymmetry", reduction: 0.40 },
      { type: "MoraleEffect", target: "Own", delta: 0.10 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 3.3 (Institutional Speed Asymmetry)",
    rationale: "Operational tempo matching; resource-intensive; essential for credible response capability."
  },
  state_responsibility_legal_framework: {
    domain: "ic_erosion",
    id: "state_responsibility_legal_framework",
    name: "International Law Update on AI Principal-Agent Liability",
    description: "Treaty and legal framework updates clarifying state responsibility for AI-autonomous derivations, establishing principal liability regardless of agent autonomy.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 14,
    cost_per_tick: 0.08,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "delegation_defense_plausible_deniability", reduction: 0.40 },
      { type: "CounterTech", target: "attribution_intent_gap_legal_deniability", reduction: 0.38 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 4.2 (Legal Sinkholes) - State Responsibility",
    rationale: "Legal framework closure; requires international negotiation; establishes deterrence through accountability."
  },
  agent_intent_analysis_methodology: {
    domain: "ic_erosion",
    id: "agent_intent_analysis_methodology",
    name: "AI Agent Objective Function Forensic Analysis",
    description: "Methodology for analyzing AI agent decision logs to establish what objective functions and principal instructions drove observed behaviors.",
    is_offensive: false,
    category: "Surveillance",
    deployment_cost: 16,
    cost_per_tick: 0.09,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "attribution_intent_gap_legal_deniability", reduction: 0.35 },
      { type: "IntelGain", probability: 0.25 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 4.1 (Attribution-Intent Gap)",
    rationale: "Attribution methodology for AI-era operations; requires log access; enables forensic establishment of intent."
  },
  workforce_investment_rebalancing: {
    domain: "ic_erosion",
    id: "workforce_investment_rebalancing",
    name: "Intelligence Community Hiring and Retention Investment",
    description: "Sustained workforce investment and retention programs addressing IC capacity degradation and verification workload surge.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 25,
    cost_per_tick: 0.13,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ic_workforce_reduction_verification_crisis", reduction: 0.45 },
      { type: "MoraleEffect", target: "Own", delta: 0.20 }
    ],
    countered_by: [],
    trl: "7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 6.1 (IC Workforce Contraction) - Detection Capacity Crisis",
    rationale: "Foundational institutional measure; requires sustained budget commitment; addresses root cause of capacity crisis."
  },
  automation_adoption_acceleration: {
    domain: "ic_erosion",
    id: "automation_adoption_acceleration",
    name: "AI-Enabled Intelligence Analysis Automation",
    description: "Acceleration of IC adoption of AI-enabled analysis tools to offset human capacity losses and increase verification speed.",
    is_offensive: false,
    category: "Cyber",
    deployment_cost: 22,
    cost_per_tick: 0.12,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "ic_workforce_reduction_verification_crisis", reduction: 0.38 },
      { type: "DetectionModifier", factor: 1.25 }
    ],
    countered_by: [],
    trl: "6-7 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 9 (Policy Recommendations) - Technical Measures",
    rationale: "Defensive automation; enables distributed verification; requires careful implementation to avoid accuracy loss."
  },
  principal_actor_liability_framework: {
    domain: "ic_erosion",
    id: "principal_actor_liability_framework",
    name: "Principal Actor Accountability Framework",
    description: "Legal and policy framework establishing clear accountability chain from AI agents to principal actors, preventing liability diffusion.",
    is_offensive: false,
    category: "Communications",
    deployment_cost: 13,
    cost_per_tick: 0.07,
    coverage_limit: null,
    effects: [
      { type: "CounterTech", target: "delegation_defense_plausible_deniability", reduction: 0.35 },
      { type: "MoraleEffect", target: "All", delta: 0.15 }
    ],
    countered_by: [],
    trl: "5-6 (2026)",
    profiles: ["defender"],
    etra_ref: "Section 4 (Crisis of Intent) - Plausible Deniability 2.0",
    rationale: "Legal framework establishing deterrence through accountability; requires international coordination; foundational for attribution strategy."
  },

};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Metadata about the known threat domains. The `order` field drives
 * the tab order in the Tech Cards UI.
 */
export const DOMAINS = [
  {
    id: 'drone',
    label: 'Drone',
    description: 'Locust ETRA — drone swarms, covert sensors, C-UAS.',
    order: 0,
  },
  {
    id: 'wmd',
    label: 'WMD',
    description: 'ETRA-2026-WMD-001 — AI agents & WMD proliferation.',
    order: 1,
  },
  {
    id: 'espionage',
    label: 'Espionage',
    description: 'ETRA-2026-ESP-001 — AI-scaled intelligence operations.',
    order: 2,
  },
  {
    id: 'political',
    label: 'Political',
    description: 'ETRA-2026-PTR-001 — AI agents & political targeting.',
    order: 3,
  },
  {
    id: 'financial',
    label: 'Financial',
    description: 'ETRA-2025-FIN-001 — AI agents & financial integrity.',
    order: 4,
  },
  {
    id: 'ic_erosion',
    label: 'IC Erosion',
    description: 'ETRA-2026-IC-001 — AI agents & institutional erosion.',
    order: 5,
  },
];

/**
 * Return cards grouped by offensive / defensive. Optional filter by
 * domain id and free-text search (matches name + description + id).
 *
 * @param {object} [opts]
 * @param {string|null} [opts.domain]  One of DOMAINS[].id, or null for all.
 * @param {string} [opts.search]       Free-text search query.
 */
export function groupedCards(opts = {}) {
  const domain = opts.domain || null;
  const q = (opts.search || '').trim().toLowerCase();
  const offensive = [];
  const defensive = [];
  for (const card of Object.values(TECH_LIBRARY)) {
    if (domain && card.domain !== domain) continue;
    if (q) {
      const hay = `${card.id} ${card.name} ${card.description || ''}`.toLowerCase();
      if (!hay.includes(q)) continue;
    }
    if (card.is_offensive) offensive.push(card);
    else defensive.push(card);
  }
  // Stable alpha sort within each group.
  offensive.sort((a, b) => a.name.localeCompare(b.name));
  defensive.sort((a, b) => a.name.localeCompare(b.name));
  return { offensive, defensive };
}

/** Return the total card count per domain. */
export function domainCounts() {
  const counts = {};
  for (const card of Object.values(TECH_LIBRARY)) {
    counts[card.domain] = (counts[card.domain] || 0) + 1;
  }
  return counts;
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
  // Match both single-line and multi-line TOML array forms:
  //   tech_access = ["a", "b"]
  //   tech_access = [
  //     "a",
  //     "b",
  //   ]
  // `[\s\S]*?` is the non-greedy "any char including newline" idiom.
  const taRegex = /\n(\s*)tech_access\s*=\s*\[([\s\S]*?)\]/;
  const match = body.match(taRegex);
  if (match) {
    const currentList = match[2];
    if (currentList.includes(`"${cardId}"`)) return { text: tomlText, granted: false };
    // Preserve whether the existing array is multi-line. If the
    // captured body contains a newline we rebuild it in that style so
    // we don't collapse user formatting.
    const indent = match[1];
    let newLine;
    if (currentList.includes('\n')) {
      const items = currentList
        .split(/\s*,\s*/)
        .map((t) => t.trim())
        .filter((t) => t.length && t !== '"');
      items.push(`"${cardId}"`);
      const itemIndent = indent + '  ';
      newLine = `\n${indent}tech_access = [\n${items
        .map((it) => itemIndent + it)
        .join(',\n')}\n${indent}]`;
    } else {
      const trimmed = currentList.trim();
      const newList = trimmed.length === 0 ? `"${cardId}"` : `${trimmed}, "${cardId}"`;
      newLine = `\n${indent}tech_access = [${newList}]`;
    }
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
