import { expect, test, type Page } from '@playwright/test';
import {
  assertNoPageOverflow,
  documentRoutes,
  documents,
  guideRoutes,
  openSearch,
  referenceTitles,
  toggleTheme,
  updateTaskTitles,
  validationTaskTitles,
} from './helpers';

async function assertDiagramReadable(page: Page) {
  const diagram = page.locator('.mermaid svg');
  // Mermaid loads asynchronously; shared runners can delay a cold render.
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  await expect(diagram.locator('title')).not.toBeEmpty();
  await expect(diagram.locator('desc')).not.toBeEmpty();
  await expect(diagram).toHaveAttribute('aria-labelledby', /.+/);
  const labels = await diagram.evaluate((svg) => {
    const viewBox = svg.getAttribute('viewBox')?.split(/\s+/).map(Number);
    if (!viewBox) throw new Error('Diagram has no viewBox');
    const scale = svg.getBoundingClientRect().width / viewBox[2];
    return [...svg.querySelectorAll('.nodeLabel')].map((label) => parseFloat(getComputedStyle(label).fontSize) * scale);
  });
  expect(labels.length).toBeGreaterThan(0);
  for (const size of labels) expect(size).toBeGreaterThanOrEqual(14);
  const frame = page.locator('.mermaid');
  expect(await frame.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await assertNoPageOverflow(page);
  return await diagram.getAttribute('id');
}

for (const theme of ['light', 'dark'] as const) {
  test(`header, search, and outline stay stable during startup in ${theme} mode`, async ({ page, isMobile }) => {
    await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
    let start!: () => void;
    const scripts = new Promise<void>((resolve) => { start = resolve; });
    await page.route(/\.js(?:\?|$)/, async (route) => {
      await scripts;
      await route.continue();
    });
    const errors: string[] = [];
    page.on('console', (message) => {
      if (/hydration/i.test(message.text())) errors.push(message.text());
    });
    const appearance = () => page.evaluate(() => {
      const selectors = ['.VPNavBar', '.VPNavBar .title', '#local-search button', '.VPDocAsideOutline'];
      return selectors.map((selector) => {
        const element = document.querySelector(selector)!;
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return { x: rect.x, y: rect.y, width: rect.width, height: rect.height,
          background: style.backgroundColor, color: style.color };
      });
    });
    try {
      await page.goto('/getting-started', { waitUntil: 'commit' });
      await expect(page.locator('.VPNavBar')).toHaveClass(/has-sidebar/);
      await expect(page.locator('#local-search button')).toBeVisible();
      await page.evaluate(async () => { await document.fonts.ready; });
      const outline = page.locator('.VPDocAsideOutline .outline-link');
      const headings = await outline.allTextContents();
      expect(headings.length).toBeGreaterThan(0);
      if (!isMobile) await expect(outline.first()).toBeVisible();
      const before = await appearance();
      start();
      await page.waitForLoadState('networkidle');
      expect(await appearance()).toEqual(before);
      await expect(outline).toHaveText(headings);
      expect(errors).toEqual([]);
      await openSearch(page);
      await expect(page.locator('.VPLocalSearchBox')).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(page.locator('.VPLocalSearchBox')).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Search', exact: true })).toBeFocused();
    } finally {
      start();
      await page.unrouteAll({ behavior: 'wait' });
    }
  });
}

test('the hero landing page and grouped Guides navigation work cleanly', async ({ page, isMobile }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Dotfiles by Stuart Neivandt | Arch Linux & Windows Rust CLI');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Dotfiles');
  const heroImage = page.locator('.VPHome .VPHero .image-src');
  await expect(heroImage).toHaveAttribute('src', '/assets/logo-hero.png');
  await expect(heroImage).toHaveAttribute('alt', 'Dotfiles by Stuart Neivandt logo');
  expect(await heroImage.evaluate((element: HTMLImageElement) => ({
    complete: element.complete,
    width: element.naturalWidth,
    height: element.naturalHeight,
  }))).toEqual({ complete: true, width: 512, height: 512 });
  const heroWordmark = await page.locator('.VPHome .VPHero .heading .name.clip').evaluate((element) => {
    const style = getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    return {
      mask: style.maskImage !== 'none' ? style.maskImage : style.webkitMaskImage,
      width: rect.width,
      height: rect.height,
    };
  });
  expect(heroWordmark.mask).toMatch(/url\(/);
  expect(heroWordmark.width).toBeGreaterThan(0);
  expect(heroWordmark.height).toBeGreaterThan(0);
  expect(heroWordmark.width / heroWordmark.height).toBeCloseTo(193.7 / 42.9, 1);
  await expect(page.locator('meta[name="author"]')).toHaveAttribute('content', 'Stuart Neivandt');
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute('content', '#1a1b26');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'index, follow, max-image-preview:large');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('meta[property="og:type"]')).toHaveAttribute('content', 'website');
  await expect(page.locator('meta[property="og:site_name"]')).toHaveAttribute('content', 'Dotfiles');
  await expect(page.locator('meta[property="og:locale"]')).toHaveAttribute('content', 'en_US');
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('meta[property="og:image"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/assets/og-image.png');
  await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
  await expect(page.locator('meta[name="twitter:image"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/assets/og-image.png');
  await expect(page.locator('.VPFeatures .VPFeature')).toHaveCount(0);
  const homeAbout = page.locator('.home-about');
  await expect(homeAbout.getByRole('heading', { name: 'About this project' })).toBeVisible();
  await expect(homeAbout.locator('p').first()).not.toBeEmpty();
  await expect(homeAbout.getByRole('link', { name: 'Getting started' })).toHaveAttribute('href', '/getting-started');
  await expect(page.locator('img[src="/assets/terminal-screenshot.svg"]')).toHaveCount(0);
  const [heroBox, aboutBox, footerBox] = await Promise.all([
    page.locator('.VPHome .VPHero .container').boundingBox(),
    homeAbout.boundingBox(),
    page.locator('.site-footer--home').boundingBox(),
  ]);
  expect(Math.abs(heroBox!.x - aboutBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(heroBox!.width - aboutBox!.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(heroBox!.x - footerBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(heroBox!.width - footerBox!.width)).toBeLessThanOrEqual(1);
  const footer = page.locator('.site-footer');
  await expect(footer).toHaveText('Created by Stuart Neivandt.');
  const footerLink = footer.getByRole('link', { name: 'Stuart Neivandt', exact: true });
  await expect(footerLink).toBeVisible();
  await expect(footerLink).toHaveAttribute('href', 'https://stuartneivandt.com/');
  expect(JSON.parse(await page.locator('head script[type="application/ld+json"]').textContent() ?? '')).toMatchObject({
    '@context': 'https://schema.org',
    '@graph': [
      { '@type': 'WebSite', name: 'Dotfiles', url: 'https://dotfiles.stuartneivandt.com/', image: { '@id': 'https://dotfiles.stuartneivandt.com/#logo-image' }, creator: { '@id': 'https://stuartneivandt.com/#person' } },
      { '@type': 'WebPage', name: 'Dotfiles by Stuart Neivandt | Arch Linux & Windows Rust CLI', url: 'https://dotfiles.stuartneivandt.com/', primaryImageOfPage: { '@id': 'https://dotfiles.stuartneivandt.com/#logo-image' } },
      { '@type': 'ImageObject', '@id': 'https://dotfiles.stuartneivandt.com/#logo-image', url: 'https://dotfiles.stuartneivandt.com/assets/logo-hero.png', width: 512, height: 512 },
      { '@type': 'SoftwareSourceCode', name: 'Dotfiles', codeRepository: 'https://github.com/sneivandt/dotfiles', programmingLanguage: 'Rust', image: { '@id': 'https://dotfiles.stuartneivandt.com/#logo-image' } },
      { '@type': 'Person', '@id': 'https://stuartneivandt.com/#person', name: 'Stuart Neivandt', alternateName: 'sneivandt', url: 'https://stuartneivandt.com/' },
    ],
  });
  await expect(page.locator('link[rel="icon"][type="image/svg+xml"]')).toHaveAttribute('href', '/favicon.svg');
  await expect(page.locator('link[rel="icon"][type="image/png"]')).toHaveAttribute('href', '/favicon.png');
  await expect(page.locator('link[rel="alternate icon"]')).toHaveAttribute('href', '/favicon.ico');
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute('href', '/apple-touch-icon.png');
  await expect(page.locator('.VPNavBarTitle a')).toHaveAccessibleName('Dotfiles');
  await expect(page.locator('.VPNavBarTitle img.logo.dark')).toBeVisible();
  await expect(page.locator('.VPNavBarTitle img.logo.light')).toBeHidden();
  const setup = page.locator('.VPHero').getByRole('link', { name: 'Getting started', exact: true });
  await expect(setup).toBeInViewport();
  await expect(setup).toHaveAttribute('href', '/getting-started');
  const actionTops = await page.locator('.VPHero .actions .VPButton').evaluateAll(
    (buttons) => buttons.map((button) => Math.round(button.getBoundingClientRect().top)),
  );
  expect(actionTops).toHaveLength(3);
  expect(new Set(actionTops).size).toBe(1);
  if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveCount(0);
  await assertNoPageOverflow(page);

  await setup.click();
  await expect(page).toHaveURL('/getting-started');
  await expect(page).toHaveTitle('Getting started | Dotfiles');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Getting started');
  await expect(footerLink).toBeVisible();
  await expect(footerLink).toHaveAttribute('href', 'https://stuartneivandt.com/');
  await expect(page.locator('.VPDocFooter .prev')).toHaveCount(0);
  await expect(page.locator('.VPDocFooter .next')).toContainText('Commands and usage');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const sidebar = page.locator('.VPSidebar');
  const groups = sidebar.locator('.group');
  await expect(groups.locator('h2')).toHaveText(['Introduction', 'Customization', 'Platforms', 'Safety and troubleshooting']);
  for (const [label, titles] of [
    ['Introduction', ['Getting started', 'Commands and usage', 'Profiles and categories', 'Configuration basics']],
    ['Customization', ['Package management', 'Application settings', 'Agent configuration', 'Private overlays']],
    ['Platforms', ['Arch desktop', 'Windows and WSL']],
    ['Safety and troubleshooting', ['Security and trust', 'Troubleshooting']],
  ] as const) {
    await expect(groups.filter({ has: page.getByRole('heading', { name: label, exact: true }) }).getByRole('link')).toHaveText([...titles]);
  }
  await expect(sidebar.getByRole('link', { name: 'About this project', exact: true })).toHaveCount(0);
  if (isMobile) {
    await page.keyboard.press('Escape');
    await expect(sidebar).not.toHaveClass(/open/);
  } else {
    await expect(page.locator('.VPNavBarMenu a.active')).toHaveText('Guides');
  }
  await page.locator('.VPNavBarTitle a').click();
  await expect(page).toHaveURL('/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Dotfiles');
  await assertNoPageOverflow(page);
});

test('header sections replace the sidebar, including direct links and search destinations', async ({ page, isMobile }) => {
  await page.goto('/getting-started');
  await expect(page).toHaveTitle('Getting started | Dotfiles');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'noindex, follow');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://dotfiles.stuartneivandt.com/getting-started');
  await expect(page.locator('meta[property="og:type"]')).toHaveAttribute('content', 'article');
  await expect(page.locator('meta[property="og:site_name"]')).toHaveAttribute('content', 'Dotfiles');
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/getting-started');
  await expect(page.locator('meta[property="og:image"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/assets/og-image.png');
  await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
  await expect(page.locator('.site-footer').getByRole('link', { name: 'Stuart Neivandt', exact: true })).toHaveAttribute('href', 'https://stuartneivandt.com/');
  for (const [label, entry, titles] of [
    ['Reference', '/cli-reference', referenceTitles],
    ['Development', '/contributing', ['Contributing', 'Architecture', 'Adding a task', 'Debugging', 'Writing documentation', 'Testing', 'Writing tests', 'Mutation testing', 'CI and releases']],
    ['Guides', '/getting-started', ['Getting started', 'Commands and usage', 'Profiles and categories', 'Configuration basics', 'Package management', 'Application settings', 'Agent configuration', 'Private overlays', 'Arch desktop', 'Windows and WSL', 'Security and trust', 'Troubleshooting']],
  ] as const) {
    if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
    await page.locator(isMobile ? '.VPNavScreen' : '.VPNavBarMenu').getByRole('link', { name: label, exact: true }).click();
    await expect(page).toHaveURL(entry);
    await expect(page.locator('.VPSidebar a')).toHaveText([...titles]);
    if (label === 'Development') {
      await expect(page.locator('.VPSidebar .group h2'))
        .toHaveText(['Overview', 'Implementation', 'Testing and releases']);
    }
    if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText(label);
  }
  await page.goto('/home-symlinks#configuration');
  await expect(page.locator('.VPSidebar a')).toHaveText(referenceTitles);
  await expect(page.locator('.VPDocFooter .next')).toContainText('Login shell');
  await expect(page.locator('.VPSidebar').getByRole('button', { name: 'toggle section', exact: true })).toHaveCount(2);
  await page.goto('/windows-and-wsl');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Security and trust');
  await page.goto('/security-and-trust');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Troubleshooting');
  await page.goto('/troubleshooting');
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
});

test('task groups collapse separately from the task reference overview', async ({ page, isMobile }) => {
  await page.goto('/home-symlinks');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const sidebar = page.locator('.VPSidebar');
  const overviewGroup = sidebar.locator('.group')
    .filter({ has: page.getByRole('link', { name: 'Task reference', exact: true }) });
  const updateTasks = sidebar.locator('.group')
    .filter({ has: page.getByRole('heading', { name: 'Update tasks', exact: true }) });
  const validationTasks = sidebar.locator('.group')
    .filter({ has: page.getByRole('heading', { name: 'Validation tasks', exact: true }) });
  await expect(overviewGroup).toHaveCount(1);
  await expect(overviewGroup.getByRole('link')).toHaveText(referenceTitles.slice(0, 2));
  await expect(updateTasks).toHaveCount(1);
  await expect(updateTasks.getByRole('link')).toHaveText(updateTaskTitles);
  await expect(validationTasks).toHaveCount(1);
  await expect(validationTasks.getByRole('link')).toHaveText(validationTaskTitles);
  await expect(sidebar.getByRole('link', { name: 'Update tasks', exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole('link', { name: 'Validation tasks', exact: true })).toHaveCount(0);
  const toggle = updateTasks.getByRole('button', { name: 'toggle section', exact: true });
  await toggle.click();
  await expect(page).toHaveURL('/home-symlinks');
  await expect(updateTasks.getByRole('link', { name: 'Home symlinks', exact: true })).not.toBeVisible();
  await expect(validationTasks.getByRole('link', { name: 'Configuration diagnostics', exact: true })).toBeVisible();
  await expect(sidebar.getByRole('link', { name: 'CLI reference', exact: true })).toBeVisible();
  await expect(sidebar.getByRole('link', { name: 'Configuration basics', exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole('link', { name: 'Task reference', exact: true })).toBeVisible();
  await sidebar.getByRole('link', { name: 'Task reference', exact: true }).click();
  await expect(page).toHaveURL('/task-reference');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Task reference');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Agent packages');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await expect(updateTasks.getByRole('link', { name: 'Home symlinks', exact: true })).not.toBeVisible();
  await toggle.click();
  await expect(updateTasks.getByRole('link', { name: 'Home symlinks', exact: true })).toBeVisible();
  const last = validationTasks.getByRole('link', { name: 'Symlink sources', exact: true });
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeInViewport();
  await last.click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Symlink sources');
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

test('configuration basics follows profiles in Guides, including search destinations', async ({ page, isMobile }) => {
  await page.goto('/profiles-and-categories');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Configuration basics');
  await page.locator('.VPDocFooter .next').click();
  await expect(page).toHaveURL('/configuration-basics');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Configuration basics');
  await expect(page.locator('.VPDocFooter .prev')).toContainText('Profiles and categories');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Package management');
  if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText('Guides');

  await page.goto('/cli-reference');
  await openSearch(page);
  await page.locator('#localsearch-input').fill('Configuration basics');
  const result = page.locator('.VPLocalSearchBox a[href*="/configuration-basics"]').first();
  await expect(result).toBeVisible({ timeout: 30_000 });
  await result.click();
  await expect(page).toHaveURL(/\/configuration-basics(?:#.*)?$/);
  await expect(page.locator('.VPSidebar').getByRole('link', { name: 'Profiles and categories', exact: true })).toHaveCount(1);
  await expect(page.locator('.VPSidebar').getByRole('link', { name: 'CLI reference', exact: true })).toHaveCount(0);
  if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText('Guides');
  await assertNoPageOverflow(page);
});

test('sidebar groups start expanded and collapse with their right-hand arrow', async ({ page, isMobile }) => {
  await page.goto('/getting-started');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const group = page.locator('.VPSidebar .group').filter({ has: page.getByRole('heading', { name: 'Introduction', exact: true }) });
  const link = group.getByRole('link', { name: 'Getting started', exact: true });
  await expect(link).toBeVisible();
  const control = group.getByRole('button', { name: 'toggle section', exact: true });
  await expect(control).toBeVisible();
  await control.click();
  await expect(link).not.toBeVisible();
  await control.click();
  await expect(link).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  test(`navigation scrolls to the last guide in ${theme} mode`, async ({ page, isMobile }) => {
    await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
    await page.setViewportSize({ width: isMobile ? 375 : 1440, height: 600 });
    await page.goto('/getting-started');
    if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
    const sidebar = page.locator('.VPSidebar');
    await expect(sidebar).toBeVisible();
    const dimensions = await sidebar.evaluate((element) => ({
      contentHeight: element.scrollHeight,
      visibleHeight: element.clientHeight,
    }));
    expect(dimensions.contentHeight).toBeGreaterThan(dimensions.visibleHeight);
    await sidebar.evaluate((element) => { element.scrollTop = element.scrollHeight; });
    expect(await sidebar.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    const lastGuide = sidebar.getByRole('link', { name: 'Troubleshooting', exact: true });
    await expect(lastGuide).toBeInViewport();
    await lastGuide.click();
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Troubleshooting');
    await assertNoPageOverflow(page);
  });
}

test('task catalog labels open dedicated pages without horizontal overflow', async ({ page, isMobile }) => {
  if (isMobile) await page.setViewportSize({ width: 375, height: 667 });
  await page.goto('/task-reference');
  const catalog = page.locator('main table').filter({ hasText: 'agent-packages' });
  const detail = catalog.getByRole('link', { name: 'Agent packages', exact: true });
  await expect(detail).toHaveAttribute('href', /^(?:\.\/|\/)agent-packages$/);
  if (isMobile) {
    const dimensions = await catalog.evaluate((element) => ({
      content: element.scrollWidth,
      visible: element.clientWidth,
    }));
    expect(dimensions.visible).toBeLessThanOrEqual(375);
  }
  await detail.click();
  await expect(page).toHaveURL(/\/agent-packages$/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Agent packages');
  await expect(page.locator('main #configuration')).toBeAttached();
});

const narrowWidthShardCount = 4;
const sortedDocumentRoutes = [...documentRoutes].sort();
for (let shard = 0; shard < narrowWidthShardCount; shard += 1) {
  const routes = sortedDocumentRoutes.filter((_, index) => index % narrowWidthShardCount === shard);
  test(`reference tables remain readable at narrow widths with system and fallback fonts (${shard + 1}/${narrowWidthShardCount})`, async ({ page, isMobile }) => {
    test.skip(!isMobile, 'Narrow-width layout pass runs once under the mobile project');
    test.setTimeout(180_000);
    await page.setViewportSize({ width: 375, height: 667 });
    if (shard === 0) await guideRoutes(page);
    const checkTables = async (route: string, label: string) => {
      await assertNoPageOverflow(page);
      const tables = await page.locator('main table').evaluateAll((elements) =>
        elements.map((element) => ({
          content: element.scrollWidth,
          visible: element.clientWidth,
          cells: [...element.querySelectorAll('th, td')].map((cell) => ({
            width: cell.getBoundingClientRect().width,
            fontSize: parseFloat(getComputedStyle(cell).fontSize),
          })),
        })),
      );
      for (const dimensions of tables) {
        expect(dimensions.visible, `${route} (${label})`).toBeLessThanOrEqual(375);
        expect(dimensions.content, `${route} (${label})`).toBeLessThanOrEqual(dimensions.visible + 1);
        for (const cell of dimensions.cells) {
          expect(cell.width, `${route} (${label})`).toBeGreaterThan(0);
          expect(cell.fontSize, `${route} (${label})`).toBeGreaterThanOrEqual(14);
        }
      }
    };
    for (const route of routes) {
      await page.goto(route);
      await checkTables(route, 'system');
      await page.addStyleTag({ content: ':root { --vp-font-family-base: Arial; --vp-font-family-mono: "Courier New"; }' });
      await checkTables(route, 'fallback');
    }
  });
}

test('native theme switch retains light and dark choices', async ({ page, isMobile }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  const heroWordmark = page.locator('.VPHome .VPHero .heading .name.clip');
  await expect(page.locator('html')).toHaveClass(/dark/);
  await expect(page.locator('.VPNavBarTitle img.logo.dark')).toBeVisible();
  await expect(page.locator('.VPNavBarTitle img.logo.light')).toBeHidden();
  await expect(heroWordmark).toHaveCSS('background-color', 'rgb(226, 232, 255)');
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  const toggle = page.getByRole('switch').filter({ visible: true });
  await toggle.click();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await expect(page.locator('.VPNavBarTitle img.logo.light')).toBeVisible();
  await expect(page.locator('.VPNavBarTitle img.logo.dark')).toBeHidden();
  await expect(heroWordmark).toHaveCSS('background-color', 'rgb(26, 27, 38)');
  await page.reload();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await expect(page.locator('.VPNavBarTitle img.logo.light')).toBeVisible();
  await expect(heroWordmark).toHaveCSS('background-color', 'rgb(26, 27, 38)');
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  await page.getByRole('switch').filter({ visible: true }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await expect(heroWordmark).toHaveCSS('background-color', 'rgb(226, 232, 255)');
  await page.reload();
  await expect(page.locator('html')).toHaveClass(/dark/);
});

test('client navigation and theme changes rerender diagrams', async ({ page, isMobile }) => {
  test.setTimeout(180_000);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/getting-started');
  await page.locator('.vp-doc a[href$="/profiles-and-categories"]').first().click();
  const diagram = page.locator('.mermaid svg');
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  const previous = await diagram.getAttribute('id');
  await toggleTheme(page, isMobile);
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  await expect(diagram).not.toHaveAttribute('id', previous!);
  expect(errors).toEqual([]);
});

for (const { name } of documents.filter(({ source }) => /^```mermaid\s*$/m.test(source))) {
  const guide = name.replace(/\.md$/, '');
  test(`${guide} has readable, accessible diagrams in light and dark modes`, async ({ page, isMobile }) => {
    test.setTimeout(180_000);
    if (isMobile) await page.setViewportSize({ width: 375, height: 667 });
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(message.text());
    });
    await page.addInitScript(() => localStorage.setItem('vitepress-theme-appearance', 'light'));
    await page.goto(`/${guide}`);
    await expect(page.locator('html')).not.toHaveClass(/dark/);
    const lightId = await assertDiagramReadable(page);

    await toggleTheme(page, isMobile);
    await expect(page.locator('html')).toHaveClass(/dark/);
    await expect(page.locator('.mermaid svg')).not.toHaveAttribute('id', lightId!);
    await assertDiagramReadable(page);
    expect(errors).toEqual([]);
  });
}
