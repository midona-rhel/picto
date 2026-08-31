import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, test } from 'vitest';
import { materializeDroppedMedia } from './dropImport.mjs';

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
    bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]),
    name: 'photo.png',
    mimeType: 'image/png',
  }, { outputDirectory: directory, createId: () => 'image' });

  assert.equal(result.path, join(directory, 'image.png'));
  assert.deepEqual([...readFileSync(result.path)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
  assert.equal(result.sourceUrl, null);
});

test('rejects mislabeled in-memory browser data', async () => {
  const directory = outputDirectory();
  await assert.rejects(
    materializeDroppedMedia({
      bytes: new TextEncoder().encode('<html>not an image</html>'),
      name: 'image.webp',
      mimeType: 'image/webp',
    }, { outputDirectory: directory }),
    /did not contain valid image data/,
  );
});

test('rejects URL-only browser drops without making a request', async () => {
  await assert.rejects(
    materializeDroppedMedia({ url: 'https://example.com/image.webp' }),
    /did not contain media data/,
  );
});
