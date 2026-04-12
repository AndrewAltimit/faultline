/**
 * Bundled scenario presets.
 * Scenarios are fetched from the site/scenarios/ directory.
 */

export const PRESETS = [
  {
    name: 'Tutorial: Symmetric Conflict',
    path: 'scenarios/tutorial_symmetric.toml',
    description: 'Two equal factions on a 2x2 grid. Pure Lanchester attrition.',
  },
  {
    name: 'Tutorial: Asymmetric Conflict',
    path: 'scenarios/tutorial_asymmetric.toml',
    description: 'Government vs insurgent. Tech cards, events, fog of war.',
  },
  {
    name: 'US Institutional Fracture',
    path: 'scenarios/us_institutional_fracture.toml',
    description: '4-faction institutional crisis across 8 US macro-regions.',
  },
  {
    name: 'Drone Swarm Destabilization',
    path: 'scenarios/drone_swarm_destabilization.toml',
    description: 'Multi-phase autonomous drone swarm campaign — kill chain from sensor emplacement through coercion.',
  },
  {
    name: 'Europe — Eastern Flank',
    path: 'scenarios/europe_eastern_flank.toml',
    description: 'NATO vs Russia with Ukraine as pivot. Demonstrates the bundled Europe map and drone-swarm tech cards.',
  },
  {
    name: 'Drone Threat Capabilities Demo',
    path: 'scenarios/capabilities_demo.toml',
    description: 'Sandbox scenario exercising every tech card in the bundled Drone Threat Library.',
  },
  {
    name: 'Compound Kill Chains — Defensive Planning Wargame',
    path: 'scenarios/compound_kill_chains.toml',
    description: 'Research simulation of three concurrent archetypal red-team campaigns (intelligence-led pressure, non-lethal capability demonstration, cyber-physical convergence) against a notional integrated defender. Exercises the multi-phase kill chain schema and cost-asymmetry analysis.',
  },
  {
    name: 'Persistent Covert Surveillance Network — Defensive Wargame',
    path: 'scenarios/persistent_covert_surveillance.toml',
    description: 'Long-dwell commodity-component surveillance campaign against a notional federal protective posture. Quantifies the detection window, attribution confidence, and cost-asymmetry ratio between an ESP32-class sensor footprint and the inspection / remediation program required to close the gap.',
  },
  {
    name: 'European Energy Infrastructure Sabotage — Defensive Wargame',
    path: 'scenarios/europe_energy_sabotage.toml',
    description: 'Multi-phase covert campaign against European cross-border energy corridors. Exercises the kill chain schema to quantify cost-asymmetry, detection window, and attribution confidence for NATO / EU critical infrastructure protection planning.',
  },
];
