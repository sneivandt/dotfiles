import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const svgPaths = (relativePath) =>
  Array.from(
    readFileSync(new URL(relativePath, import.meta.url), 'utf8').matchAll(/<path\b[^>]*\bd="([^"]+)"/g),
    ([, path]) => path,
  );

test('hero wordmark mask keeps the exact glyph paths used in the README and navbar logos', () => {
  const heroWordmark = svgPaths('../.vitepress/theme/wordmark.svg');
  assert.equal(heroWordmark.length, 8);
  for (const logo of ['logo-dark.svg', 'logo-light.svg', 'logo-readme.svg']) {
    assert.deepEqual(
      heroWordmark,
      svgPaths(`../public/assets/${logo}`),
      `wordmark.svg paths drifted from ${logo}`,
    );
  }
});

test('native documentation checker preserves scope, ordering, and committed inputs', () => {
  const python = process.env.DOCS_TEST_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3');
  const result = spawnSync(python, ['-B', fileURLToPath(new URL('test_check.py', import.meta.url))], {
    encoding: 'utf8',
  });

  assert.equal(result.status, 0, result.error?.message ?? `${result.stdout}\n${result.stderr}`);
});

test('documentation source links and metadata reject drift without network access', () => {
  const python = process.env.DOCS_TEST_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3');
  const result = spawnSync(python, ['-B', fileURLToPath(new URL('test_content.py', import.meta.url))], {
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.error?.message ?? `${result.stdout}\n${result.stderr}`);
});
