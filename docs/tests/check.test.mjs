import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

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
