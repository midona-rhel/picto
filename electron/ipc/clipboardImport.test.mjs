import { mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { expect, test } from 'vitest';
import { clipboardFilePaths } from './clipboardImport.mjs';

function clipboard(values = {}) {
  return {
    availableFormats: () => values.formats ?? [],
    read: (format) => values[format] ?? '',
    readBuffer: () => Buffer.alloc(0),
    readBookmark: () => values.bookmark ?? { title: '', url: '' },
    readText: () => values.text ?? '',
  };
}

test('macOS Finder file list wins over its clipboard icon preview', () => {
  const directory = mkdtempSync(join(tmpdir(), 'picto-clipboard-test-'));
  const path = join(directory, 'actual image & copy.jpeg');
  writeFileSync(path, 'actual bytes');
  const plist = `<plist><array><string>${path.replaceAll('&', '&amp;')}</string></array></plist>`;
  expect(clipboardFilePaths(clipboard({ NSFilenamesPboardType: plist }), 'darwin')).toEqual([path]);
});

test('plain file URLs remain portable clipboard imports', () => {
  const directory = mkdtempSync(join(tmpdir(), 'picto-clipboard-test-'));
  const path = join(directory, 'actual.png');
  writeFileSync(path, 'actual bytes');
  expect(clipboardFilePaths(clipboard({ text: `file://${path}` }), 'linux')).toEqual([path]);
});
