import { readdirSync, readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';

const contentDirectory = new URL('../../content/', import.meta.url);
const documents = readdirSync(contentDirectory)
  .filter((name) => name.endsWith('.md') && !['index.md', '404.md'].includes(name))
  .map((name) => ({ name, source: readFileSync(new URL(name, contentDirectory), 'utf8') }));
const taskDocuments = documents.filter(({ name, source }) =>
  name !== 'task-reference.md' && /^\| Selector \|/m.test(source));
const referenceTitles = ['CLI reference', 'Task reference', ...taskDocuments
  .map(({ source }) => source.match(/^title: (.+)$/m)![1])
  .sort((left, right) => left.localeCompare(right, 'en'))];
const pageCount = documents.length;

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
  expect(routes.size).toBe(pageCount);
  return [...routes];
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
      await page.goto('/getting-started.html', { waitUntil: 'commit' });
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
      await page.getByRole('button', { name: 'Search', exact: true }).click();
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

test('the introduction and grouped navigation fit the default reading layout', async ({ page, isMobile }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Dotfiles by Stuart Neivandt');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', 'https://dotfiles.stuartneivandt.com/');
  await expect(page.locator('.vp-doc p').first()).not.toBeEmpty();
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
  await expect(groups.locator('h2')).toHaveText(['Introduction', 'Using dotfiles', 'Platforms', 'Customization']);
  for (const [label, titles] of [
    ['Introduction', ['Getting started']],
    ['Using dotfiles', ['Commands and usage', 'Profiles and categories', 'Configuration basics', 'Security and trust', 'Package management', 'Troubleshooting']],
    ['Platforms', ['Arch desktop', 'Windows and WSL']],
    ['Customization', ['Application settings', 'Agent configuration', 'Private overlays']],
  ] as const) {
    await expect(groups.filter({ has: page.getByRole('heading', { name: label, exact: true }) }).getByRole('link')).toHaveText([...titles]);
  }
  await sidebar.getByRole('link', { name: 'Getting started', exact: true }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Getting started');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Commands and usage');
  await page.locator('.VPNavBarTitle a').click();
  await expect(page).toHaveURL('/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('About this project');
  await assertNoPageOverflow(page);
});

test('header sections replace the sidebar, including direct links and search destinations', async ({ page, isMobile }) => {
  await page.goto('/getting-started.html');
  for (const [label, entry, titles] of [
    ['Reference', '/cli-reference.html', referenceTitles],
    ['Development', '/contributing.html', ['Contributing', 'Writing documentation', 'Architecture', 'Adding a task', 'Debugging', 'Arch package recipes', 'Testing', 'Writing tests', 'Mutation testing', 'CI and releases']],
    ['Guides', '/getting-started.html', ['Getting started', 'Commands and usage', 'Profiles and categories', 'Configuration basics', 'Security and trust', 'Package management', 'Troubleshooting', 'Arch desktop', 'Windows and WSL', 'Application settings', 'Agent configuration', 'Private overlays']],
  ] as const) {
    if (isMobile) await page.getByRole('button', { name: 'mobile navigation', exact: true }).click();
    await page.locator(isMobile ? '.VPNavScreen' : '.VPNavBarMenu').getByRole('link', { name: label, exact: true }).click();
    await expect(page).toHaveURL(entry);
    await expect(page.locator('.VPSidebar a')).toHaveText([...titles]);
    if (label === 'Development') {
      await expect(page.locator('.VPSidebar .group h2'))
        .toHaveText(['Workflows', 'Implementation', 'Verification']);
    }
    if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText(label);
  }
  await page.goto('/home-symlinks.html#configuration');
  await expect(page.locator('.VPSidebar a')).toHaveText(referenceTitles);
  await expect(page.locator('.VPDocFooter .next')).toContainText('Login shell');
  await expect(page.locator('.VPSidebar').getByRole('button', { name: 'toggle section', exact: true })).toHaveCount(1);
  await page.goto('/private-overlays.html');
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
  await page.goto('/package-management.html');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Troubleshooting');
});

test('task details collapse separately from the task reference overview', async ({ page, isMobile }) => {
  await page.goto('/home-symlinks.html');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  const sidebar = page.locator('.VPSidebar');
  const overviewGroup = sidebar.locator('.group')
    .filter({ has: page.getByRole('link', { name: 'Task reference', exact: true }) });
  const tasks = sidebar.locator('.group')
    .filter({ has: page.getByRole('heading', { name: 'Task details', exact: true }) });
  await expect(overviewGroup).toHaveCount(1);
  await expect(overviewGroup.getByRole('link')).toHaveText(referenceTitles.slice(0, 2));
  await expect(tasks).toHaveCount(1);
  await expect(tasks.getByRole('link')).toHaveText(referenceTitles.slice(2));
  await expect(sidebar.getByRole('link', { name: 'Task details', exact: true })).toHaveCount(0);
  const toggle = tasks.getByRole('button', { name: 'toggle section', exact: true });
  await toggle.click();
  await expect(page).toHaveURL('/home-symlinks.html');
  await expect(tasks.getByRole('link', { name: 'Home symlinks', exact: true })).not.toBeVisible();
  await expect(sidebar.getByRole('link', { name: 'CLI reference', exact: true })).toBeVisible();
  await expect(sidebar.getByRole('link', { name: 'Configuration basics', exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole('link', { name: 'Task reference', exact: true })).toBeVisible();
  await sidebar.getByRole('link', { name: 'Task reference', exact: true }).click();
  await expect(page).toHaveURL('/task-reference.html');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Task reference');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Agent packages');
  if (isMobile) await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await expect(tasks.getByRole('link', { name: 'Home symlinks', exact: true })).not.toBeVisible();
  await toggle.click();
  await expect(tasks.getByRole('link', { name: 'Home symlinks', exact: true })).toBeVisible();
  const last = tasks.getByRole('link', { name: 'Windows Developer Mode', exact: true });
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeInViewport();
  await last.click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Windows Developer Mode');
  await expect(page.locator('.VPDocFooter .next')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

test('configuration basics follows profiles in Guides, including search destinations', async ({ page, isMobile }) => {
  await page.goto('/profiles-and-categories.html');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Configuration basics');
  await page.locator('.VPDocFooter .next').click();
  await expect(page).toHaveURL('/configuration-basics.html');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Configuration basics');
  await expect(page.locator('.VPDocFooter .prev')).toContainText('Profiles and categories');
  await expect(page.locator('.VPDocFooter .next')).toContainText('Security and trust');
  if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText('Guides');

  await page.goto('/cli-reference.html');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.locator('#localsearch-input').fill('Configuration basics');
  const result = page.locator('.VPLocalSearchBox a[href*="configuration-basics.html"]').first();
  await expect(result).toBeVisible({ timeout: 30_000 });
  await result.click();
  await expect(page).toHaveURL(/configuration-basics\.html(?:#.*)?$/);
  await expect(page.locator('.VPSidebar').getByRole('link', { name: 'Profiles and categories', exact: true })).toHaveCount(1);
  await expect(page.locator('.VPSidebar').getByRole('link', { name: 'CLI reference', exact: true })).toHaveCount(0);
  if (!isMobile) await expect(page.locator('.VPNavBarMenu a.active')).toHaveText('Guides');
  await assertNoPageOverflow(page);
});

test('the first reading path leads from setup through usage to configuration', async ({ page }) => {
  for (const [route, next] of [
    ['getting-started', 'Commands and usage'],
    ['commands-and-usage', 'Profiles and categories'],
    ['profiles-and-categories', 'Configuration basics'],
  ]) {
    await page.goto(`/${route}.html`);
    await expect(page.locator('.VPDocFooter .next')).toContainText(next);
  }
  await page.goto('/getting-started.html');
  await expect(page.locator('main pre').filter({ hasText: 'PLANNED Home symlinks' }))
    .toContainText('SKIPPED VS Code extensions');
});

test('task lookup starts with reader questions and task details use title order', async ({ page }) => {
  expect(referenceTitles.slice(2)).toEqual(referenceTitles.slice(2).sort((a, b) => a.localeCompare(b, 'en')));
  await page.goto('/task-reference.html');
  const lookup = page.locator('main table').first();
  await expect(lookup).toContainText('What do you want to do?');
  await lookup.getByRole('link', { name: 'User registry settings', exact: true }).click();
  await expect(page).toHaveURL('/user-registry-settings.html');
});

test('testing lookup exposes prerequisites and keeps general setup outside the TLS exception', async ({ page, isMobile }) => {
  await page.goto('/testing.html');
  for (const anchor of [
    'baseline-and-prerequisites', 'local-npm-tls-failures', 'analyzer-prerequisites',
    'sensitive-pattern-scanning', 'full-commit-hook-suite', 'ci-selection-fixtures',
  ]) {
    await expect(page.locator(`main h3#${anchor}`)).toHaveCount(1);
    if (!isMobile) await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
  }
  const general = await page.locator('main #baseline-and-prerequisites').evaluate((heading) => {
    const paragraphs: string[] = [];
    for (let node = heading.nextElementSibling; node && !/^H[23]$/.test(node.tagName); node = node.nextElementSibling) {
      paragraphs.push(node.textContent ?? '');
    }
    return paragraphs.join('\n');
  });
  expect(general).toContain('Refresh origin/main before planning.');
  expect(general).toContain('Provide the tools required by the printed plan');
  await page.goto('/psscriptanalyzer.html');
  await expect(page.locator('main table')).toContainText('PowerShell 7 (pwsh)');
  await expect(page.locator('main')).toContainText('Windows PowerShell cannot substitute for it');
});

test('development guides connect implementation and isolated test authoring', async ({ page }) => {
  await page.goto('/contributing.html');
  await page.locator('main').getByRole('link', { name: 'Adding a task', exact: true }).first().click();
  await expect(page).toHaveURL('/adding-a-task.html');
  for (const anchor of ['choose-the-boundary', 'implement-inspection-and-application', 'wire-command-membership-and-dependencies']) {
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
  }
  await expect(page.locator('main h3#follow-one-declaration-through-the-task')).toHaveCount(1);
  await page.locator('main').getByRole('link', { name: 'Writing tests', exact: true }).first().click();
  await expect(page).toHaveURL('/writing-tests.html');
  await expect(page.locator('main #confine-every-writable-path')).toHaveCount(1);
  await expect(page.locator('main #exercise-a-complete-state-transition')).toHaveCount(1);
  await expect(page.locator('main #a-complete-isolated-task-regression')).toHaveCount(1);
  await expect(page.locator('main #integration-test-suites')).toHaveCount(1);
  await expect(page.locator('main #dry-run-testing')).toHaveCount(1);
  await expect(page.locator('.VPDocFooter .next')).toContainText('Mutation testing');
  await page.goto('/testing.html');
  for (const anchor of ['choosing-coverage', 'fast-local-sequence', 'before-pushing', 'focused-mutation-testing']) {
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
  }
  await expect(page.locator('main #integration-test-suites, main #dry-run-testing')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

test('maintenance procedures have their own contributor guides', async ({ page }) => {
  await page.goto('/architecture.html');
  await expect(page.locator('main #agent-deployment-implementation')).toHaveCount(0);
  await page.goto('/agent-configuration.html');
  await expect(page.locator('main #deployment-maintenance')).toHaveCount(0);
  await page.goto('/package-management.html');
  await expect(page.locator('main #arch-package-recipes')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'Arch package recipes', exact: true }).click();
  await expect(page).toHaveURL('/arch-package-recipes.html');
  await expect(page.locator('main #recipe-inputs')).toBeVisible();
  await page.goto('/testing.html');
  await expect(page.locator('main #prepare-an-isolated-source-snapshot')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'Mutation testing', exact: true }).click();
  await expect(page).toHaveURL('/mutation-testing.html');
  await expect(page.locator('main #prepare-an-isolated-source-snapshot')).toBeVisible();
  await assertNoPageOverflow(page);
});

test('developer guides cover debugging and documentation without assigning skills as reading', async ({ page }) => {
  for (const [title, route, anchor] of [
    ['Debugging', 'debugging', 'reproduce-the-behavior-in-a-test'],
    ['Writing documentation', 'writing-documentation', 'task-reference-pages'],
  ]) {
    await page.goto('/contributing.html');
    await page.locator('main').getByRole('link', { name: title, exact: true }).first().click();
    await expect(page).toHaveURL(`/${route}.html`);
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
    await assertNoPageOverflow(page);
  }
  for (const route of ['contributing', 'adding-a-task', 'application-settings', 'testing', 'writing-documentation']) {
    await page.goto(`/${route}.html`);
    await expect(page.locator('main a[href*="/.agents/skills/"][href*="/SKILL.md"]')).toHaveCount(0);
  }
  await assertNoPageOverflow(page);
});

test('sidebar groups start expanded and collapse with their right-hand arrow', async ({ page, isMobile }) => {
  await page.goto('/getting-started.html');
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
    ['application-settings', 'visual-studio-code', 'Visual Studio Code'],
    ['application-settings', 'vim-and-neovim', 'Vim and Neovim'],
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
  await expect(page.locator('#using-a-fork')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'fork setup', exact: true }).click();
  await expect(page).toHaveURL('/getting-started.html#using-a-fork');
  await expect(page.locator('#using-a-fork')).toBeVisible();

  await page.goto('/cli-reference.html');
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
    ['configuration-basics', 'files', 'making-a-configuration-change'],
    ['contributing', '2-find-the-owning-files', 'development-setup'],
    ['testing', 'before-pushing', 'specialized-checks'],
    ['agent-configuration', 'deployment-targets', 'validate-preview-and-apply'],
    ['agent-configuration', 'what-a-preview-can-show', 'configuration-fragments'],
    ['package-management', 'logs-and-troubleshooting', 'custom-arch-packages'],
    ['contributing', 'contribution-workflow', 'git-hooks'],
    ['commands-and-usage', 'update-lifecycle', 'select-tasks'],
    ['commands-and-usage', 'control-what-advances', 'move-a-checkout'],
    ['testing', 'baseline-and-prerequisites', 'local-npm-tls-failures'],
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
  test.setTimeout(180_000);
  await page.goto('/');
  const routes = await guideRoutes(page);
  expect(routes).toHaveLength(pageCount);
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
  const links = page.locator('.vp-doc')
    .getByRole('link', { name: 'site maintenance README', exact: true });
  await expect(links.first()).toBeAttached();
  for (const link of await links.all()) {
    await expect(link).toHaveAttribute('href', 'https://github.com/sneivandt/dotfiles/blob/main/docs/README.md');
  }
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
  ['package-management', ['Installing a missing package can upgrade the system', 'Preparation executes recipes and replaces Paru preferences']],
  ['arch-package-recipes', ['Review recipes before preparing packages', 'Preparation replaces Paru preferences']],
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

test('task catalog labels open dedicated pages without horizontal overflow', async ({ page, isMobile }) => {
  if (isMobile) await page.setViewportSize({ width: 375, height: 667 });
  await page.goto('/task-reference.html');
  const catalog = page.locator('main table').filter({ hasText: 'agent-packages' });
  const detail = catalog.getByRole('link', { name: 'Agent packages', exact: true });
  await expect(detail).toHaveAttribute('href', /^(?:\.\/|\/)agent-packages\.html$/);
  if (isMobile) {
    const dimensions = await catalog.evaluate((element) => ({
      content: element.scrollWidth,
      visible: element.clientWidth,
    }));
    expect(dimensions.visible).toBeLessThanOrEqual(375);
  }
  await detail.click();
  await expect(page).toHaveURL(/agent-packages\.html$/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Agent packages');
  await expect(page.locator('main #configuration')).toBeAttached();
});

test('task pages combine behavior, configuration, and removal guidance', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('/task-reference.html');
  const tasks = await page.locator('main table').filter({ hasText: 'agent-packages' }).locator('tbody tr').evaluateAll(
    (rows) => rows.map((row) => ({
      selector: row.querySelector('code')!.textContent,
      href: row.querySelector('a')!.getAttribute('href')!,
    })),
  );
  expect(tasks.map(({ href }) => href.split('/').pop()).sort()).toEqual(
    taskDocuments.filter(({ name, source }) => name !== 'overlay-scripts.md' && /^\| Commands \|.*`update`/m.test(source))
      .map(({ name }) => name.replace(/\.md$/, '.html')).sort(),
  );
  for (const task of tasks) {
    await page.goto(task.href);
    const introduction = await page.locator('main table').first().evaluate((table) => {
      const paragraph = table.previousElementSibling;
      return paragraph?.tagName === 'P' ? paragraph.textContent : '';
    });
    expect(introduction?.trim(), task.href).toBeTruthy();
    await expect(page.locator('main table').first()).toContainText(task.selector!);
    for (const anchor of ['what-it-does', 'configuration', 'preview-and-apply', 'removal']) {
      await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
      await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
    }
    await assertNoPageOverflow(page);
  }
});

test('the page contents group troubleshooting symptoms', async ({ page }) => {
  await page.goto('/troubleshooting.html');
  await expect(page.locator('main h2')).toHaveText([
    'Find the symptom', 'Read the failed run', 'Recover from a partial update',
    'Startup and configuration', 'Tasks and repository updates',
    'Packages and tools', 'Platform settings', 'Removal leaves settings behind',
  ]);
  for (const anchor of ['wrapper-download-fails', 'elevation-is-unavailable', 'windows-symlink-fails']) {
    await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
  }
});

for (const font of ['system', 'fallback']) {
  test(`reference tables remain readable at narrow widths with ${font} fonts`, async ({ page }) => {
    test.setTimeout(180_000);
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
  const result = page.locator('.VPLocalSearchBox a[href*="home-symlinks.html"]').first();
  await expect(result).toBeVisible();
  await result.click();
  await expect(page).toHaveURL(/home-symlinks\.html(?:#.*)?$/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Home symlinks');
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(page.locator('.VPSidebar a')).toHaveText(referenceTitles);
});

test('topic discovery uses reader terminology and page descriptions', async ({ page }) => {
  for (const [query, route, title] of [
    ['tab completion', 'shell-completions', 'Shell completions'],
    ['Copilot JSON', 'agent-settings', 'Agent settings'],
    ['HKEY_CURRENT_USER', 'user-registry-settings', 'User registry settings'],
    ['retire desktop', 'profiles-and-categories', 'Profiles and categories'],
  ]) {
    await page.goto('/getting-started.html');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await page.locator('#localsearch-input').fill(query);
    const result = page.locator(`.VPLocalSearchBox .result[href*="/${route}.html"]`).first();
    await expect(result, query).toBeVisible();
    const topResults = await page.locator('.VPLocalSearchBox .result').evaluateAll(
      (links) => links.slice(0, 5).map((link) => link.getAttribute('href')),
    );
    expect(topResults.some((href) => href?.includes(`/${route}.html`)), query).toBe(true);
    await result.click();
    await expect(page).toHaveURL(new RegExp(`${route}\\.html(?:#.*)?$`));
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(title);
  }
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.locator('#localsearch-input').fill('Page not found');
  await expect(page.locator('.VPLocalSearchBox')).toBeVisible();
  await expect(page.locator('.VPLocalSearchBox .result[href*="/404.html"]')).toHaveCount(0);
});

test('keyboard skip navigation and search dismissal preserve visible focus', async ({ page }) => {
  await page.goto('/getting-started.html');
  await page.keyboard.press('Tab');
  const skip = page.getByRole('link', { name: 'Skip to content', exact: true });
  await expect(skip).toBeFocused();
  await expect(skip).toBeInViewport();
  const skipAppearance = await skip.evaluate((element) => ({
    clipPath: getComputedStyle(element).clipPath,
    width: element.getBoundingClientRect().width,
  }));
  expect(skipAppearance.clipPath).toBe('none');
  expect(skipAppearance.width).toBeGreaterThan(40);
  await page.keyboard.press('Enter');
  await expect(page.locator('#VPContent')).toBeFocused();

  await page.goto('/getting-started.html');
  const search = page.getByRole('button', { name: 'Search', exact: true });
  for (let step = 0; step < 20 && !await search.evaluate((element) => element === document.activeElement); step++) {
    await page.keyboard.press('Tab');
  }
  await expect(search).toBeFocused();
  const focusAppearance = await search.evaluate((element) => {
    const style = getComputedStyle(element);
    return { outline: style.outlineStyle, width: parseFloat(style.outlineWidth) };
  });
  expect(focusAppearance.outline).not.toBe('none');
  expect(focusAppearance.width).toBeGreaterThanOrEqual(1);
  await page.keyboard.press('Enter');
  await expect(page.locator('#localsearch-input')).toBeFocused();
  await page.keyboard.type('symlinks');
  await expect(page.locator('.VPLocalSearchBox .result').first()).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.VPLocalSearchBox')).toHaveCount(0);
  await expect(search).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('#localsearch-input')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(search).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  const shortcutOrigin = page.locator('.VPNavBarTitle a');
  await expect(shortcutOrigin).toBeFocused();
  await page.keyboard.press('Control+k');
  await expect(page.locator('#localsearch-input')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(shortcutOrigin).toBeFocused();
});

for (const theme of ['light', 'dark'] as const) {
  test(`body, links, and warning text meet contrast requirements in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
    for (const guide of ['getting-started', 'package-management', 'private-overlays']) {
      await page.goto(`/${guide}.html`);
      const measurements = await page.locator('main p, main p a, main li a, main .custom-block-title').evaluateAll((elements) => {
        type Color = [number, number, number, number];
        function color(value: string): Color {
          const match = value.match(/^rgba?\((.+)\)$/);
          if (!match) throw new Error(`Unsupported computed color: ${value}`);
          const channels = match[1].split(/[, /]+/).map(Number);
          if (channels.length < 3 || channels.some((channel) => !Number.isFinite(channel))) {
            throw new Error(`Invalid computed color: ${value}`);
          }
          return [channels[0], channels[1], channels[2], channels[3] ?? 1];
        }
        function composite(foreground: Color, background: Color): Color {
          const alpha = foreground[3];
          return [
            foreground[0] * alpha + background[0] * (1 - alpha),
            foreground[1] * alpha + background[1] * (1 - alpha),
            foreground[2] * alpha + background[2] * (1 - alpha),
            1,
          ];
        }
        function luminance(value: Color) {
          const linear = value.slice(0, 3).map((channel) => {
            const normalized = channel / 255;
            return normalized <= 0.04045 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
          });
          return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
        }
        return elements.filter((element) => element.getClientRects().length && element.textContent?.trim())
          .map((element) => {
            const ancestors: Element[] = [];
            for (let current: Element | null = element; current; current = current.parentElement) ancestors.push(current);
            let background: Color = [255, 255, 255, 1];
            for (const ancestor of ancestors.reverse()) {
              background = composite(color(getComputedStyle(ancestor).backgroundColor), background);
            }
            const style = getComputedStyle(element);
            const foreground = composite(color(style.color), background);
            const light = luminance(foreground);
            const dark = luminance(background);
            const large = parseFloat(style.fontSize) >= 24 ||
              (parseFloat(style.fontSize) >= 18.66 && Number(style.fontWeight) >= 700);
            return {
              text: element.textContent!.trim().slice(0, 90),
              ratio: (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05),
              minimum: large ? 3 : 4.5,
            };
          });
      });
      expect(measurements.length).toBeGreaterThan(0);
      for (const measurement of measurements) {
        expect(measurement.ratio, `${guide}: ${measurement.text}`).toBeGreaterThanOrEqual(measurement.minimum);
      }
    }
  });
}

test('removed guides and repository-only docs do not publish routes', async ({ request }) => {
  for (const path of [
    '/USAGE.html', '/LOCAL-PACKAGES.html', '/TESTING-RECIPES.html',
    '/local-package-recipes.html', '/ai-agent-configuration.html',
    '/git-configuration.html', '/documentation-site.html', '/README.html', '/README.md',
    '/update-lifecycle.html', '/visual-studio-code.html', '/vim-and-neovim.html',
    '/configuration-reference.html', '/agent-deployment-internals.html', '/agent-deployment-maintenance.html',
    '/configuration-files.html',
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
  test.setTimeout(180_000);
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

for (const guide of ['profiles-and-categories', 'arch-desktop', 'architecture', 'agent-configuration', 'package-management', 'ci-and-releases']) {
  for (const theme of ['light', 'dark'] as const) {
    test(`${guide} has readable, accessible diagrams in ${theme} mode`, async ({ page }) => {
      test.setTimeout(180_000);
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
