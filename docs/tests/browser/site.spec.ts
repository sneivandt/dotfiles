import { expect, test } from '@playwright/test';
import {
  assertNoPageOverflow,
  openSearch,
  referenceTitles,
  taskDocuments,
  updateTaskTitles,
  validationTaskTitles,
} from './helpers';

test('removed pages render the 404 heading', async ({ page }) => {
  await page.goto('/arch-package-recipes');
  await expect(page.getByRole('heading', { name: 'Page not found', exact: true })).toBeVisible();
});

test('the first reading path leads from setup through usage to configuration', async ({ page }) => {
  for (const [route, next] of [
    ['getting-started', 'Commands and usage'],
    ['commands-and-usage', 'Profiles and categories'],
    ['profiles-and-categories', 'Configuration basics'],
    ['configuration-basics', 'Package management'],
  ]) {
    await page.goto(`/${route}`);
    await expect(page.locator('.VPDocFooter .next')).toContainText(next);
  }
  await page.goto('/getting-started');
  await expect(page.locator('main pre').filter({ hasText: 'PLANNED Home symlinks' }))
    .toContainText('SKIPPED VS Code extensions');
});

test('task lookup starts with reader questions and task groups use title order', async ({ page }) => {
  expect(updateTaskTitles.length + validationTaskTitles.length).toBe(taskDocuments.length);
  expect(updateTaskTitles).toEqual([...updateTaskTitles].sort((a, b) => a.localeCompare(b, 'en')));
  expect(validationTaskTitles).toEqual([...validationTaskTitles].sort((a, b) => a.localeCompare(b, 'en')));
  await page.goto('/task-reference');
  const lookup = page.locator('main table').first();
  await expect(lookup).toContainText('What do you want to do?');
  await lookup.getByRole('link', { name: 'User registry settings', exact: true }).click();
  await expect(page).toHaveURL('/user-registry-settings');
});

test('testing lookup exposes prerequisites and keeps general setup outside the TLS exception', async ({ page }) => {
  await page.goto('/testing');
  for (const anchor of [
    'baseline-and-prerequisites', 'local-npm-tls-failures', 'analyzer-prerequisites',
    'sensitive-pattern-scanning', 'full-commit-hook-suite', 'ci-selection-fixtures',
  ]) {
    await expect(page.locator(`main h3#${anchor}`)).toHaveCount(1);
    await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
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
  await expect(page.locator('main')).toContainText('PowerShell 7 (pwsh)');
  await expect(page.locator('main')).toContainText('Windows PowerShell cannot substitute for it');
});

test('development guides connect implementation and isolated test authoring', async ({ page }) => {
  await page.goto('/contributing');
  await page.locator('main').getByRole('link', { name: 'Adding a task', exact: true }).first().click();
  await expect(page).toHaveURL('/adding-a-task');
  for (const anchor of ['choose-the-boundary', 'implement-inspection-and-application', 'wire-command-membership-and-dependencies']) {
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
  }
  await expect(page.locator('main h3#follow-one-declaration-through-the-task')).toHaveCount(1);
  await page.locator('main').getByRole('link', { name: 'Writing tests', exact: true }).first().click();
  await expect(page).toHaveURL('/writing-tests');
  await expect(page.locator('main #confine-every-writable-path')).toHaveCount(1);
  await expect(page.locator('main #exercise-a-complete-state-transition')).toHaveCount(1);
  await expect(page.locator('main #a-complete-isolated-task-regression')).toHaveCount(1);
  await expect(page.locator('main #integration-test-suites')).toHaveCount(1);
  await expect(page.locator('main #dry-run-testing')).toHaveCount(1);
  await expect(page.locator('.VPDocFooter .next')).toContainText('Mutation testing');
  await page.goto('/testing');
  for (const anchor of ['choosing-coverage', 'fast-local-sequence', 'before-pushing', 'focused-mutation-testing']) {
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
  }
  await expect(page.locator('main #integration-test-suites, main #dry-run-testing')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

test('maintenance procedures link to task references and contributor guides', async ({ page }) => {
  await page.goto('/architecture');
  await expect(page.locator('main #agent-deployment-implementation')).toHaveCount(0);
  await page.goto('/agent-configuration');
  await expect(page.locator('main #deployment-maintenance')).toHaveCount(0);
  await page.goto('/package-management');
  await page.locator('main').getByRole('link', { name: 'Custom package repository', exact: true }).click();
  await expect(page).toHaveURL('/custom-package-repository');
  await expect(page.locator('main #configuration')).toBeVisible();
  await page.goto('/testing');
  await expect(page.locator('main #prerequisites-and-baseline')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'Mutation testing', exact: true }).first().click();
  await expect(page).toHaveURL('/mutation-testing');
  await expect(page.locator('main #prerequisites-and-baseline')).toBeVisible();
  await assertNoPageOverflow(page);
});

test('developer guides cover debugging and documentation', async ({ page }) => {
  for (const [title, route, anchor] of [
    ['Debugging', 'debugging', 'reproduce-the-behavior-in-a-test'],
    ['Writing documentation', 'writing-documentation', 'task-reference-pages'],
  ]) {
    await page.goto('/contributing');
    await page.locator('main').getByRole('link', { name: title, exact: true }).first().click();
    await expect(page).toHaveURL(`/${route}`);
    await expect(page.locator(`main h2#${anchor}`)).toHaveCount(1);
    await assertNoPageOverflow(page);
  }
});

test('the terminal illustration matches the CLI progress and action styling', async ({ page }) => {
  await page.goto('/assets/terminal-screenshot.svg');
  const rows = page.locator('g[class^="live-"], g.checking, g.updating');
  expect(await rows.count()).toBeGreaterThan(0);
  for (const row of await rows.all()) {
    const fills = await row.locator('text:first-child, text:first-child tspan').evaluateAll((elements) =>
      elements.map((element) => getComputedStyle(element).fill));
    expect(new Set(fills)).toEqual(new Set(['rgb(125, 207, 255)']));
    await expect(row.locator('text').last()).toHaveCSS('fill', 'rgb(126, 133, 161)');
  }
  await expect(page.locator('g.checking text').last()).toHaveText('Checking for CLI updates');
  await expect(page.locator('g.updating text').last()).toHaveText('Updating CLI to v2026.01.01-2');
  await expect(page.locator('g.live-sync text').last()).toHaveText('18 left · Dotfiles repository');
  const rowY = async (selector: string) => page.locator(selector).first().evaluate((element) =>
    element.getBoundingClientRect().y);
  const headerY = await rowY('.startup text');
  const upgradedY = await rowY('.upgraded text');
  expect(upgradedY).toBeGreaterThan(headerY);
  expect(await rowY('.checking text')).toBe(upgradedY);
  expect(await rowY('.updating text')).toBe(upgradedY);
  expect(await rowY('.live-sync text')).toBe(await rowY('.synced text'));
  const counts = await page.locator('g[class^="live-"] text:last-child').allTextContents();
  for (const count of counts) {
    expect(count).toMatch(/^([1-9]\d| \d) left · \S/);
  }
  for (const task of ['links', 'completions', 'packages']) {
    const block = page.locator(`g.${task}`);
    if (task !== 'completions') {
      await expect(block.locator('text').first()).toContainText(' · 2 actions applied');
      await expect(block.locator('text:first-child tspan').last()).toHaveCSS('fill', 'rgb(126, 133, 161)');
    }
    const actions = block.locator('text:not(:first-child)');
    for (const action of await actions.all()) {
      const text = await action.textContent();
      expect(text).toMatch(/^\S+ \S/);
      await expect(action).toHaveCSS('fill', 'rgb(192, 202, 245)');
      await expect(action.locator('tspan').first()).toHaveCSS('fill', 'rgb(125, 207, 255)');
      if (text?.includes(' → ')) {
        await expect(action.locator('tspan').nth(1)).toHaveCSS('fill', 'rgb(126, 133, 161)');
      }
    }
  }
});

test('the terminal illustration preserves the completed transcript with reduced motion', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/assets/terminal-screenshot.svg');
  for (const row of await page.locator('.transient, g[class^="live-"]').all()) {
    await expect(row).toBeHidden();
  }
  for (const name of ['upgraded', 'startup', 'synced', 'links', 'completions', 'packages', 'summary', 'prompt']) {
    await expect(page.locator(`g.${name}`)).toBeVisible();
  }
  await expect(page.locator('.upgraded tspan').first()).toHaveCSS('font-weight', '700');
  await expect(page.locator('.upgraded tspan').nth(1)).toHaveCSS('fill', 'rgb(126, 133, 161)');
  await expect(page.locator('.upgraded tspan').last()).toHaveText('v2026.01.01-2');
  await expect(page.locator('.upgraded tspan').last()).toHaveCSS('fill', 'rgb(192, 202, 245)');
  await expect(page.locator('.synced tspan').first()).toHaveCSS('font-weight', '700');
  await expect(page.locator('.synced text')).toHaveText('Repository synced · restarting to load configuration');
  await expect(page.locator('.synced tspan').last()).toHaveCSS('fill', 'rgb(126, 133, 161)');
  await expect(page.locator('.completions text').last()).toHaveText(/write +symlinks\/config\/zsh\/completions\/_dotfiles$/);
  for (const label of await page.locator('.startup text:first-child tspan:first-child, .links text:first-child tspan:nth-child(2), .completions text:first-child tspan:nth-child(2), .packages text:first-child tspan:nth-child(2)').all()) {
    await expect(label).toHaveCSS('font-weight', '700');
  }
  const rowY = async (selector: string) => page.locator(selector).first().evaluate((element) =>
    element.getBoundingClientRect().y);
  const headerY = await rowY('.startup text');
  const noticeY = await rowY('.upgraded text');
  const syncY = await rowY('.synced text');
  const taskY = await rowY('.links text');
  expect(noticeY - headerY).toBe(48);
  expect(syncY - noticeY).toBe(24);
  expect(taskY - syncY).toBe(48);
  await expect(page.locator('.summary text')).toHaveText('Tasks · 3 changed · 14 unchanged · 8.3s');
  await expect(page.locator('.summary tspan').first()).toHaveCSS('fill', 'rgb(126, 133, 161)');
  await expect(page.locator('.summary tspan').nth(1)).toHaveCSS('fill', 'rgb(158, 206, 106)');
  const clipped = await page.locator('svg').evaluate((svg) => {
    const bounds = svg.getBoundingClientRect();
    return [...svg.querySelectorAll('text')].filter((text) => {
      const box = text.getBoundingClientRect();
      return box.width > 0 && (box.left < bounds.left || box.top < bounds.top ||
        box.right > bounds.right || box.bottom > bounds.bottom);
    }).map((text) => text.textContent);
  });
  expect(clipped).toEqual([]);
});

test('consolidated topics link to their sections after everyday guidance', async ({ page }) => {
  for (const [guide, anchor, title] of [
    ['application-settings', 'visual-studio-code', 'Visual Studio Code'],
    ['application-settings', 'vim-and-neovim', 'Vim and Neovim'],
    ['contributing', 'git-hooks', 'Git hooks'],
  ]) {
    await page.goto(`/${guide}`);
    const link = page.locator('.vp-doc').getByRole('link', { name: title, exact: true }).first();
    await expect(link).toHaveAttribute('href', `#${anchor}`);
    await link.click();
    await expect(page).toHaveURL(`/${guide}#${anchor}`);
    await expect(page.locator(`#${anchor}`)).toBeVisible();
    await assertNoPageOverflow(page);
  }
});

test('guides link to the owning command and application references', async ({ page }) => {
  await page.goto('/commands-and-usage');
  await expect(page.locator('main #command-options')).toHaveCount(0);
  const reference = page.locator('main').getByRole('link', { name: 'CLI reference', exact: true });
  await reference.click();
  await expect(page).toHaveURL('/cli-reference');
  await expect(page.locator('#command-options')).toBeAttached();
  await expect(page.locator('#using-a-fork')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'fork setup', exact: true }).click();
  await expect(page).toHaveURL('/getting-started#using-a-fork');
  await expect(page.locator('#using-a-fork')).toBeVisible();

  await page.goto('/cli-reference');
  await expect(page.locator('#log-locations')).toBeAttached();
  await expect(page.locator('#execution-output-for-scripts')).toBeAttached();

  await page.goto('/contributing#git-hooks');
  await expect(page.locator('main #merged-branch-cleanup')).toHaveCount(0);
  await page.locator('main').getByRole('link', { name: 'personal Git templates', exact: true }).click();
  await expect(page).toHaveURL('/application-settings#git-templates');
  await expect(page.locator('#git')).toBeVisible();
  await expect(page.locator('#git-templates')).toBeAttached();
  await expect(page.getByRole('cell', { name: 'git st', exact: true })).toBeAttached();
  await expect(page.getByText('The templates can automatically delete local branches.', { exact: true }))
    .toBeAttached();
  await assertNoPageOverflow(page);
});

test('inline code and fenced code blocks render visibly on reference pages', async ({ page }) => {
  await page.goto('/cli-reference');
  await expect(page.locator('main :not(pre) > code').first()).toBeVisible();
  await expect(page.locator('main pre').first()).toBeVisible();
});

test('site maintenance links point to the repository README, not a published guide', async ({ page }) => {
  await page.goto('/contributing');
  const links = page.locator('.vp-doc')
    .getByRole('link', { name: 'site maintenance README', exact: true });
  await expect(links.first()).toBeAttached();
  for (const link of await links.all()) {
    await expect(link).toHaveAttribute('href', 'https://github.com/sneivandt/dotfiles/blob/main/docs/README.md');
  }
  await expect(page.locator('.VPSidebar a[href="/documentation-site"]')).toHaveCount(0);
  await assertNoPageOverflow(page);
});

const safetyCalloutGuides = [
  ['package-management', ['Installing a missing package can upgrade the system', 'Preparation executes recipes and replaces Paru preferences']],
  ['custom-package-repository', ['Review recipes before preparing packages', 'Existing Paru configuration is replaced']],
  ['agent-configuration', ['Packages can do more than provide text', 'Disabling a managed workflow in the App is temporary']],
  ['getting-started', ['Back up existing configuration first', 'A preview is not a sandbox']],
  ['profiles-and-categories', ['Profiles are roles, not safety levels']],
  ['private-overlays', ['Review overlay scripts before previewing']],
] as const;

for (const theme of ['light', 'dark'] as const) {
  test(`guides render accessible safety callouts in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((choice) => localStorage.setItem('vitepress-theme-appearance', choice), theme);
    for (const [guide, titles] of safetyCalloutGuides) {
      await page.goto(`/${guide}`);
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
        expect(appearance.background, guide).not.toBe('rgba(0, 0, 0, 0)');
        expect(appearance.background, guide).not.toBe(appearance.pageBackground);
        expect(appearance.padding, guide).toBeGreaterThanOrEqual(16);
        expect(Number(appearance.titleWeight), guide).toBeGreaterThanOrEqual(500);
      }
      await assertNoPageOverflow(page);
    }
  });
}

test('the page contents group troubleshooting symptoms', async ({ page }) => {
  await page.goto('/troubleshooting');
  await expect(page.locator('main h2')).toHaveText([
    'Find the symptom', 'Read the failed run', 'Recover from a partial update',
    'Startup and configuration', 'Tasks and repository updates',
    'Packages and tools', 'Platform settings', 'Removal leaves settings behind',
  ]);
  for (const anchor of ['wrapper-download-fails', 'elevation-is-unavailable', 'windows-symlink-fails']) {
    await expect(page.locator(`.VPDocAsideOutline a[href="#${anchor}"]`)).toHaveCount(1);
  }
});

test('search opens the relevant section of the published guide', async ({ page }) => {
  await page.goto('/');
  await openSearch(page);
  await page.locator('#localsearch-input').fill('symlinks');
  const result = page.locator('.VPLocalSearchBox a[href*="/home-symlinks"]').first();
  await expect(result).toBeVisible();
  await result.click();
  await expect(page).toHaveURL(/\/home-symlinks(?:#.*)?$/);
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
    await page.goto('/getting-started');
    await openSearch(page);
    await page.locator('#localsearch-input').fill(query);
    const result = page.locator(`.VPLocalSearchBox .result[href*="/${route}"]`).first();
    await expect(result, query).toBeVisible();
    const topResults = await page.locator('.VPLocalSearchBox .result').evaluateAll(
      (links) => links.slice(0, 5).map((link) => link.getAttribute('href')),
    );
    expect(topResults.some((href) => href?.includes(`/${route}`)), query).toBe(true);
    await result.click();
    await expect(page).toHaveURL(new RegExp(`/${route}(?:#.*)?$`));
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(title);
  }
  await openSearch(page);
  await page.locator('#localsearch-input').fill('Page not found');
  await expect(page.locator('.VPLocalSearchBox')).toBeVisible();
  await expect(page.locator('.VPLocalSearchBox .result[href*="/404"]')).toHaveCount(0);
});

test('keyboard skip navigation and search dismissal preserve visible focus', async ({ page }) => {
  await page.goto('/getting-started');
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

  await page.goto('/getting-started');
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
    for (const route of ['/', '/getting-started', '/package-management', '/private-overlays']) {
      await page.goto(route);
      const selector = route === '/'
        ? '.VPHero .tagline, .VPHero .actions .VPButton, main p, main p a, main li a, .site-footer, .site-footer a'
        : 'main p, main p a, main li a, main .custom-block-title, .site-footer, .site-footer a';
      const measurements = await page.locator(selector).evaluateAll((elements) => {
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
        expect(measurement.ratio, `${route}: ${measurement.text}`).toBeGreaterThanOrEqual(measurement.minimum);
      }
    }
  });
}

test('removed guides and repository-only docs do not publish routes', async ({ request }) => {
  for (const path of [
    '/USAGE', '/LOCAL-PACKAGES', '/TESTING-RECIPES',
    '/local-package-recipes', '/ai-agent-configuration',
    '/git-configuration', '/documentation-site', '/README', '/README.md',
    '/update-lifecycle', '/visual-studio-code', '/vim-and-neovim',
    '/configuration-reference', '/agent-deployment-internals', '/agent-deployment-maintenance',
    '/configuration-files', '/arch-package-recipes',
    '/USAGE.html', '/documentation-site.html',
  ]) {
    expect((await request.get(path)).status()).toBe(404);
  }
});

test('a saved system theme follows operating-system appearance', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('vitepress-theme-appearance', 'auto'));
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveClass(/dark/);
});
