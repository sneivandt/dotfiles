import { expect, test, type Page } from '@playwright/test';

async function assertNoPageOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    page: document.documentElement.scrollWidth,
    viewport: window.innerWidth,
  }));
  expect(dimensions.page).toBeLessThanOrEqual(dimensions.viewport + 1);
}

async function guideRoutes(page: Page) {
  const routes = new Set<string>();
  for (const section of ['/getting-started.html', '/cli-reference.html', '/contributing.html']) {
    await page.goto(section);
    for (const href of await page.locator('.VPSidebar a[href]').evaluateAll(
      (links) => links.map((link) => link.getAttribute('href')!),
    )) routes.add(href);
  }
  expect(routes.size).toBe(18);
  return [...routes];
}

test('the introduction and grouped navigation fit the default reading layout', async ({ page, isMobile }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Dotfiles by Stuart Neivandt');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('.vp-doc p').first()).toContainText('I started this as a simple dotfiles repo back in 2014');
  expect(JSON.parse(await page.locator('head script[type="application/ld+json"]').textContent() ?? '')).toMatchObject({
    '@type': 'WebSite', creator: { name: 'Stuart Neivandt' },
  });
  await expect(page.locator('link[rel~="icon"]')).toHaveAttribute('href', '/favicon.svg');
  const setup = page.locator('.vp-doc').getByRole('link', { name: 'Getting started', exact: true });
  await expect(setup).toBeInViewport();
  await expect(page.locator('.VPDocFooter .prev-next')).toHaveCount(0);
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const sidebar = page.locator('.VPSidebar');
  const groups = sidebar.locator('.group');
  await expect(groups.locator('h2')).toHaveText(['Introduction', 'Workflows', 'Platforms', 'Customization']);
  for (const [label, titles] of [
    ['Introduction', ['Getting started', 'Profiles and categories', 'Security and trust']],
    ['Workflows', ['Commands and usage', 'Package management', 'Troubleshooting']],
    ['Platforms', ['Arch desktop', 'Windows and WSL']],
    ['Customization', ['Agent configuration', 'Application settings', 'Private overlays']],
  ] as const) {
    await expect(groups.filter({ has: page.getByRole('heading', { name: label, exact: true }) }).getByRole('link')).toHaveText([...titles]);
  }
  await sidebar.getByRole('link', { name: 'Getting started', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Getting started');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Profiles and categories');
  await page.locator('.VPNavBarTitle a').click();
  await expect(page).toHaveURL('/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await assertNoPageOverflow(page);
});

test('header sections replace the sidebar, including direct links and search destinations', async ({ page, isMobile }) => {
  await page.goto('/getting-started.html');
  for (const [label, entry, titles] of [
    ['Reference', '/cli-reference.html', ['CLI reference', 'Task reference', 'Configuration reference']],
    ['Development', '/contributing.html', ['Contributing', 'Testing', 'Architecture', 'CI and releases']],
    ['Guides', '/getting-started.html', ['Getting started', 'Profiles and categories', 'Security and trust', 'Commands and usage', 'Package management', 'Troubleshooting', 'Arch desktop', 'Windows and WSL', 'Agent configuration', 'Application settings', 'Private overlays']],
  ] as const) {
    if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
    await page.locator(isMobile ? '.VPNavScreen' : '.VPNavBarMenu').getByRole('link', { name: label, exact: true }).click();
    await expect(page).toHaveURL(entry);
    await expect(page.locator('.VPSidebar a')).toHaveText([...titles]);
    if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText(label);
  }
  await page.goto('/configuration-reference.html#symlinks');
  await expect(page.locator('.VPSidebar a')).toHaveText(['CLI reference', 'Task reference', 'Configuration reference']);
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
  await page.goto('/private-overlays.html');
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
  await page.goto('/package-management.html');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Troubleshooting');
});

test('sidebar groups start expanded and collapse with their right-hand arrow', async ({ page, isMobile }) => {
  await page.goto('/getting-started.html');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const group = page.locator('.VPSidebar .group').filter({ has: page.getByRole('heading', { name: 'Introduction', exact: true }) });
  const link = group.getByRole('link', { name: 'Profiles and categories', exact: true });
  await expect(link).toBeVisible();
  const control = group.getByRole('button', { name: 'toggle section', exact: true });
  await expect(control).toBeVisible();
  await control.click();
  await expect(link).not.toBeVisible();
  await control.click();
  await expect(link).toBeVisible();
});

test('the terminal illustration dims every part of each live status line', async ({ page }) => {
  await page.goto('/assets/terminal-screenshot.svg');
  const rows = page.locator('g[class^="live-"]');
  await expect(rows).toHaveCount(8);
  for (const row of await rows.all()) {
    const fills = await row.locator('text, tspan').evaluateAll((elements) =>
      elements.map((element) => getComputedStyle(element).fill));
    expect(fills.length).toBeGreaterThan(0);
    expect(new Set(fills)).toEqual(new Set(['rgb(137, 146, 181)']));
  }
});

test('consolidated topics link to their sections after everyday guidance', async ({ page }) => {
  for (const [guide, anchor, title] of [
    ['agent-configuration', 'deployment-maintenance', 'Deployment maintenance'],
    ['package-management', 'arch-package-recipes', 'Arch package recipes'],
    ['application-settings', 'visual-studio-code', 'Visual Studio Code'],
    ['application-settings', 'vim-and-neovim', 'Vim and Neovim'],
    ['commands-and-usage', 'update-lifecycle', 'Update lifecycle'],
    ['contributing', 'git-hooks', 'Git hooks'],
  ]) {
    await page.goto(`/${guide}.html`);
    const link = page.locator('.vp-doc').getByRole('link', { name: title, exact: true }).first();
    await expect(link).toHaveAttribute('href', `#${anchor}`);
    await link.click();
    await expect(page).toHaveURL(`/${guide}.html#${anchor}`);
    await expect(page.locator(`#${anchor}`)).toBeVisible();
    await assertNoPageOverflow(page);
  }
});

test('guides link to the owning command and application references', async ({ page }) => {
  await page.goto('/commands-and-usage.html');
  await expect(page.locator('main #command-options')).toHaveCount(0);
  const reference = page.locator('main').getByRole('link', { name: 'CLI reference', exact: true });
  await reference.click();
  await expect(page).toHaveURL('/cli-reference.html');
  await expect(page.locator('#command-options')).toBeAttached();
  await expect(page.locator('#using-a-fork')).toBeAttached();
  await expect(page.locator('#log-locations')).toBeAttached();
  await expect(page.locator('#execution-output-for-scripts')).toBeAttached();

  await page.goto('/contributing.html#git-hooks');
  await expect(page.locator('main #merged-branch-cleanup')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'personal Git templates', exact: true }).click();
  await expect(page).toHaveURL('/application-settings.html#git-templates');
  await expect(page.locator('#git')).toBeVisible();
  await expect(page.locator('#git-templates')).toBeAttached();
  await expect(page.getByRole('cell', { name: 'git st', exact: true })).toBeAttached();
  await expect(page.getByText('The templates can automatically delete local branches.', { exact: true }))
    .toBeAttached();
  await assertNoPageOverflow(page);
});

test('lookup and prerequisite information precedes advanced details', async ({ page }) => {
  for (const [guide, earlier, later] of [
    ['task-reference', 'catalog-overview', 'scheduling-model'],
    ['configuration-reference', 'files', 'making-a-configuration-change'],
    ['contributing', '2-find-the-owning-files', 'development-setup'],
    ['testing', 'before-pushing', 'specialized-checks'],
    ['agent-configuration', 'deployment-targets', 'validate-preview-and-apply'],
    ['agent-configuration', 'what-a-preview-can-show', 'configuration-fragments'],
    ['agent-configuration', 'troubleshoot-deployment', 'deployment-maintenance'],
    ['package-management', 'install-and-upgrade', 'arch-package-recipes'],
    ['package-management', 'logs-and-troubleshooting', 'custom-arch-packages'],
    ['contributing', 'contribution-workflow', 'git-hooks'],
    ['commands-and-usage', 'select-tasks', 'update-lifecycle'],
  ]) {
    await page.goto(`/${guide}.html`);
    const order = await page.evaluate(({ earlier, later }) => {
      const first = document.getElementById(earlier);
      const second = document.getElementById(later);
      if (!first || !second) throw new Error(`Missing headings: ${earlier}, ${later}`);
      return Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);
    }, { earlier, later });
    expect(order).toBe(true);
  }
});

test('guide links use ordinary labels without boxed code', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/');
  const routes = await guideRoutes(page);
  expect(routes).toHaveLength(18);
  for (const route of ['/', ...routes]) {
    await page.goto(route);
    await expect(page.locator('.vp-doc a code')).toHaveCount(0);
  }
  await page.goto('/cli-reference.html');
  await expect(page.locator('main :not(pre) > code').first()).toBeVisible();
  await expect(page.locator('main pre').first()).toBeVisible();
});

test('site maintenance links point to the repository README, not a published guide', async ({ page }) => {
  await page.goto('/contributing.html');
  const link = page.locator('.vp-doc')
    .getByRole('link', { name: 'docs/README.md', exact: true });
  await expect(link).toHaveAttribute('href', 'https://github.com/sneivandt/dotfiles/blob/main/docs/README.md');
  await expect(page.locator('.VPSidebar a[href="/documentation-site.html"]')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

for (const theme of ['light', 'dark'] as const) {
  test(`navigation scrolls to the last guide in ${theme} mode`, async ({ page, isMobile }) => {
    await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
    await page.setViewportSize({ width: isMobile ? 375 : 1440, height: 600 });
    await page.goto('/getting-started.html');
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
    const lastGuide = sidebar.getByRole('link', { name: 'Private overlays', exact: true });
    await expect(lastGuide).toBeInViewport();
    await lastGuide.click();
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Private overlays');
    await assertNoPageOverflow(page);
  });
}

for (const [guide, titles] of [
  ['package-management', ['Installing a missing package can upgrade the system', 'Review recipes before preparing packages']],
  ['agent-configuration', ['Packages can do more than provide text', 'Disabling a managed workflow in the App is temporary']],
  ['getting-started', ['Back up existing configuration first', 'A preview is not a sandbox']],
  ['profiles-and-categories', ['Profiles are roles, not safety levels']],
  ['private-overlays', ['Review overlay scripts before previewing']],
] as const) {
  for (const theme of ['light', 'dark'] as const) {
    test(`${guide} renders accessible safety callouts in ${theme} mode`, async ({ page }) => {
      await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
      await page.goto(`/${guide}.html`);
      await expect(page.locator('html')).toHaveClass(theme === 'dark' ? /dark/ : /^(?!.*dark)/);
      await expect(page.locator('.custom-block.warning')).toHaveCount(titles.length);
      for (const title of titles) {
        const warning = page.locator('.custom-block.warning').filter({ has: page.locator('.custom-block-title', { hasText: title }) });
        await expect(warning).toBeVisible();
        await expect(warning.locator('.custom-block-title')).toHaveText(title);
        await expect(warning.locator('p').nth(1)).not.toBeEmpty();
        const appearance = await warning.evaluate((element) => {
          const style = getComputedStyle(element);
          const heading = element.querySelector('.custom-block-title');
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
    });
  }
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
    expect(dimensions.visible).toBeLessThanOrEqual(375);
  }
  await detail.click();
  await expect(page).toHaveURL(/task-reference\.html#agent-packages$/);
  await expect(page.locator('#agent-packages')).toBeVisible();
});

test('the page contents expose task details and group troubleshooting symptoms', async ({ page, isMobile }) => {
  await page.goto('/task-reference.html');
  const taskLinks = await page.locator('main table').first().locator('a').evaluateAll(
    (links) => [...new Set(links.map((link) => link.getAttribute('href')!))],
  );
  expect(taskLinks.length).toBeGreaterThan(15);
  for (const href of taskLinks) {
    await expect(page.locator(`.VPDocAsideOutline a[href="${href}"]`)).toHaveCount(1);
    await expect(page.locator(`main h3${href}`)).toHaveCount(1);
  }
  if (!isMobile) {
    await page.locator('.VPDocAsideOutline a[href="#home-symlinks"]').click();
    await expect(page).toHaveURL(/#home-symlinks$/);
    await expect(page.locator('#home-symlinks')).toBeInViewport();
  }

  await page.goto('/troubleshooting.html');
  await expect(page.locator('main h2')).toHaveText([
    'Read the failed run', 'Recover from a partial update',
    'Startup and configuration', 'Tasks and repository updates',
    'Packages and tools', 'Platform settings', 'Removal leaves settings behind',
  ]);
  for (const anchor of ['wrapper-download-fails', 'elevation-is-unavailable', 'windows-symlink-fails']) {
    await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
  }
});

for (const font of ['system', 'fallback']) {
  test(`reference tables remain readable at narrow widths with ${font} fonts`, async ({ page }) => {
    test.setTimeout(60_000);
    await page.setViewportSize({ width: 375, height: 667 });
    await page.goto('/');
    const routes = await guideRoutes(page);
    for (const route of routes) {
      await page.goto(route);
      if (font === 'fallback') {
        await page.addStyleTag({ content: ':root { --vp-font-family-base: Arial; --vp-font-family-mono: "Courier New"; }' });
      }
      await assertNoPageOverflow(page);
      for (const table of await page.locator('main table').all()) {
        const dimensions = await table.evaluate((element) => ({
          content: element.scrollWidth,
          visible: element.clientWidth,
          cells: [...element.querySelectorAll('th, td')].map((cell) => ({
            width: cell.getBoundingClientRect().width,
            fontSize: parseFloat(getComputedStyle(cell).fontSize),
          })),
        }));
        expect(dimensions.visible, route).toBeLessThanOrEqual(375);
        for (const cell of dimensions.cells) {
          expect(cell.width, route).toBeGreaterThan(0);
          expect(cell.fontSize, route).toBeGreaterThanOrEqual(14);
        }
      }
    }
  });
}

test('search opens the relevant section of the published guide', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.locator('#localsearch-input').fill('symlinks');
  const result = page.locator('.VPLocalSearchBox a[href*="configuration-reference.html#symlinks"]').first();
  await expect(result).toBeVisible();
  await result.click();
  await expect(page).toHaveURL(/configuration-reference\.html#symlinks$/);
  await expect(page.locator('#symlinks')).toBeVisible();
  await expect(page.locator('.VPSidebar a')).toHaveText(['CLI reference', 'Task reference', 'Configuration reference']);
});

test('removed guides and repository-only docs do not publish routes', async ({ request }) => {
  for (const path of [
    '/USAGE.html', '/LOCAL-PACKAGES.html', '/TESTING-RECIPES.html',
    '/local-package-recipes.html', '/ai-agent-configuration.html',
    '/git-configuration.html', '/documentation-site.html', '/README.html', '/README.md',
    '/update-lifecycle.html', '/visual-studio-code.html', '/vim-and-neovim.html',
    '/git-hooks.html', '/agent-deployment-internals.html', '/arch-package-recipes.html',
  ]) {
    expect((await request.get(path)).status()).toBe(404);
  }
});

test('native theme switch retains light and dark choices', async ({ page, isMobile }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).toHaveClass(/dark/);
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  const toggle = page.getByRole('switch').filter({ visible: true });
  await toggle.click();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await page.reload();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  await page.getByRole('switch').filter({ visible: true }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await page.reload();
  await expect(page.locator('html')).toHaveClass(/dark/);
});

test('a saved system theme follows operating-system appearance', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('vitepress-theme-appearance', 'auto'));
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveClass(/dark/);
});

test('client navigation and theme changes rerender diagrams', async ({ page, isMobile }) => {
  test.setTimeout(60_000);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/getting-started.html');
  await page.locator('.vp-doc').getByRole('link', { name: 'Profiles and categories', exact: true }).first().click();
  const diagram = page.locator('.mermaid svg');
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  const previous = await diagram.getAttribute('id');
  if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
  await page.getByRole('switch').filter({ visible: true }).click();
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  await expect(diagram).not.toHaveAttribute('id', previous!);
  expect(errors).toEqual([]);
});

for (const guide of ['profiles-and-categories', 'commands-and-usage', 'arch-desktop', 'architecture', 'agent-configuration', 'package-management', 'ci-and-releases']) {
  for (const theme of ['light', 'dark'] as const) {
    test(`${guide} has readable, accessible diagrams in ${theme} mode`, async ({ page }) => {
      test.setTimeout(60_000);
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      page.on('console', (message) => {
        if (message.type() === 'error') errors.push(message.text());
      });
      await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
      await page.goto(`/${guide}.html`);
      await expect(page.locator('html')).toHaveClass(theme === 'dark' ? /dark/ : /^(?!.*dark)/);
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
      await assertNoPageOverflow(page);
      expect(errors).toEqual([]);
    });
  }
}
