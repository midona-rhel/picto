import { existsSync, statSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';

function decodeXml(value) {
  return value
    .replaceAll('&amp;', '&')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'");
}

function addCandidate(candidates, value) {
  const trimmed = value?.trim();
  if (!trimmed || trimmed.startsWith('#')) return;
  try {
    candidates.push(trimmed.startsWith('file:') ? fileURLToPath(trimmed) : trimmed);
  } catch {}
}

export function clipboardFilePaths(clipboard, platform = process.platform) {
  const candidates = [];
  if (platform === 'darwin') {
    for (const match of clipboard.read('NSFilenamesPboardType').matchAll(/<string>([\s\S]*?)<\/string>/g)) {
      addCandidate(candidates, decodeXml(match[1]));
    }
    addCandidate(candidates, clipboard.read('public.file-url'));
    const bookmark = clipboard.readBookmark();
    addCandidate(candidates, bookmark?.url);
  }
  for (const format of clipboard.availableFormats?.() ?? []) {
    if (!/(file.?url|uri.?list|filenames)/i.test(format)) continue;
    try {
      const raw = (clipboard.read(format) || clipboard.readBuffer(format).toString('utf8'))
        .replaceAll('\0', '');
      for (const value of raw.split(/[\r\n]+/)) addCandidate(candidates, value);
    } catch {}
  }
  for (const value of clipboard.readText().split(/\r?\n/)) addCandidate(candidates, value);

  return [...new Set(candidates)].filter((path) => {
    try {
      const metadata = isAbsolute(path) && existsSync(path) ? statSync(path) : null;
      return metadata?.isFile() || metadata?.isDirectory() || false;
    } catch {
      return false;
    }
  });
}

export function clipboardHasImport(clipboard, platform = process.platform) {
  return clipboardFilePaths(clipboard, platform).length > 0 || !clipboard.readImage().isEmpty();
}
