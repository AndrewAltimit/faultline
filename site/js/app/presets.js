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
];
