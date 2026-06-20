// Drive the gallery's inline filtering script in a real browser.
//
// The gallery is a single self-contained HTML file whose interactive behavior —
// one page-wide toggle bar that filters every card at once — lives in an inline
// <script>. screencomp's Rust tests assert the static HTML only and never run a
// browser, so a regression in that script (e.g. selection no longer hides
// non-matching cards) would pass them. This builds a real gallery from a fixture
// with the `screencomp` CLI (on PATH; the test-visual-docs.yml self-test builds it
// from source) and asserts the script actually filters.
import { test, expect, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const fixture = path.join(__dirname, '..', 'fixture');
let indexUrl = '';

test.beforeAll(() => {
  try {
    execFileSync('screencomp', ['--version'], { stdio: 'ignore' });
  } catch {
    test.skip(true, 'screencomp is not on PATH (build it with `cargo install --path .`)');
  }
  const out = mkdtempSync(path.join(tmpdir(), 'screencomp-gallery-'));
  execFileSync(
    'screencomp',
    [
      '--config',
      path.join(fixture, 'screencomp.toml'),
      'gallery',
      '--input',
      path.join(fixture, 'shots'),
      '--output',
      out,
      '--title',
      'Gallery browser test',
    ],
    { stdio: 'inherit' },
  );
  const index = path.join(out, 'index.html');
  if (!existsSync(index)) throw new Error(`gallery did not produce ${index}`);
  indexUrl = pathToFileURL(index).href;
});

test.beforeEach(async ({ page }) => {
  await page.goto(indexUrl);
});

// The src of every image the script is currently showing: not itself hidden, and
// not inside a card the script filtered out. Reads the live DOM state the inline
// script applies, so it is independent of whether the placeholder PNGs render.
function visibleSrcs(page: Page): Promise<string[]> {
  return page.$$eval('.variant', (els) =>
    els
      .filter((e) => {
        const img = e as HTMLImageElement;
        const card = img.closest('section.shot') as HTMLElement | null;
        return !img.hidden && !(card && card.hidden);
      })
      .map((e) => e.getAttribute('src') ?? '')
      .sort(),
  );
}

async function pick(page: Page, dim: string, value: string): Promise<void> {
  await page.locator(`.toggle[data-dim="${dim}"] button`, { hasText: value }).click();
}

test('renders exactly one page-wide toggle bar, not one per card', async ({ page }) => {
  await expect(page.locator('.toggles')).toHaveCount(1);
  // Both declared dimensions become controls on that single bar.
  await expect(page.locator('.toggle[data-dim="theme"]')).toHaveCount(1);
  await expect(page.locator('.toggle[data-dim="viewport"]')).toHaveCount(1);
});

function cardHidden(page: Page, name: string): Promise<boolean> {
  return page
    .locator('section.shot', { has: page.locator('h2', { hasText: name }) })
    .evaluate((el) => (el as HTMLElement).hidden);
}

test('the default selection filters every card server-side', async ({ page }) => {
  // Default is the first value of each control: theme=light, viewport=desktop.
  // Each name shows its one matching image; `home` wildcards the viewport it lacks.
  expect(await visibleSrcs(page)).toEqual([
    'home/light.png',
    'legacy/light-desktop.png',
    'settings/light-desktop.png',
  ]);
});

test('changing one control filters every card at once', async ({ page }) => {
  await pick(page, 'viewport', 'mobile');
  // home wildcards viewport (unaffected); settings switches to its mobile shot;
  // legacy has no mobile shot, so its whole card is filtered out.
  expect(await visibleSrcs(page)).toEqual(['home/light.png', 'settings/light-mobile.png']);
  await expect(page.locator('.toggle[data-dim="viewport"] button.active')).toHaveText('mobile');

  await pick(page, 'theme', 'dark');
  expect(await visibleSrcs(page)).toEqual(['home/dark.png', 'settings/dark-mobile.png']);
});

test('a card with no shot for the selection is hidden entirely', async ({ page }) => {
  expect(await cardHidden(page, 'legacy')).toBe(false); // visible under the default
  await pick(page, 'theme', 'dark'); // legacy has only a light shot
  expect(await cardHidden(page, 'legacy')).toBe(true);
});
