import { randomUUID } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

const MIME_EXTENSIONS = new Map([
  ['image/avif', '.avif'],
  ['image/bmp', '.bmp'],
  ['image/gif', '.gif'],
  ['image/x-icon', '.ico'],
  ['image/vnd.microsoft.icon', '.ico'],
  ['image/jpeg', '.jpg'],
  ['image/png', '.png'],
  ['image/svg+xml', '.svg'],
  ['image/tiff', '.tiff'],
  ['image/webp', '.webp'],
]);
const IMAGE_EXTENSIONS = new Set([...MIME_EXTENSIONS.values(), '.jpeg', '.tif']);

export async function fetchWithBlockedClientFallback(primaryFetch, fallbackFetch, url, options) {
  try {
    return await primaryFetch(url, options);
  } catch (error) {
    if (!String(error?.message ?? error).includes('ERR_BLOCKED_BY_CLIENT')) throw error;
    return fallbackFetch(url, options);
  }
}

function safeExtension(name, mimeType) {
  const extension = extname(name ?? '').toLowerCase();
  if (IMAGE_EXTENSIONS.has(extension)) return extension;
  return MIME_EXTENSIONS.get(mimeType?.split(';', 1)[0]?.toLowerCase()) ?? '.bin';
}

function detectImageMimeType(bytes) {
  if (bytes.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) return 'image/png';
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return 'image/jpeg';
  const header = bytes.subarray(0, 12).toString('ascii');
  if (header.startsWith('GIF87a') || header.startsWith('GIF89a')) return 'image/gif';
  if (header.startsWith('RIFF') && header.slice(8, 12) === 'WEBP') return 'image/webp';
  if (header.startsWith('BM')) return 'image/bmp';
  if (bytes.subarray(0, 4).equals(Buffer.from([0x49, 0x49, 0x2a, 0x00])) || bytes.subarray(0, 4).equals(Buffer.from([0x4d, 0x4d, 0x00, 0x2a]))) return 'image/tiff';
  if (bytes.subarray(0, 4).equals(Buffer.from([0x00, 0x00, 0x01, 0x00]))) return 'image/x-icon';
  if (header.slice(4, 8) === 'ftyp' && /^(avif|avis)$/.test(header.slice(8, 12))) return 'image/avif';
  if (/^(?:\uFEFF|\s)*(?:<\?xml[^>]*>\s*)?<svg\b/i.test(bytes.subarray(0, 1024).toString('utf8'))) return 'image/svg+xml';
  return null;
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

    const detectedMimeType = detectImageMimeType(bytes);
    if (!detectedMimeType) {
      throw new Error(`The dropped URL did not return a supported image (${mimeType || 'unknown content type'}).`);
    }
    mimeType = detectedMimeType;
  }

  if (!bytes?.length) throw new Error('The dropped browser item did not contain media data.');
  const detectedMimeType = detectImageMimeType(bytes);
  if (!detectedMimeType) {
    throw new Error('The dropped browser item did not contain valid image data.');
  }
  mimeType = detectedMimeType;
  mkdirSync(outputDirectory, { recursive: true });
  const sourceName = input?.name || (sourceUrl ? new URL(sourceUrl).pathname : '');
  const path = join(outputDirectory, `${createId()}${safeExtension(sourceName, mimeType)}`);
  writeFileSync(path, bytes);
  return { path, sourceUrl };
}
