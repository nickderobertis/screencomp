// Playwright config for the screencomp visual-docs demo.
// Managed from nickderobertis/screencomp:demo/ — do not edit in the demo repo.
//
// Determinism is the whole point: screencomp compares screenshots by byte digest
// and `verify` captures the same build twice and requires byte-identical output.
// The launch flags below pick a CPU-independent render path and remove GPU/anti-
// aliasing variance so two captures match. Pinned to the container image's
// Playwright version (1.60.0) so the bundled browsers are used as-is.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: 'line',
  use: {
    browserName: 'chromium',
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    launchOptions: {
      args: [
        '--disable-skia-runtime-opts', // CPU-independent raster path (key flag)
        '--headless=new',
        '--disable-gpu',
        '--disable-gpu-rasterization',
        '--use-gl=angle',
        '--use-angle=swiftshader',
        '--force-color-profile=srgb',
        '--font-render-hinting=none',
        '--disable-lcd-text',
        '--hide-scrollbars',
        '--disable-dev-shm-usage',
      ],
    },
  },
});
