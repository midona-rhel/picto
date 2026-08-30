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

test('resolves the primary image from a dropped image page', async () => {
  const outputDirectory = mkdtempSync(join(tmpdir(), 'picto-drop-test-'));
  const requests = [];
  const result = await materializeDroppedMedia({ url: 'https://example.com/view/1' }, {
    outputDirectory,
    createId: () => 'page-image',
    fetchImpl: async (url, options) => {
      requests.push({ url, options });
      if (url.endsWith('/view/1')) {
        return new Response('<html><img id="img" src="/media/image.webp"></html>', {
          headers: { 'content-type': 'text/html; charset=utf-8' },
        });
      }
      return new Response(new Uint8Array([6, 7]), {
        headers: { 'content-type': 'image/webp' },
      });
    },
  });

  assert.equal(result.path, join(outputDirectory, 'page-image.webp'));
  assert.equal(result.sourceUrl, 'https://example.com/view/1');
  assert.deepEqual(requests, [
    { url: 'https://example.com/view/1', options: undefined },
    {
      url: 'https://example.com/media/image.webp',
      options: { headers: { referer: 'https://example.com/view/1' } },
    },
  ]);
});

test('rejects non-web URL drops', async () => {
  await assert.rejects(
    materializeDroppedMedia({ url: 'file:///private/item.png' }, { fetchImpl: fetch }),
    /Only HTTP and HTTPS/,
  );
});
