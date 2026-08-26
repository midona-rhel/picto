import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

const MAX_ICON_BYTES = 512 * 1024;
const ICON_PATHS = ['/apple-touch-icon.png', '/favicon.ico'];

function normalizeDomain(value) {
  if (typeof value !== 'string') throw new Error('Invalid site icon domain');
  const domain = value.trim().toLowerCase();
  if (!domain || domain.length > 253 || !/^[a-z0-9.-]+$/.test(domain) || domain.includes('..')) {
    throw new Error('Invalid site icon domain');
  }
  return domain;
}

function detectMime(bytes, contentType) {
  if (bytes.length >= 8 && bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) return 'image/png';
  if (bytes.length >= 4 && bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 1 && bytes[3] === 0) return 'image/x-icon';
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return 'image/jpeg';
  if (bytes.length >= 6 && (bytes.subarray(0, 6).toString('ascii') === 'GIF87a' || bytes.subarray(0, 6).toString('ascii') === 'GIF89a')) return 'image/gif';
  if (bytes.length >= 12 && bytes.subarray(0, 4).toString('ascii') === 'RIFF' && bytes.subarray(8, 12).toString('ascii') === 'WEBP') return 'image/webp';

  const normalized = contentType?.split(';', 1)[0]?.trim().toLowerCase();
  return normalized === 'image/vnd.microsoft.icon' || normalized === 'image/x-icon'
    ? 'image/x-icon'
    : null;
}

function cacheName(domain) {
  return createHash('sha256').update(domain).digest('hex');
}

export function createSiteIconService({ cacheDirectory, fetchImpl }) {
  const inFlight = new Map();

  async function readCached(domain) {
    try {
      const record = JSON.parse(await fs.readFile(path.join(cacheDirectory, `${cacheName(domain)}.json`), 'utf8'));
      if (typeof record.mime !== 'string' || typeof record.data !== 'string') return null;
      return `data:${record.mime};base64,${record.data}`;
    } catch {
      return null;
    }
  }

  async function fetchAndCache(domain) {
    const cached = await readCached(domain);
    if (cached) return cached;

    for (const iconPath of ICON_PATHS) {
      try {
        const response = await fetchImpl(`https://${domain}${iconPath}`, {
          method: 'GET',
          redirect: 'follow',
          signal: AbortSignal.timeout(7000),
          headers: { Accept: 'image/png,image/vnd.microsoft.icon,image/x-icon,image/webp,image/jpeg,image/gif' },
        });
        if (!response.ok) continue;

        const contentLength = Number(response.headers.get('content-length') ?? 0);
        if (contentLength > MAX_ICON_BYTES) continue;
        const bytes = Buffer.from(await response.arrayBuffer());
        if (bytes.length === 0 || bytes.length > MAX_ICON_BYTES) continue;
        const mime = detectMime(bytes, response.headers.get('content-type'));
        if (!mime) continue;

        const record = JSON.stringify({ mime, data: bytes.toString('base64') });
        await fs.mkdir(cacheDirectory, { recursive: true });
        const destination = path.join(cacheDirectory, `${cacheName(domain)}.json`);
        const temporary = `${destination}.${process.pid}.tmp`;
        await fs.writeFile(temporary, record, { encoding: 'utf8', mode: 0o600 });
        await fs.rename(temporary, destination);
        return `data:${mime};base64,${bytes.toString('base64')}`;
      } catch {
        // Try the conventional favicon before falling back to the local icon.
      }
    }
    return null;
  }

  return {
    get(domainInput) {
      const domain = normalizeDomain(domainInput);
      const existing = inFlight.get(domain);
      if (existing) return existing;
      const request = fetchAndCache(domain).finally(() => inFlight.delete(domain));
      inFlight.set(domain, request);
      return request;
    },
  };
}
