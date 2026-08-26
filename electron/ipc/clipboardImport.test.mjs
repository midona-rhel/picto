import { mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { expect, test, vi } from 'vitest';
import { clipboardFilePaths, writeClipboardFilePaths } from './clipboardImport.mjs';

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

test('macOS writes a native multi-file pasteboard list with escaped paths', () => {
  const writeBuffer = vi.fn();
  writeClipboardFilePaths({ writeBuffer }, ['/one & two.jpg', '/three.png'], { platform: 'darwin' });
  expect(writeBuffer).toHaveBeenCalledOnce();
  const [format, value] = writeBuffer.mock.calls[0];
  expect(format).toBe('NSFilenamesPboardType');
  expect(value.toString('utf8')).toContain('<string>/one &amp; two.jpg</string>');
  expect(value.toString('utf8')).toContain('<string>/three.png</string>');
});

test('Linux writes the standard URI-list clipboard format', () => {
  const writeBuffer = vi.fn();
  writeClipboardFilePaths({ writeBuffer }, ['/one two.jpg', '/three.png'], { platform: 'linux' });
  expect(writeBuffer).toHaveBeenCalledWith(
    'text/uri-list',
    Buffer.from('file:///one%20two.jpg\r\nfile:///three.png\r\n'),
  );
});

test('Windows delegates to the native CF_HDROP writer', () => {
  const copyFiles = vi.fn(() => true);
  writeClipboardFilePaths({}, ['C:\\one.jpg', 'C:\\two.png'], { platform: 'win32', copyFiles });
  expect(copyFiles).toHaveBeenCalledWith(['C:\\one.jpg', 'C:\\two.png']);
});
