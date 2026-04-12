/**
 * Polish-features end-to-end coverage:
 *   - Scenario sharing via #scenario= URL hash (encode/auto-load).
 *   - Monte Carlo runs in a web worker (UI stays responsive,
 *     time-sliced regional control heatmap renders).
 *   - Sensitivity sweep produces a tornado chart.
 *
 * These tests build on the existing app.spec.js conventions: snap a
 * screenshot at each interesting checkpoint and assert on observable
 * DOM state. The webServer in playwright.config.js serves site/ over
 * http://localhost:8888.
 */

import { test, expect } from '@playwright/test';

async function snap(page, name) {
  await page.screenshot({
    path: `tests/browser/screenshots/${name}.png`,
    fullPage: false,
  });
}

/**
 * Mirror site/js/app/sharing.js's encodeScenario for use in Node so we
 * don't have to bounce through page.evaluate. Node 22 ships
 * CompressionStream natively.
 */
async function encodeForHash(toml) {
  const stream = new Blob([toml]).stream().pipeThrough(new CompressionStream('gzip'));
  const compressed = new Uint8Array(await new Response(stream).arrayBuffer());
  let binary = '';
  for (let i = 0; i < compressed.length; i++) binary += String.fromCharCode(compressed[i]);
  return Buffer.from(binary, 'binary')
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

// Wait for the WASM module to finish loading. The bootstrap code hides
// #map-loading once the module is initialized — that's the cleanest
// signal that AppState._WasmEngine is wired up.
async function waitForWasmReady(page) {
  await page.waitForFunction(
    () => {
      const overlay = document.getElementById('map-loading');
      return !overlay || overlay.style.display === 'none';
    },
    { timeout: 15000 },
  );
}

test.describe('Polish features', () => {

  // -------------------------------------------------------------------
  // Scenario sharing (URL hash round-trip)
  // -------------------------------------------------------------------

  test('P5-01 — Share button copies a #scenario= URL with current TOML', async ({
    page,
    context,
  }) => {
    // Grant clipboard permissions so navigator.clipboard.writeText
    // doesn't reject. Without this, the editor falls back to placing
    // the URL in the address bar — which is also valid behavior, but
    // we'd lose the clipboard assertion below.
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);

    await page.goto('/app.html');
    await waitForWasmReady(page);

    // Wait for the default tutorial scenario to populate the editor.
    await page.waitForFunction(
      () => document.getElementById('toml-editor')?.value?.length > 0,
      { timeout: 5000 },
    );

    // Click Share. The handler updates #validation-msg with a
    // success or error message — we wait for "copied" to appear.
    await page.click('#btn-share');
    await page.waitForFunction(
      () => /copied|address bar/i.test(
        document.getElementById('validation-msg')?.textContent || '',
      ),
      { timeout: 5000 },
    );
    await snap(page, 'p5-01-share-clicked');

    // Read what the page actually wrote to the clipboard. If the
    // browser refuses, fall back to inspecting the URL hash directly.
    let shared = '';
    try {
      shared = await page.evaluate(() => navigator.clipboard.readText());
    } catch (_) {
      shared = await page.evaluate(() => window.location.href);
    }
    expect(shared).toMatch(/#scenario=[A-Za-z0-9_-]+/);
  });

  test('P5-02 — Auto-loads a scenario from a #scenario= URL hash', async ({ page }) => {
    // We pre-compute the encoded payload in Node using the same
    // CompressionStream API the browser sharing.js uses, then make
    // exactly one page.goto with the hash already set. This avoids
    // the same-document fragment-navigation gotcha (where a second
    // goto to /app.html#... wouldn't re-run bootstrap because the
    // path didn't change).
    const original = '[meta]\nname = "Share roundtrip"\nauthor = "test"\nversion = "0.0.0"\ndescription = "test"\ntags = []\n';
    const encoded = await encodeForHash(original);

    await page.goto(`/app.html#scenario=${encoded}`);
    await waitForWasmReady(page);

    // The bootstrap path should populate the editor with the decoded
    // TOML and strip the hash from the URL.
    await page.waitForFunction(
      () => {
        const ta = document.getElementById('toml-editor');
        return ta && ta.value.includes('Share roundtrip');
      },
      { timeout: 5000 },
    );
    const editorText = await page.locator('#toml-editor').inputValue();
    expect(editorText).toBe(original);

    // The hash should be cleared so the user doesn't accidentally
    // re-share the URL with stale content.
    const finalUrl = await page.evaluate(() => window.location.href);
    expect(finalUrl).not.toMatch(/#scenario=/);

    await snap(page, 'p5-02-shared-url-loaded');
  });

  // -------------------------------------------------------------------
  // Monte Carlo web worker + regional heatmap
  // -------------------------------------------------------------------

  test('P5-03 — Monte Carlo runs in a worker and renders the heatmap', async ({ page }) => {
    // Capture console errors as we go — the worker path is the most
    // likely place for "wasm not initialized" or postMessage clone
    // errors to surface, and we want to fail loudly if any appear.
    const errors = [];
    page.on('pageerror', (err) => errors.push(err.message));
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/app.html');
    await waitForWasmReady(page);

    // Load a scenario big enough to be interesting on the heatmap.
    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(500);
    await page.click('#btn-load');
    await page.waitForTimeout(500);

    // Run a small MC batch — the worker path should populate the
    // results container without freezing the UI thread.
    await page.fill('#mc-runs', '10');
    await page.click('#btn-mc-run');

    // Wait for the regional control heatmap canvas to appear. Its id
    // is added to the DOM only after _renderMcResults runs, which
    // only runs after the worker resolves.
    await page.waitForSelector('#chart-heatmap', { timeout: 30000 });
    await snap(page, 'p5-03-mc-with-heatmap');

    // The heatmap canvas should be sized non-trivially.
    const dims = await page.evaluate(() => {
      const c = document.getElementById('chart-heatmap');
      return c ? { w: c.width, h: c.height } : null;
    });
    expect(dims).not.toBeNull();
    expect(dims.w).toBeGreaterThan(0);
    expect(dims.h).toBeGreaterThan(0);

    // The MC button should have re-enabled (handler ran finally{}).
    const btnState = await page.locator('#btn-mc-run').isDisabled();
    expect(btnState).toBe(false);

    // No console errors should have leaked from the worker path.
    if (errors.length > 0) {
      console.log('console errors during MC worker run:', errors);
    }
    expect(errors.filter((e) => /wasm|worker|clone/i.test(e))).toEqual([]);
  });

  test('P5-04 — UI stays interactive while a Monte Carlo batch is running', async ({ page }) => {
    // The whole point of moving MC into a worker is that the main
    // thread doesn't freeze. We test this by kicking off a batch and
    // immediately interacting with the UI — if the main thread were
    // blocked, the click wouldn't register until the batch finished.
    await page.goto('/app.html');
    await waitForWasmReady(page);

    await page.selectOption('#preset-select', 'scenarios/us_institutional_fracture.toml');
    await page.waitForTimeout(300);
    await page.click('#btn-load');
    await page.waitForTimeout(300);

    await page.fill('#mc-runs', '50');
    await page.click('#btn-mc-run');

    // Immediately try to interact with the editor tab. If the main
    // thread is blocked by a synchronous wasm call, this click will
    // be queued and the tab won't switch until MC finishes.
    const tabSwitchedAt = Date.now();
    await page.click('.app-tab[data-tab="tab-builder"]');
    await page.waitForFunction(
      () => document.querySelector('.app-tab[data-tab="tab-builder"]')?.classList.contains('active'),
      { timeout: 1000 },
    );
    const tabSwitchMs = Date.now() - tabSwitchedAt;
    // 1 second is generous — a blocked main thread on a 50-run MC of
    // the fracture scenario typically takes several seconds.
    expect(tabSwitchMs).toBeLessThan(1000);

    // Now wait for MC to finish.
    await page.waitForSelector('#chart-heatmap', { timeout: 30000 });
    await snap(page, 'p5-04-ui-responsive-during-mc');
  });

  // -------------------------------------------------------------------
  // Sensitivity tornado chart
  // -------------------------------------------------------------------

  test('P5-05 — Sensitivity sweep produces a tornado chart', async ({ page }) => {
    await page.goto('/app.html');
    await waitForWasmReady(page);

    await page.selectOption('#preset-select', 'scenarios/tutorial_asymmetric.toml');
    await page.waitForTimeout(500);
    await page.click('#btn-load');
    await page.waitForTimeout(500);

    // Configure the sweep: tension across [0, 1], 4 steps, 5 runs each.
    await page.selectOption('#sens-param', 'political_climate.tension');
    await page.fill('#sens-low', '0');
    await page.fill('#sens-high', '1');
    await page.fill('#sens-steps', '4');
    await page.fill('#sens-runs', '5');

    await page.click('#btn-sens-run');

    // The tornado canvas appears once _renderSensitivityResults runs.
    await page.waitForSelector('#chart-tornado', { timeout: 30000 });
    await snap(page, 'p5-05-tornado-chart');

    // Verify the chart canvas was actually drawn (non-zero size).
    const dims = await page.evaluate(() => {
      const c = document.getElementById('chart-tornado');
      return c ? { w: c.width, h: c.height } : null;
    });
    expect(dims).not.toBeNull();
    expect(dims.w).toBeGreaterThan(0);

    // The per-step duration table should have one entry per sweep step.
    const stepCount = await page.locator('#sens-results .mc-stat').count();
    expect(stepCount).toBe(4);

    // Sweep button should re-enable.
    expect(await page.locator('#btn-sens-run').isDisabled()).toBe(false);
  });

  test('P5-06 — Sensitivity input validation rejects inverted ranges', async ({ page }) => {
    // The inline validator should refuse low > high without making a
    // wasm call (which would otherwise return a stats error string).
    // Either error message is acceptable; we just want to confirm the
    // UI surfaces *something* and doesn't draw a chart.
    await page.goto('/app.html');
    await waitForWasmReady(page);

    await page.selectOption('#preset-select', 'scenarios/tutorial_symmetric.toml');
    await page.waitForTimeout(500);
    await page.click('#btn-load');
    await page.waitForTimeout(500);

    await page.fill('#sens-low', '0.9');
    await page.fill('#sens-high', '0.1');
    await page.click('#btn-sens-run');

    // No tornado chart should appear; an error message should.
    await page.waitForSelector('#sens-results .validation-msg.error', { timeout: 5000 });
    expect(await page.locator('#chart-tornado').count()).toBe(0);
    await snap(page, 'p5-06-sens-invalid-range');
  });

});
