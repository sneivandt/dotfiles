import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createMarkdownRenderer } from 'vitepress';
import config from '../.vitepress/config.mts';

const md = await createMarkdownRenderer(fileURLToPath(new URL('../content/', import.meta.url)), config.markdown);
const render = config.themeConfig.search.options._render;

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
