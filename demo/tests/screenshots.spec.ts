// Capture the demo's static pages into $SHOTS_OUT as a `captures.json` index plus
// the PNGs it references — the layout screencomp classifies. Managed from
// nickderobertis/screencomp:demo/.
//
// Each page is captured at two viewports, so the gallery renders a `viewport`
// toggle (desktop/mobile) and one card per page you toggle through — the feature
// screencomp exists to show off. Each shot records its own sha256 (the hash IS
// the source of truth; screencomp never re-hashes the PNG), so two captures of one
// build produce identical hashes (the `verify` gate) as long as the pixels are
// byte-reproducible.
//
// The reusable workflow exports SHOTS_OUT (e.g. shots/current/x86_64); locally it
// defaults to shots/current. This spec is the canonical reference for how a
// consumer emits the captures.json schema (schema, name, toggles, hash, image).
// It stays hand-rolled on purpose: the capture runs in the Playwright container,
// where screencomp is not installed. A capture that does have the CLI available
// can write only the PNGs and author the index with
// `screencomp index --input "$SHOTS_OUT" --toggles-from-path` instead.
import { test } from '@playwright/test';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const SHOTS_OUT = process.env.SHOTS_OUT ?? 'shots/current';

const PAGES = [
  ['home', 'index.html'],
  ['about', 'about.html'],
] as const;

// Each viewport is a value of the `viewport` toggle declared in screencomp.toml.
const VIEWPORTS = [
  ['desktop', 1280],
  ['mobile', 390],
] as const;

type Shot = { name: string; toggles: Record<string, string>; hash: string; image: string };
const shots: Shot[] = [];

function pageUrl(file: string): string {
  return pathToFileURL(path.join(__dirname, '..', 'pages', file)).href;
}

for (const [name, file] of PAGES) {
  for (const [viewport, width] of VIEWPORTS) {
    test(`${name} ${viewport}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(pageUrl(file), { waitUntil: 'load' });
      // Static content settles synchronously; no network/animation to await.
      const image = `${name}/${viewport}.png`;
      const dest = path.join(SHOTS_OUT, image);
      mkdirSync(path.dirname(dest), { recursive: true });
      await page.screenshot({ path: dest, fullPage: true });
      const hash = createHash('sha256').update(readFileSync(dest)).digest('hex');
      shots.push({ name, toggles: { viewport }, hash, image });
    });
  }
}

// Assemble the index once every shot is captured. workers=1 (playwright.config.ts)
// keeps `shots` consistent within the single worker.
test.afterAll(() => {
  shots.sort((a, b) =>
    `${a.name} ${a.toggles.viewport}`.localeCompare(`${b.name} ${b.toggles.viewport}`),
  );
  mkdirSync(SHOTS_OUT, { recursive: true });
  writeFileSync(
    path.join(SHOTS_OUT, 'captures.json'),
    `${JSON.stringify({ schema: 1, shots }, null, 2)}\n`,
  );
});
