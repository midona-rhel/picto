import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, test } from 'vitest';
import { fetchWithBlockedClientFallback, materializeDroppedMedia } from './dropImport.mjs';

test('retries client-blocked Electron requests through the fallback transport', async () => {
  const calls = [];
  const response = await fetchWithBlockedClientFallback(
    async () => { throw new Error('net::ERR_BLOCKED_BY_CLIENT'); },
    async (url, options) => {
      calls.push({ url, options });
      return 'downloaded';
    },
    'https://example.com/image.webp',
    { headers: { referer: 'https://example.com/view' } },
  );

  assert.equal(response, 'downloaded');
  assert.deepEqual(calls, [{
    url: 'https://example.com/image.webp',
    options: { headers: { referer: 'https://example.com/view' } },
  }]);
});

test('does not hide ordinary network failures behind the fallback transport', async () => {
  let fallbackCalled = false;
  await assert.rejects(
    fetchWithBlockedClientFallback(
      async () => { throw new Error('net::ERR_NAME_NOT_RESOLVED'); },
      async () => { fallbackCalled = true; },
      'https://example.invalid/image.webp',
    ),
    /ERR_NAME_NOT_RESOLVED/,
  );
  assert.equal(fallbackCalled, false);
});

const outputDirectories = [];

afterEach(() => {
  for (const path of outputDirectories.splice(0)) rmSync(path, { recursive: true, force: true });
});

function outputDirectory() {
  const path = mkdtempSync(join(tmpdir(), 'picto-drop-test-'));
  outputDirectories.push(path);
  return path;
}

test('materializes an in-memory browser image with its media extension', async () => {
  const directory = outputDirectory();
  const result = await materializeDroppedMedia({
    bytes: new Uint8Array([1, 2, 3]),
    name: 'photo.png',
    mimeType: 'image/png',
  }, { outputDirectory: directory, createId: () => 'image' });

  assert.equal(result.path, join(directory, 'image.png'));
  assert.deepEqual([...readFileSync(result.path)], [1, 2, 3]);
  assert.equal(result.sourceUrl, null);
});

test('downloads a URL-only browser drag and retains its source URL', async () => {
  const directory = outputDirectory();
  const result = await materializeDroppedMedia({ url: 'https://example.com/image' }, {
    outputDirectory: directory,
    createId: () => 'download',
    fetchImpl: async () => new Response(new Uint8Array([4, 5]), {
      headers: { 'content-type': 'image/webp' },
    }),
  });

  assert.equal(result.path, join(directory, 'download.webp'));
  assert.deepEqual([...readFileSync(result.path)], [4, 5]);
  assert.equal(result.sourceUrl, 'https://example.com/image');
});

test('resolves the primary image from a dropped image page', async () => {
  const directory = outputDirectory();
  const requests = [];
  const result = await materializeDroppedMedia({ url: 'https://example.com/view/1' }, {
    outputDirectory: directory,
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

  assert.equal(result.path, join(directory, 'page-image.webp'));
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
