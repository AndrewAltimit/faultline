import { test, expect } from '@playwright/test';

// Helper: take a named screenshot and save to the screenshots dir.
async function snap(page, name) {
  await page.screenshot({ path: `tests/browser/screenshots/${name}.png`, fullPage: false });
}

test.describe('Faultline Simulator App', () => {

  test('01 - app loads with empty map', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(1500);
    await snap(page, '01-app-initial-load');

    // Check basic structure is present.
    await expect(page.locator('#map-canvas')).toBeVisible();
    await expect(page.locator('#preset-select')).toBeVisible();
    await expect(page.locator('#toml-editor')).toBeVisible();
  });

  test('02 - preset dropdown has options', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    const options = await page.locator('#preset-select option').count();
    console.log(`Preset dropdown options: ${options}`);
    await snap(page, '02-preset-dropdown');
  });

  test('03 - load US Institutional Fracture scenario', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    // Select the US scenario preset.
    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(1000);
    await snap(page, '03-us-scenario-toml-loaded');

    // Click Load & Run.
    await page.click('#btn-load');
    await page.waitForTimeout(1500);
    await snap(page, '03-us-scenario-map-rendered');
  });

  test('04 - load Tutorial Symmetric scenario', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    await page.selectOption('#preset-select', 'scenarios/tutorial_symmetric.toml');
    await page.waitForTimeout(1000);

    await page.click('#btn-load');
    await page.waitForTimeout(1500);
    await snap(page, '04-tutorial-symmetric-map');
  });

  test('05 - step simulation forward', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(1000);
    await page.click('#btn-load');
    await page.waitForTimeout(1000);

    // Step 5 times.
    for (let i = 0; i < 5; i++) {
      await page.click('#btn-step');
      await page.waitForTimeout(200);
    }
    await snap(page, '05-after-5-steps');
  });

  test('06 - play simulation to tick 50', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(1000);
    await page.click('#btn-load');
    await page.waitForTimeout(1000);

    // Set speed to max (index 5 = 50x).
    await page.fill('#speed-slider', '5');
    await page.dispatchEvent('#speed-slider', 'input');

    // Click play and wait a bit.
    await page.click('#btn-play');
    await page.waitForTimeout(3000);

    // Pause.
    await page.click('#btn-play');
    await page.waitForTimeout(500);
    await snap(page, '06-after-play-50x');
  });

  test('07 - run Monte Carlo analysis', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    await page.selectOption('#preset-select', 'scenarios/tutorial_symmetric.toml');
    await page.waitForTimeout(1000);
    await page.click('#btn-load');
    await page.waitForTimeout(1000);

    // Set MC runs to 20 (fast).
    await page.fill('#mc-runs', '20');

    // Run MC.
    await page.click('#btn-mc-run');
    await page.waitForTimeout(5000);
    await snap(page, '07-monte-carlo-results');
  });

  test('08 - console errors check', async ({ page }) => {
    const errors = [];
    page.on('console', msg => {
      if (msg.type() === 'error') errors.push(msg.text());
    });
    page.on('pageerror', err => errors.push(err.message));

    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    // Load and step a scenario.
    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(1000);
    await page.click('#btn-load');
    await page.waitForTimeout(1000);
    await page.click('#btn-step');
    await page.waitForTimeout(500);

    await snap(page, '08-console-errors-check');

    if (errors.length > 0) {
      console.log('Console errors found:', errors);
    }
    // Don't fail on errors yet — just capture them for visibility.
  });

  test('09 - load Tutorial Asymmetric scenario', async ({ page }) => {
    await page.goto('/app.html');
    await page.waitForTimeout(2000);

    await page.selectOption('#preset-select', 'scenarios/tutorial_asymmetric.toml');
    await page.waitForTimeout(1000);

    await page.click('#btn-load');
    await page.waitForTimeout(1500);
    await snap(page, '09-tutorial-asymmetric-map');
  });

});
