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

export async function materializeDroppedMedia(input, {
  outputDirectory = join(tmpdir(), 'picto-browser-drops'),
  createId = randomUUID,
} = {}) {
  const bytes = input?.bytes ? Buffer.from(input.bytes) : null;

  if (!bytes?.length) throw new Error('The dropped browser item did not contain media data.');
  const mimeType = detectImageMimeType(bytes);
  if (!mimeType) {
    throw new Error('The dropped browser item did not contain valid image data.');
  }
  mkdirSync(outputDirectory, { recursive: true });
  const path = join(outputDirectory, `${createId()}${safeExtension(input?.name, mimeType)}`);
  writeFileSync(path, bytes);
  return { path, sourceUrl: null };
}
