import { readdirSync, readFileSync } from 'node:fs';
import { basename } from 'node:path';
import { expect, type Page } from '@playwright/test';

const contentDirectory = new URL('../../content/', import.meta.url);

export const documents = readdirSync(contentDirectory, { recursive: true })
  .filter((path): path is string => typeof path === 'string' && path.endsWith('.md') && basename(path) !== '404.md')
  .map((path) => ({
    name: basename(path),
    path,
    source: readFileSync(new URL(path.replaceAll('\\', '/'), contentDirectory), 'utf8'),
  }));

export const taskDocuments = documents.filter(({ name, source }) =>
  name !== 'task-reference.md' && /^\| Selector \|/m.test(source));

const sortedTitles = (items: typeof taskDocuments) => items
  .map(({ source }) => source.match(/^title: (.+)$/m)![1])
  .sort((left, right) => left.localeCompare(right, 'en'));

export const updateTaskTitles = sortedTitles(
  taskDocuments.filter(({ source }) => /^\| Commands \|.*`update`/m.test(source)),
);
export const validationTaskTitles = sortedTitles(
  taskDocuments.filter(({ source }) => /^\| Commands \|.*`check`/m.test(source)),
);
export const referenceTitles = ['CLI reference', 'Task reference', ...updateTaskTitles, ...validationTaskTitles];
export const documentRoutes = documents.map(({ name }) =>
  name === 'index.md' ? '/' : `/${name.replace(/\.md$/, '')}`);

export async function assertNoPageOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    page: document.documentElement.scrollWidth,
    viewport: window.innerWidth,
  }));
  expect(dimensions.page).toBeLessThanOrEqual(dimensions.viewport + 1);
}

export async function openSearch(page: Page) {
  // The server-rendered button is visible before Vue attaches its listener.
  // These search tests exercise the hydrated control; startup layout is tested separately.
  await page.waitForLoadState('networkidle');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(page.locator('#localsearch-input')).toBeVisible();
}

export async function guideRoutes(page: Page) {
  const routes = new Set<string>();
  for (const section of ['/getting-started', '/cli-reference', '/contributing']) {
    await page.goto(section);
    for (const href of await page.locator('.VPSidebar a[href]').evaluateAll(
      (links) => links.map((link) => link.getAttribute('href')!),
    )) routes.add(href);
  }
  expect(routes.has('/')).toBe(false);
  expect([...routes, '/'].sort()).toEqual([...documentRoutes].sort());
  return ['/', ...routes];
}

export async function toggleTheme(page: Page, isMobile: boolean) {
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  await page.getByRole('switch').filter({ visible: true }).click();
  if (isMobile) {
    const close = page.getByRole('button', { name: 'mobile navigation', exact: true });
    if (await page.locator('.VPNavScreen').isVisible()) await close.click();
  }
}
