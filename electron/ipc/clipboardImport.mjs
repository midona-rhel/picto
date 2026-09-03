import { existsSync, statSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

function escapeXml(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

export function writeClipboardFilePaths(
  clipboard,
  paths,
  { platform = process.platform, copyFiles } = {},
) {
  if (platform === 'darwin') {
    const entries = paths.map((path) => `<string>${escapeXml(path)}</string>`).join('');
    const plist = `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><array>${entries}</array></plist>`;
    clipboard.writeBuffer('NSFilenamesPboardType', Buffer.from(plist, 'utf8'));
    return;
  }
  if (copyFiles?.(paths)) return;
  if (platform === 'linux') {
    const uriList = `${paths.map((filePath) => {
      const url = new URL('file://');
      url.pathname = filePath;
      return url.href;
    }).join('\r\n')}\r\n`;
    clipboard.writeBuffer('text/uri-list', Buffer.from(uriList, 'utf8'));
    return;
  }
  throw new Error('Native file copying is unavailable on this platform.');
}

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
