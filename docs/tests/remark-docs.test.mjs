import assert from 'node:assert/strict';
import { test } from 'node:test';
import { publishedLink, remarkDocs } from '../src/remark-docs.mjs';

test('guide links preserve filenames, fragments, and queries', () => {
  for (const [source, expected] of [
    ['commands-and-usage.md', 'commands-and-usage.html'],
    ['./private-overlays.md#merge-rules', './private-overlays.html#merge-rules'],
    ['windows-and-wsl.md?from=setup#requirements', 'windows-and-wsl.html?from=setup#requirements'],
    ['getting-started.md', 'getting-started.html'],
    ['./getting-started.md#getting-started', './getting-started.html#getting-started'],
    ['#getting-started', '#getting-started'],
    ['https://github.com/example/repo/blob/main/README.md', 'https://github.com/example/repo/blob/main/README.md'],
    ['//example.com/README.md', '//example.com/README.md'],
    ['mailto:fixture%40example.invalid', 'mailto:fixture%40example.invalid'],
    ['../assets/terminal-screenshot.svg', '../assets/terminal-screenshot.svg'],
  ]) {
    assert.equal(publishedLink(source), expected, source);
  }
});

test('the original title remains an anchor without duplicating Starlight’s h1', () => {
  const tree = {
    type: 'root',
    children: [
      {
        type: 'heading',
        depth: 1,
        children: [
          { type: 'text', value: 'Profiles and ' },
          { type: 'inlineCode', value: 'categories' },
        ],
      },
      { type: 'heading', depth: 2, children: [{ type: 'text', value: 'Getting started' }] },
      {
        type: 'paragraph',
        children: [{ type: 'link', url: 'commands-and-usage.md#dry-run-scope', children: [{ type: 'text', value: 'Preview' }] }],
      },
      { type: 'definition', identifier: 'home', url: 'getting-started.md' },
    ],
  };
  remarkDocs()(tree);
  assert.equal(tree.children[0].type, 'html');
  assert.match(tree.children[0].value, /id="profiles-and-categories"/);
  assert.equal(tree.children[1].depth, 2);
  assert.equal(tree.children[2].children[0].url, 'commands-and-usage.html#dry-run-scope');
  assert.equal(tree.children[3].url, 'getting-started.html');
});

test('a page without a Markdown title still rewrites its guide links', () => {
  const tree = { type: 'root', children: [{ type: 'definition', identifier: 'usage', url: 'commands-and-usage.md' }] };
  remarkDocs()(tree);
  assert.equal(tree.children[0].url, 'commands-and-usage.html');
});
