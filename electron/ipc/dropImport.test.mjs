import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { materializeDroppedMedia } from './dropImport.mjs';

test('materializes an in-memory browser image with its media extension', async () => {
  const outputDirectory = mkdtempSync(join(tmpdir(), 'picto-drop-test-'));
  const result = await materializeDroppedMedia({
    bytes: new Uint8Array([1, 2, 3]),
    name: 'photo.png',
    mimeType: 'image/png',
  }, { outputDirectory, createId: () => 'image' });

  assert.equal(result.path, join(outputDirectory, 'image.png'));
  assert.deepEqual([...readFileSync(result.path)], [1, 2, 3]);
  assert.equal(result.sourceUrl, null);
});

test('downloads a URL-only browser drag and retains its source URL', async () => {
  const outputDirectory = mkdtempSync(join(tmpdir(), 'picto-drop-test-'));
  const result = await materializeDroppedMedia({ url: 'https://example.com/image' }, {
    outputDirectory,
    createId: () => 'download',
    fetchImpl: async () => new Response(new Uint8Array([4, 5]), {
      headers: { 'content-type': 'image/webp' },
    }),
  });

  assert.equal(result.path, join(outputDirectory, 'download.webp'));
  assert.deepEqual([...readFileSync(result.path)], [4, 5]);
  assert.equal(result.sourceUrl, 'https://example.com/image');
});

test('rejects non-web URL drops', async () => {
  await assert.rejects(
    materializeDroppedMedia({ url: 'file:///private/item.png' }, { fetchImpl: fetch }),
    /Only HTTP and HTTPS/,
  );
});
