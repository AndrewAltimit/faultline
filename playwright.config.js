import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  outputDir: './tests/browser/screenshots',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:8888',
    screenshot: 'on',
    viewport: { width: 1400, height: 900 },
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
  ],
  webServer: {
    command: 'python3 -m http.server 8888 -d site',
    port: 8888,
    reuseExistingServer: true,
  },
});
