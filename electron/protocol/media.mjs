import fs from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import { forwardWarn } from '../services/logForwarder.mjs';

export function isValidHash(value) {
  return typeof value === 'string' && value.length === 64 && /^[a-fA-F0-9]+$/.test(value);
}

export function parseMediaUrl(urlString) {
  const url = new URL(urlString);
  const parts = url.pathname.split('/').filter(Boolean);
  if (parts.length !== 2) return null;
  const [kind, segment] = parts;
  if (kind === 'thumb') {
    const match = segment.match(/^([a-fA-F0-9]{64})\.jpg$/);
    if (!match) return null;
    return { kind: 'thumb', hash: match[1], ext: 'jpg' };
  }
  if (kind === 'file') {
    const match = segment.match(/^([a-fA-F0-9]{64})\.([a-zA-Z0-9]+)$/);
    if (!match) return null;
    return { kind: 'file', hash: match[1], ext: match[2].toLowerCase() };
  }
  return null;
}

export function extToMime(ext) {
  const mimeByExt = {
    jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif', webp: 'image/webp',
    bmp: 'image/bmp', tiff: 'image/tiff', tif: 'image/tiff', svg: 'image/svg+xml', avif: 'image/avif',
    heif: 'image/heif', heic: 'image/heif', jxl: 'image/jxl', ico: 'image/x-icon', psd: 'image/vnd.adobe.photoshop',
    mp4: 'video/mp4', webm: 'video/webm', mkv: 'video/x-matroska', mov: 'video/quicktime', flv: 'video/x-flv',
    avi: 'video/x-msvideo', flac: 'audio/flac', wav: 'audio/x-wav', pdf: 'application/pdf', epub: 'application/epub+zip',
  };
  return mimeByExt[ext] || 'application/octet-stream';
}

export function parseRange(range, size) {
  if (!range || !range.startsWith('bytes=')) return null;
  const spec = range.slice(6).split(',')[0].trim();
  if (spec.startsWith('-')) {
    const count = Number(spec.slice(1));
    if (!Number.isFinite(count) || count <= 0 || count > size) return null;
    return { start: size - count, end: size - 1 };
  }
  if (spec.endsWith('-')) {
    const start = Number(spec.slice(0, -1));
    if (!Number.isFinite(start) || start < 0 || start >= size) return null;
    return { start, end: size - 1 };
  }
  const [start, endRaw] = spec.split('-', 2).map(Number);
  if (!Number.isFinite(start) || !Number.isFinite(endRaw) || start < 0 || endRaw < start || start >= size) return null;
  return { start, end: Math.min(endRaw, size - 1) };
}

export function createMediaProtocolService({
  protocol,
  path,
  invoke,
  isDev,
  getCurrentLibraryRoot,
}) {
  const thumbEnsureInFlight = new Map();
  const thumbMetaCache = new Map();
  const fileMetaCache = new Map();
  let cachedRoot = null;

  function syncCachesForCurrentRoot() {
    const root = getCurrentLibraryRoot() ?? null;
    if (root === cachedRoot) return root;
    cachedRoot = root;
    thumbMetaCache.clear();
    fileMetaCache.clear();
    thumbEnsureInFlight.clear();
    return root;
  }

  async function tryStat(filePath) {
    try {
      return await fs.stat(filePath);
    } catch {
      return null;
    }
  }

  function buildMeta(filePath, stat, extHint = '') {
    return {
      filePath,
      size: stat.size,
      actualExt: path.extname(filePath).slice(1).toLowerCase() || extHint,
    };
  }

  function buildBlobPath(kind, hash, ext) {
    const root = syncCachesForCurrentRoot();
    if (!root) return '';
    const ab = hash.slice(0, 2);
    const cd = hash.slice(2, 4);
    if (kind === 'thumb') return path.join(root, 'blobs', 't', ab, cd, `${hash}.jpg`);
    return path.join(root, 'blobs', 'f', ab, cd, `${hash}.${ext}`);
  }

  async function resolveOriginalMeta(hash, extHint) {
    const root = syncCachesForCurrentRoot();
    if (!root) return null;
    const cached = fileMetaCache.get(hash);
    if (cached) return cached;
    const ab = hash.slice(0, 2);
    const cd = hash.slice(2, 4);
    const dir = path.join(root, 'blobs', 'f', ab, cd);
    const hinted = path.join(dir, `${hash}.${extHint}`);
    const hintedStat = await tryStat(hinted);
    if (hintedStat) {
      const meta = buildMeta(hinted, hintedStat, extHint);
      fileMetaCache.set(hash, meta);
      return meta;
    }
    try {
      const entries = await fs.readdir(dir);
      const prefix = `${hash}.`;
      const found = entries.find((name) => name.startsWith(prefix));
      if (found) {
        const filePath = path.join(dir, found);
        const stat = await tryStat(filePath);
        if (stat) {
          const meta = buildMeta(filePath, stat, extHint);
          fileMetaCache.set(hash, meta);
          return meta;
        }
      }
    } catch {}
    return null;
  }

  async function resolveThumbMeta(hash) {
    const root = syncCachesForCurrentRoot();
    if (!root) return null;
    const cached = thumbMetaCache.get(hash);
    if (cached) return cached;
    const ab = hash.slice(0, 2);
    const cd = hash.slice(2, 4);
    const dir = path.join(root, 'blobs', 't', ab, cd);
    const jpg = path.join(dir, `${hash}.jpg`);
    const jpgStat = await tryStat(jpg);
    if (jpgStat) {
      const meta = buildMeta(jpg, jpgStat, 'jpg');
      thumbMetaCache.set(hash, meta);
      return meta;
    }
    const png = path.join(dir, `${hash}.png`);
    const pngStat = await tryStat(png);
    if (pngStat) {
      const meta = buildMeta(png, pngStat, 'png');
      thumbMetaCache.set(hash, meta);
      return meta;
    }
    return null;
  }

  async function ensureThumbBefore404(hash) {
    const existing = thumbEnsureInFlight.get(hash);
    if (existing) {
      await existing;
      return;
    }
    const task = (async () => {
      try {
        await invoke('ensure_thumbnail', { hash });
      } catch {}
    })().finally(() => {
      thumbEnsureInFlight.delete(hash);
    });
    thumbEnsureInFlight.set(hash, task);
    await task;
  }

  async function registerMediaProtocol() {
    protocol.handle('media', async (request) => {
      syncCachesForCurrentRoot();
      const parsed = parseMediaUrl(request.url);
      if (!parsed || !isValidHash(parsed.hash)) {
        forwardWarn('media', `Failed to parse: ${request.url}`);
        return new Response('Invalid media URL', {
          status: 400,
          headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
        });
      }

      let meta = parsed.kind === 'thumb'
        ? await resolveThumbMeta(parsed.hash)
        : await resolveOriginalMeta(parsed.hash, parsed.ext);

      if (!meta && parsed.kind === 'thumb') {
        await ensureThumbBefore404(parsed.hash);
        meta = await resolveThumbMeta(parsed.hash);
      }

      if (!meta) {
        const missingPath = parsed.kind === 'thumb'
          ? buildBlobPath(parsed.kind, parsed.hash, 'jpg')
          : buildBlobPath(parsed.kind, parsed.hash, parsed.ext);
        forwardWarn('media', `404: ${parsed.kind} ${parsed.hash.slice(0, 12)} ${missingPath}`);
        return new Response('Not found', {
          status: 404,
          headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
        });
      }

      const mime = parsed.kind === 'thumb'
        ? extToMime(meta.actualExt || 'jpg')
        : extToMime(meta.actualExt || parsed.ext);
      const range = parseRange(request.headers.get('range'), meta.size);

      if (!range) {
        const stream = createReadStream(meta.filePath);
        return new Response(Readable.toWeb(stream), {
          status: 200,
          headers: {
            'Content-Type': mime,
            'Content-Length': String(meta.size),
            'Accept-Ranges': 'bytes',
            'Cache-Control': 'public, max-age=31536000, immutable',
          },
        });
      }

      const length = range.end - range.start + 1;
      const stream = createReadStream(meta.filePath, { start: range.start, end: range.end });
      return new Response(Readable.toWeb(stream), {
        status: 206,
        headers: {
          'Content-Type': mime,
          'Content-Length': String(length),
          'Content-Range': `bytes ${range.start}-${range.end}/${meta.size}`,
          'Accept-Ranges': 'bytes',
          'Cache-Control': 'public, max-age=31536000, immutable',
        },
      });
    });
  }

  return {
    buildBlobPath,
    registerMediaProtocol,
  };
}
