// Capture the demo's static pages into $SHOTS_OUT/<project>/<name>.png — the
// layout screencomp classifies. Managed from nickderobertis/screencomp:demo/.
//
// The reusable workflow exports SHOTS_OUT (e.g. shots/current/x86_64); locally it
// defaults to shots/current. Each page is static (no animation, time, or random),
// so two captures of one build are byte-identical (the `verify` gate).
import { test } from '@playwright/test';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

const SHOTS_OUT = process.env.SHOTS_OUT ?? 'shots/current';
const PROJECT = 'marketing';

function pageUrl(file: string): string {
  return pathToFileURL(path.join(__dirname, '..', 'pages', file)).href;
}

for (const [name, file] of [
  ['home', 'index.html'],
  ['about', 'about.html'],
] as const) {
  test(name, async ({ page }) => {
    await page.goto(pageUrl(file), { waitUntil: 'load' });
    // Static content settles synchronously; no network/animation to await.
    await page.screenshot({ path: `${SHOTS_OUT}/${PROJECT}/${name}.png`, fullPage: true });
  });
}
