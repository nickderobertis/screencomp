// Playwright config for screencomp's gallery browser tests.
//
// These exist for one reason the Rust suite cannot cover: the gallery ships an
// inline <script> that does the interactive toggle filtering, and the Rust tests
// only assert the static HTML — they never run a browser. This drives the real
// script in chromium. Determinism is not a concern here (we assert DOM state, not
// pixels), so the launch flags are just enough to run headless in CI.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: 'line',
  use: {
    browserName: 'chromium',
    launchOptions: {
      args: ['--headless=new', '--disable-gpu', '--disable-dev-shm-usage', '--hide-scrollbars'],
    },
  },
});
