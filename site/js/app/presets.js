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
];
