import { test } from '@playwright/test';

test('debug - inspect scenario JSON structure', async ({ page }) => {
  await page.goto('/app.html');
  await page.waitForTimeout(2000);

  const info = await page.evaluate(async () => {
    try {
      const resp = await fetch('scenarios/us_institutional_fracture.toml');
      const toml = await resp.text();
      const wasm = await import('/pkg/faultline_backend_wasm.js');
      await wasm.default();
      const scenario = wasm.load_scenario(toml);

      const map = scenario.map;
      const regions = map?.regions;
      const factions = scenario?.factions;

      return {
        mapSourceType: typeof map?.source,
        mapSource: map?.source,
        regionsType: typeof regions,
        regionsIsMap: regions instanceof Map,
        regionsConstructor: regions?.constructor?.name,
        regionCount: regions instanceof Map ? regions.size : (regions ? Object.keys(regions).length : 0),
        regionKeys: regions instanceof Map ? [...regions.keys()] : (regions ? Object.keys(regions) : []),
        factionsType: typeof factions,
        factionsIsMap: factions instanceof Map,
        factionsConstructor: factions?.constructor?.name,
        factionKeys: factions instanceof Map ? [...factions.keys()] : (factions ? Object.keys(factions) : []),
        sampleRegion: regions instanceof Map ? regions.entries().next().value : null,
        sampleFaction: factions instanceof Map ? factions.entries().next().value : null,
      };
    } catch (e) {
      return { error: e.message || String(e), stack: e.stack };
    }
  });

  console.log('Scenario debug:', JSON.stringify(info, null, 2));
});
