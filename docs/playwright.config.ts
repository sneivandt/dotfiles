import { defineConfig, devices } from '@playwright/test';

const port = Number(process.env.DOCS_TEST_PORT ?? 8000);

export default defineConfig({
  testDir: './tests/browser',
  fullyParallel: true,
  workers: process.env.CI ? 4 : undefined,
  forbidOnly: Boolean(process.env.CI),
  use: {
    baseURL: `http://localhost:${port}`,
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 1000 } } },
    {
      name: 'mobile',
      testMatch: /responsive\.spec\.ts$/,
      use: { ...devices['Pixel 7'] },
    },
  ],
  webServer: {
    command: 'node tests/preview.mjs',
    url: `http://localhost:${port}`,
    reuseExistingServer: false,
  },
});
