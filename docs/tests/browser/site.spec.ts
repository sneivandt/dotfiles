import { expect, test, type Page } from '@playwright/test';

async function assertNoPageOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    page: document.documentElement.scrollWidth,
    viewport: window.innerWidth,
  }));
  expect(dimensions.page).toBeLessThanOrEqual(dimensions.viewport + 1);
}

test('the homepage introduces the repository and links to setup, with grouped navigation and a favicon', async ({ page, isMobile }) => {
  await page.goto('/');
  await expect(page).toHaveURL('/');
  await expect(page).toHaveTitle('Dotfiles by Stuart Neivandt');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/');
  const introduction = page.locator('.sl-markdown-content p').first();
  await expect(introduction).toContainText(/^Hi, I/);
  await expect(introduction).toContainText('I started this as a simple dotfiles repo back in 2014');
  await expect(introduction).toContainText('coding agents can help build and maintain software');
  await expect(introduction.getByRole('link', { name: 'Stuart Neivandt', exact: true }))
    .toHaveAttribute('href', 'https://stuartneivandt.com/');
  const structuredData = page.locator('head script[type="application/ld+json"]');
  await expect(structuredData).toHaveCount(1);
  expect(JSON.parse(await structuredData.textContent() ?? '')).toMatchObject({
    '@context': 'https://schema.org',
    '@type': 'WebSite',
    name: 'Dotfiles',
    url: 'https://dotfiles.stuartneivandt.com/',
    creator: { '@type': 'Person', name: 'Stuart Neivandt', url: 'https://stuartneivandt.com/' },
  });
  await expect(page.locator('.hero')).toHaveCount(0);
  await expect(page.getByText('On this page', { exact: true })).toHaveCount(0);
  const setupLink = page.locator('.sl-markdown-content').getByRole('link', { name: 'Getting started', exact: true });
  expect(await setupLink.evaluate((element) => element.getBoundingClientRect().bottom))
    .toBeLessThanOrEqual(await page.evaluate(() => window.innerHeight));
  if (isMobile) {
    await page.setViewportSize({ width: 375, height: 667 });
    expect(await setupLink.evaluate((element) => element.getBoundingClientRect().bottom))
      .toBeLessThanOrEqual(await page.evaluate(() => window.innerHeight));
  }
  for (const [label, href] of [
    ['Getting started', 'getting-started.html'],
    ['Commands and usage', 'commands-and-usage.html'],
    ['Configuration reference', 'configuration-reference.html'],
  ]) {
    await expect(page.locator('.sl-markdown-content').getByRole('link', { name: label, exact: true }))
      .toHaveAttribute('href', href);
  }
  await expect(page.locator('link[rel~="icon"]')).toHaveAttribute('href', '/favicon.svg');
  await expect(page.getByText('Built with Starlight')).toHaveCount(0);
  await expect(page.getByText('This site uses Just the Docs')).toHaveCount(0);
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const sidebar = page.locator('#starlight__sidebar');
  const homeLink = page.getByRole('banner').getByRole('link', { name: 'Dotfiles', exact: true });
  await expect(homeLink).toHaveAttribute('href', '/index.html');
  const brandSpacing = await homeLink.evaluate((element) => {
    const image = element.querySelector('img');
    const text = element.querySelector('span');
    if (!image || !text) throw new Error('The home link must contain its icon and title');
    const icon = image.getBoundingClientRect();
    const title = text.getBoundingClientRect();
    return {
      gap: title.left - icon.right,
      verticalOffset: Math.abs((icon.top + icon.bottom) / 2 - (title.top + title.bottom) / 2),
    };
  });
  expect(brandSpacing.gap).toBeGreaterThanOrEqual(8);
  expect(brandSpacing.gap).toBeLessThanOrEqual(12);
  expect(brandSpacing.verticalOffset).toBeLessThanOrEqual(1);
  await expect(sidebar.getByRole('link', { name: 'Dotfiles', exact: true })).toHaveCount(0);
  const startHeading = sidebar.getByText('Start here', { exact: true });
  await expect(startHeading).toBeVisible();
  const groups = sidebar.locator('details');
  await expect(groups.locator(':scope > summary')).toHaveText([
    'Start here', 'Use dotfiles', 'Customize',
    'Platforms', 'Packages', 'AI agents', 'Editors', 'Development',
  ]);
  await expect(groups.locator('details')).toHaveCount(0);
  for (const section of await groups.all()) {
    await expect(section).toHaveAttribute('open', '');
  }
  for (const [label, titles] of [
    ['Platforms', ['Arch desktop', 'Windows and WSL']],
    ['Packages', ['Package management', 'Arch package recipes']],
    ['AI agents', ['Agent configuration', 'Agent deployment internals']],
    ['Editors', ['Visual Studio Code', 'Vim and Neovim']],
  ] as const) {
    const group = groups.filter({ has: page.getByText(label, { exact: true }) });
    await expect(group.getByRole('link')).toHaveText([...titles]);
  }
  const start = page.locator('#starlight__sidebar details')
    .filter({ has: page.getByText('Start here', { exact: true }) });
  await expect(start.getByRole('link')).toHaveText(['Getting started', 'Profiles and categories']);
  const usage = page.locator('#starlight__sidebar details')
    .filter({ has: page.getByText('Use dotfiles', { exact: true }) });
  await expect(usage.getByRole('link', { name: 'Update lifecycle', exact: true }))
    .toHaveAttribute('href', '/update-lifecycle.html');
  const customize = page.locator('#starlight__sidebar details')
    .filter({ has: page.getByText('Customize', { exact: true }) });
  await expect(customize.getByRole('link', { name: 'Private overlays', exact: true }))
    .toHaveAttribute('href', '/private-overlays.html');
  const development = page.locator('#starlight__sidebar details')
    .filter({ has: page.getByText('Development', { exact: true }) });
  await expect(development.getByRole('link')).toHaveText([
    'Contributing', 'Architecture', 'Testing', 'Git hooks', 'CI and releases',
  ]);
  for (const [label, href] of [
    ['Agent deployment internals', '/agent-deployment-internals.html'],
    ['Arch package recipes', '/arch-package-recipes.html'],
  ]) {
    await expect(sidebar.getByRole('link', { name: label, exact: true }))
      .toHaveAttribute('href', href);
  }
  await page.locator('#starlight__sidebar a[href="/getting-started.html"]').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Getting started');
  await homeLink.click();
  await expect(page).toHaveURL('/index.html');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await expect(page.getByText('On this page', { exact: true })).toHaveCount(0);
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.locator('#starlight__sidebar a[href="/commands-and-usage.html"]').click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Commands and usage');
  await expect(page.getByText('On this page', { exact: true }).filter({ visible: true })).toBeVisible();
  await assertNoPageOverflow(page);
});

test('everyday guides link maintainers to their advanced guides', async ({ page }) => {
  for (const [guide, title, target] of [
    ['agent-configuration', 'Agent deployment internals', 'agent-deployment-internals'],
    ['package-management', 'Arch package recipes', 'arch-package-recipes'],
  ]) {
    await page.goto(`/${guide}.html`);
    const link = page.locator('.sl-markdown-content p')
      .filter({ hasText: 'For maintainers:' })
      .getByRole('link', { name: title, exact: true });
    await expect(link).toHaveAttribute('href', `${target}.html`);
    await link.click();
    await expect(page).toHaveURL(`/${target}.html`);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(title);
  }
});

test('advanced guides remain visible within top-level topic navigation', async ({ page, isMobile }) => {
  for (const [guide, title] of [
    ['agent-deployment-internals', 'Agent deployment internals'],
    ['arch-package-recipes', 'Arch package recipes'],
  ]) {
    await page.goto(`/${guide}.html`);
    if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
    const current = page.locator('#starlight__sidebar [aria-current="page"]');
    await expect(current).toHaveText(title);
    await expect(current).toBeVisible();
  }
});

for (const theme of ['light', 'dark'] as const) {
  test(`safety warnings render as distinct accessible callouts in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((choice) => localStorage.setItem('starlight-theme', choice), theme);
    for (const [guide, titles] of [
      ['arch-package-recipes', ['Review recipes before preparing packages']],
      ['package-management', ['Installing a missing package can upgrade the system']],
      ['agent-configuration', ['Packages can do more than provide text', 'Disabling a managed workflow in the App is temporary']],
      ['getting-started', ['Back up existing configuration first', 'A preview is not a sandbox']],
      ['profiles-and-categories', ['Profiles are roles, not safety levels']],
      ['private-overlays', ['Review overlay scripts before previewing']],
    ] as const) {
      await page.goto(`/${guide}.html`);
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await expect(page.locator('aside.starlight-aside--caution')).toHaveCount(titles.length);
      for (const title of titles) {
        const warning = page.getByRole('complementary', { name: title, exact: true });
        await expect(warning).toBeVisible();
        await expect(warning.locator('.starlight-aside__title')).toHaveText(title);
        await expect(warning.locator('.starlight-aside__icon')).toBeVisible();
        await expect(warning.locator('.starlight-aside__content')).not.toBeEmpty();
        const appearance = await warning.evaluate((element) => {
          const style = getComputedStyle(element);
          const heading = element.querySelector('.starlight-aside__title');
          if (!heading) throw new Error('The warning must have a title');
          return {
            background: style.backgroundColor,
            pageBackground: getComputedStyle(document.body).backgroundColor,
            padding: parseFloat(style.paddingInlineStart),
            titleWeight: getComputedStyle(heading).fontWeight,
          };
        });
        expect(appearance.background).not.toBe('rgba(0, 0, 0, 0)');
        expect(appearance.background).not.toBe(appearance.pageBackground);
        expect(appearance.padding).toBeGreaterThanOrEqual(16);
        expect(Number(appearance.titleWeight)).toBeGreaterThanOrEqual(500);
      }
      await assertNoPageOverflow(page);
    }
  });
}

test('task catalog labels jump to details without horizontal overflow', async ({ page, isMobile }) => {
  if (isMobile) await page.setViewportSize({ width: 375, height: 667 });
  await page.goto('/task-reference.html');
  const catalog = page.locator('main table').filter({ hasText: 'agent-packages' });
  const detail = catalog.getByRole('link', { name: 'Agent packages', exact: true });
  await expect(detail).toHaveAttribute('href', '#agent-packages');
  if (isMobile) {
    const dimensions = await catalog.evaluate((element) => ({
      content: element.scrollWidth,
      visible: element.clientWidth,
    }));
    expect(dimensions.content).toBeLessThanOrEqual(dimensions.visible + 1);
  }
  await detail.click();
  await expect(page).toHaveURL(/task-reference\.html#agent-packages$/);
  await expect(page.locator('#agent-packages')).toBeVisible();
});

test('search opens the relevant section of the published guide', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Search docs', exact: true }).click();
  await page.locator('.pagefind-ui__search-input').fill('symlinks');
  const result = page.locator('.pagefind-ui__result-link[href*="configuration-reference.html#symlinks"]').first();
  await expect(result).toBeVisible();
  await result.click();
  await expect(page).toHaveURL(/configuration-reference\.html#symlinks$/);
  await expect(page.locator('#symlinks')).toBeVisible();
});

test('renamed pages do not create compatibility routes', async ({ request }) => {
  for (const path of ['/USAGE.html', '/LOCAL-PACKAGES.html', '/TESTING-RECIPES.html', '/local-package-recipes.html', '/ai-agent-configuration.html']) {
    expect((await request.get(path)).status()).toBe(404);
  }
});

test('light, dark, and system themes work and retain the reader’s choice', async ({ page, isMobile }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const select = page.locator('starlight-theme-select select').filter({ visible: true });
  await expect(select).toHaveValue('dark');
  await select.selectOption('light');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await select.selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await select.selectOption('auto');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await expect(select).toHaveValue('auto');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

for (const guide of ['profiles-and-categories', 'update-lifecycle', 'arch-desktop', 'architecture', 'agent-configuration', 'arch-package-recipes', 'ci-and-releases']) {
  for (const theme of ['light', 'dark'] as const) {
    test(`${guide} has readable, accessible diagrams in ${theme} mode`, async ({ page }) => {
      test.setTimeout(60_000);
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      page.on('console', (message) => {
        if (message.type() === 'error') errors.push(message.text());
      });
      await page.addInitScript((choice) => localStorage.setItem('starlight-theme', choice), theme);
      await page.goto(`/${guide}.html`);
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      const diagram = page.locator('pre.mermaid svg');
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
      await assertNoPageOverflow(page);
      expect(errors).toEqual([]);
    });
  }
}
