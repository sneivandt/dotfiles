import assert from 'node:assert/strict';
import { readdirSync } from 'node:fs';
import { basename } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createMarkdownRenderer } from 'vitepress';
import config from '../.vitepress/config.mts';

const md = await createMarkdownRenderer(fileURLToPath(new URL('../content/', import.meta.url)), config.markdown);
const render = config.themeConfig.search.options._render;

test('section sources retain unique flat routes without the removed recipe guide', () => {
  const sources = readdirSync(new URL('../content/', import.meta.url), { recursive: true })
    .filter((path) => path.endsWith('.md') && path !== '404.md')
    .map((path) => path.replaceAll('\\', '/'));
  assert.equal(new Set(sources.map((path) => basename(path))).size, sources.length);
  assert.ok(sources.every((path) => /^(guides|reference|development)\/[^/]+\.md$/.test(path)));
  assert.deepEqual(config.rewrites, Object.fromEntries(sources.map((path) => [path, basename(path)])));
  assert.ok(!sources.some((path) => path.endsWith('/arch-package-recipes.md')));
});

test('cross-section Markdown links retain flat routes, anchors, and external sources', () => {
  for (const [source, destination] of [
    ['../reference/cli-reference.md#command-options', '/cli-reference.html#command-options'],
    ['../development/contributing.md', '/contributing.html'],
    ['../guides/index.md', '/'],
    ['getting-started.md#using-a-fork', './getting-started.html#using-a-fork'],
    ['#details', '#details'],
    ['https://github.com/sneivandt/dotfiles/blob/main/docs/content/guides/index.md', 'https://github.com/sneivandt/dotfiles/blob/main/docs/content/guides/index.md'],
  ]) {
    assert.ok(md.render(`[Link](${source})`, {}).includes(`href="${destination}"`), source);
  }
});

test('local search indexes escaped descriptions in the page introduction only', () => {
  const source = '---\ntitle: Guide\ndescription: Find tab completion & <startup> settings.\n---\n\n# Guide\n\nIntro.\n\n## Details\n\nSection.\n';
  const html = render(source, { relativePath: 'guide.md' }, md);
  assert.match(html, /<\/h1>\s*<p>Find tab completion &amp; &lt;startup&gt; settings\.<\/p>/);
  assert.equal(html.split('Find tab completion').length, 2);
  assert.ok(html.indexOf('Find tab completion') < html.indexOf('<h2'));
  assert.match(html, /Section\./);
});

test('local search preserves opt-out pages and rejects missing descriptions', () => {
  assert.equal(render('---\nsearch: false\n---\n\n# Not indexed\n', { relativePath: 'hidden.md' }, md), '');
  assert.throws(() => render('# Missing\n', { relativePath: 'missing.md' }, md),
    /missing\.md: search requires a nonempty page description/);
});
