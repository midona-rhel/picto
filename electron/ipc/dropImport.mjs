import { randomUUID } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const MIME_EXTENSIONS = new Map([
  ['image/avif', '.avif'],
  ['image/bmp', '.bmp'],
  ['image/gif', '.gif'],
  ['image/jpeg', '.jpg'],
  ['image/png', '.png'],
  ['image/svg+xml', '.svg'],
  ['image/tiff', '.tiff'],
  ['image/webp', '.webp'],
]);

function safeExtension(name, mimeType) {
  const extension = extname(name ?? '').toLowerCase();
  if (/^\.[a-z0-9]{1,8}$/.test(extension)) return extension;
  return MIME_EXTENSIONS.get(mimeType?.split(';', 1)[0]?.toLowerCase()) ?? '.bin';
}

function decodeHtmlUrl(value) {
  return value.replaceAll('&amp;', '&').replaceAll('&#39;', "'").replaceAll('&quot;', '"');
}

function imageUrlFromHtml(html, pageUrl) {
  const candidates = [
    /<img\b(?=[^>]*\bid=["']img["'])[^>]*\bsrc=["']([^"']+)/i,
    /<meta\b(?=[^>]*\bproperty=["']og:image["'])[^>]*\bcontent=["']([^"']+)/i,
    /<meta\b(?=[^>]*\bcontent=["']([^"']+)["'])[^>]*\bproperty=["']og:image["']/i,
    /<img\b[^>]*\bsrc=["']([^"']+)/i,
  ];
  for (const pattern of candidates) {
    const match = html.match(pattern);
    if (match?.[1]) return new URL(decodeHtmlUrl(match[1]), pageUrl).href;
  }
  return null;
}

export async function materializeDroppedMedia(input, {
  fetchImpl,
  outputDirectory = join(tmpdir(), 'picto-browser-drops'),
  createId = randomUUID,
} = {}) {
  let bytes = input?.bytes ? Buffer.from(input.bytes) : null;
  let mimeType = input?.mimeType ?? '';
  let sourceUrl = null;

  if (!bytes && input?.url) {
    const parsed = new URL(input.url);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('Only HTTP and HTTPS browser drops are supported.');
    }
    if (!fetchImpl) throw new Error('Browser image downloading is unavailable.');
    let response = await fetchImpl(parsed.href);
    if (!response.ok) throw new Error(`Could not download dropped media (${response.status}).`);
    bytes = Buffer.from(await response.arrayBuffer());
    mimeType ||= response.headers.get('content-type') ?? '';
    sourceUrl = parsed.href;

    if (mimeType.toLowerCase().startsWith('text/html')) {
      const imageUrl = imageUrlFromHtml(bytes.toString('utf8'), parsed.href);
      if (!imageUrl) throw new Error('The dropped web page did not contain an importable image.');
      response = await fetchImpl(imageUrl, { headers: { referer: parsed.href } });
      if (!response.ok) throw new Error(`Could not download dropped image (${response.status}).`);
      bytes = Buffer.from(await response.arrayBuffer());
      mimeType = response.headers.get('content-type') ?? '';
    }
  }

  if (!bytes?.length) throw new Error('The dropped browser item did not contain media data.');
  mkdirSync(outputDirectory, { recursive: true });
  const path = join(outputDirectory, `${createId()}${safeExtension(input?.name, mimeType)}`);
  writeFileSync(path, bytes);
  return { path, sourceUrl };
}
