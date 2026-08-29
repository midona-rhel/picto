import fs from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import { forwardLog, forwardWarn } from '../services/logForwarder.mjs';

const DOCUMENT_THUMBNAIL_MIMES = new Set([
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  'text/plain', 'text/markdown', 'application/json', 'application/rtf',
  'application/epub+zip', 'application/vnd.comicbook+zip', 'image/vnd.djvu',
]);

export function isValidHash(value) {
  return typeof value === 'string' && value.length === 64 && /^[a-fA-F0-9]+$/.test(value);
}

export function parseMediaUrl(urlString) {
  const url = new URL(urlString);
  const parts = url.pathname.split('/').filter(Boolean);
  if (parts.length !== 2) return null;
  const [kind, segment] = parts;
  if (kind === 'library-cover' && segment === 'cover') {
    const libraryRoot = url.searchParams.get('library');
    return libraryRoot ? { kind: 'library-cover', libraryRoot } : null;
  }
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
    avi: 'video/x-msvideo',
    aac: 'audio/aac', flac: 'audio/flac', m4a: 'audio/mp4', mka: 'audio/x-matroska', mp3: 'audio/mpeg',
    oga: 'audio/ogg', ogg: 'audio/ogg', opus: 'audio/opus', tta: 'audio/x-tta', wav: 'audio/x-wav',
    wma: 'audio/x-ms-wma', wv: 'audio/wavpack',
    pdf: 'application/pdf', epub: 'application/epub+zip', cbz: 'application/vnd.comicbook+zip',
    djvu: 'image/vnd.djvu', djv: 'image/vnd.djvu', docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation', swf: 'application/x-shockwave-flash',
    txt: 'text/plain', md: 'text/markdown', markdown: 'text/markdown', json: 'application/json', rtf: 'application/rtf',
    ttf: 'font/ttf', ttc: 'font/collection', otf: 'font/otf', woff: 'font/woff',
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
  getKnownLibraryRoots = () => [],
  flashThumbnail,
  pdfThumbnail,
  documentThumbnail,
  onThumbnailReady = () => {},
}) {
  const thumbRequestInFlight = new Map();
  const thumbRequestsQueued = new Set();
  const thumbMetaCache = new Map();
  const fileMetaCache = new Map();
  let cachedRoot = null;

  function syncCachesForCurrentRoot() {
    const root = getCurrentLibraryRoot() ?? null;
    if (root === cachedRoot) return root;
    cachedRoot = root;
    thumbMetaCache.clear();
    fileMetaCache.clear();
    thumbRequestInFlight.clear();
    thumbRequestsQueued.clear();
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
    if (kind === 'thumb') return path.join(root, 'blobs', 't', ab, cd, `${hash}.${ext}`);
    return path.join(root, 'blobs', 'f', ab, cd, `${hash}.${ext}`);
  }

  async function resolveOriginalMeta(hash, extHint) {
    const root = syncCachesForCurrentRoot();
    if (!root) return null;
    const cached = fileMetaCache.get(hash);
    if (cached) {
      if (await tryStat(cached.filePath)) return cached;
      fileMetaCache.delete(hash);
    }
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
    try {
      const resolved = await invoke('media.resolve_paths', { file_hashes: [hash] });
      const filePath = resolved?.[0]?.path;
      if (typeof filePath === 'string') {
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

  function originalLibraryRoot(filePath) {
    const cdDirectory = path.dirname(filePath);
    const abDirectory = path.dirname(cdDirectory);
    const originalsDirectory = path.dirname(abDirectory);
    const blobsDirectory = path.dirname(originalsDirectory);
    if (path.basename(originalsDirectory) !== 'f' || path.basename(blobsDirectory) !== 'blobs') {
      return null;
    }
    return path.dirname(blobsDirectory);
  }

  async function resolveThumbMeta(hash) {
    const root = syncCachesForCurrentRoot();
    if (!root) return null;
    const cached = thumbMetaCache.get(hash);
    if (cached) return cached;
    const roots = [root];
    const sourceRoot = originalLibraryRoot(fileMetaCache.get(hash)?.filePath ?? '');
    if (sourceRoot && sourceRoot !== root) roots.push(sourceRoot);
    for (const candidateRoot of roots) {
      const ab = hash.slice(0, 2);
      const cd = hash.slice(2, 4);
      const dir = path.join(candidateRoot, 'blobs', 't', ab, cd);
      for (const extension of ['jpg', 'png']) {
        const filePath = path.join(dir, `${hash}.${extension}`);
        const stat = await tryStat(filePath);
        if (!stat) continue;
        const meta = buildMeta(filePath, stat, extension);
        thumbMetaCache.set(hash, meta);
        thumbRequestsQueued.delete(hash);
        return meta;
      }
    }
    return null;
  }

  function invalidateThumbnail(hash) {
    thumbMetaCache.delete(hash);
    thumbRequestInFlight.delete(hash);
    thumbRequestsQueued.delete(hash);
  }

  async function resolveLibraryCoverMeta(libraryRoot) {
    for (const extension of ['jpg', 'png']) {
      const filePath = path.join(libraryRoot, `.picto-library-cover.${extension}`);
      const stat = await tryStat(filePath);
      if (stat) return buildMeta(filePath, stat, extension);
    }
    return null;
  }

  async function setThumbnail(hash, pngBase64) {
    if (!isValidHash(hash)) throw new Error('Invalid thumbnail hash');
    const png = Buffer.from(String(pngBase64 ?? ''), 'base64');
    if (png.length === 0 || png.length > 32 * 1024 * 1024) throw new Error('Invalid thumbnail size');
    if (!png.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) {
      throw new Error('Thumbnail data is not a PNG');
    }
    const outputPath = buildBlobPath('thumb', hash, 'png');
    const oldJpgPath = buildBlobPath('thumb', hash, 'jpg');
    const temporaryPath = `${outputPath}.${process.pid}.${Date.now()}.tmp`;
    await fs.mkdir(path.dirname(outputPath), { recursive: true });
    try {
      await fs.writeFile(temporaryPath, png);
      await fs.rename(temporaryPath, outputPath);
      await fs.rm(oldJpgPath, { force: true });
    } finally {
      await fs.rm(temporaryPath, { force: true });
    }
    invalidateThumbnail(hash);
  }

  async function renderThumbnailNow(hash) {
    const existing = thumbRequestInFlight.get(hash);
    if (existing) {
      await existing;
      return;
    }
    const task = (async () => {
      try {
        await invoke('media.render_thumbnail_now', { file_hash: hash });
      } catch {}
      if (!await resolveThumbMeta(hash)) await renderExternalThumbnail(hash);
    })().finally(() => {
      thumbRequestInFlight.delete(hash);
    });
    thumbRequestInFlight.set(hash, task);
    await task;
  }

  function scheduleThumbnailAfterMiss(hash) {
    if (thumbRequestInFlight.has(hash) || thumbRequestsQueued.has(hash)) return;
    const task = (async () => {
      try {
        const requested = await invoke('media.request_thumbnail', { file_hash: hash });
        if (requested?.ready) {
          onThumbnailReady(hash);
          return;
        }
        if (requested?.supported) {
          thumbRequestsQueued.add(hash);
          if (requested.queued) {
            forwardLog('DEBUG', 'media', `Thumbnail queued: ${hash.slice(0, 12)}`);
          }
          return;
        }
      } catch {}

      // Browser-backed formats are also generated off the request path. The
      // failed image response remains a placeholder until this event retries it.
      if (await renderExternalThumbnail(hash)) onThumbnailReady(hash);
    })().catch((error) => {
      forwardWarn('media', `Deferred thumbnail failed: ${error?.message ?? String(error)}`);
    }).finally(() => {
      thumbRequestInFlight.delete(hash);
    });
    thumbRequestInFlight.set(hash, task);
  }

  async function renderExternalThumbnail(hash) {
    const original = await resolveOriginalMeta(hash, 'bin');
    if (!original) return false;
    const outputPath = buildBlobPath('thumb', hash, 'png');
    if (original.actualExt === 'swf' && flashThumbnail) {
      await flashThumbnail.render({
        sourceUrl: `media://localhost/file/${hash}.swf`,
        outputPath,
      });
    } else if (original.actualExt === 'pdf' && pdfThumbnail) {
      await pdfThumbnail.render({
        sourceUrl: `media://localhost/file/${hash}.pdf`,
        outputPath,
      });
    } else {
      const mimeType = extToMime(original.actualExt);
      if (!documentThumbnail || !DOCUMENT_THUMBNAIL_MIMES.has(mimeType)) return false;
      await documentThumbnail.render({ hash, mimeType, outputPath });
    }
    return Boolean(await resolveThumbMeta(hash));
  }

  async function regenerateThumbnail(hash) {
    if (!isValidHash(hash)) throw new Error('Invalid thumbnail hash');
    const previous = await resolveThumbMeta(hash);
    const backupPath = previous
      ? `${previous.filePath}.${process.pid}.${Date.now()}.regenerate-backup`
      : null;

    if (previous && backupPath) await fs.rename(previous.filePath, backupPath);
    invalidateThumbnail(hash);
    try {
      await renderThumbnailNow(hash);
      const regenerated = await resolveThumbMeta(hash);
      if (!regenerated) throw new Error('No thumbnail renderer supports this file.');
      if (backupPath) await fs.rm(backupPath, { force: true });
      return regenerated;
    } catch (error) {
      const partial = await resolveThumbMeta(hash);
      if (partial) await fs.rm(partial.filePath, { force: true });
      if (previous && backupPath) await fs.rename(backupPath, previous.filePath);
      invalidateThumbnail(hash);
      throw error;
    }
  }

  async function registerMediaProtocol() {
    protocol.handle('media', async (request) => {
      syncCachesForCurrentRoot();
      const parsed = parseMediaUrl(request.url);
      if (!parsed || (parsed.kind !== 'library-cover' && !isValidHash(parsed.hash))) {
        forwardWarn('media', `Failed to parse: ${request.url}`);
        return new Response('Invalid media URL', {
          status: 400,
          headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
        });
      }

      const requestedLibraryRoot = parsed.kind === 'library-cover' ? parsed.libraryRoot : null;
      if (requestedLibraryRoot) {
        const knownRoots = new Set([
          getCurrentLibraryRoot(),
          ...getKnownLibraryRoots(),
        ].filter(Boolean));
        if (!knownRoots.has(requestedLibraryRoot)) {
          return new Response('Unknown library', {
            status: 403,
            headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
          });
        }
      }

      const original = parsed.kind === 'thumb'
        ? await resolveOriginalMeta(parsed.hash, 'bin')
        : null;
      const meta = parsed.kind === 'library-cover'
        ? await resolveLibraryCoverMeta(requestedLibraryRoot)
        : parsed.kind === 'thumb'
          ? (original ? await resolveThumbMeta(parsed.hash) : null)
          : await resolveOriginalMeta(parsed.hash, parsed.ext);

      if (!meta && parsed.kind === 'thumb' && original) {
        scheduleThumbnailAfterMiss(parsed.hash);
      }

      if (!meta) {
        const missingPath = parsed.kind === 'library-cover'
          ? path.join(requestedLibraryRoot, '.picto-library-cover.jpg')
          : parsed.kind === 'thumb'
          ? buildBlobPath(parsed.kind, parsed.hash, 'jpg')
          : buildBlobPath(parsed.kind, parsed.hash, parsed.ext);
        if (parsed.kind === 'file') {
          forwardWarn('media', `404: ${parsed.kind} ${parsed.hash.slice(0, 12)} ${missingPath}`);
        }
        return new Response('Not found', {
          status: 404,
          headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
        });
      }

      const mime = parsed.kind === 'thumb' || parsed.kind === 'library-cover'
        ? extToMime(meta.actualExt || 'jpg')
        : extToMime(meta.actualExt || parsed.ext);
      const cacheControl = parsed.kind === 'thumb' || parsed.kind === 'library-cover'
        ? 'no-store'
        : 'public, max-age=31536000, immutable';
      const range = parseRange(request.headers.get('range'), meta.size);

      if (!range) {
        const stream = createReadStream(meta.filePath);
        return new Response(Readable.toWeb(stream), {
          status: 200,
          headers: {
            'Content-Type': mime,
            'Content-Length': String(meta.size),
            'Accept-Ranges': 'bytes',
            'Cache-Control': cacheControl,
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
          'Cache-Control': cacheControl,
        },
      });
    });
  }

  return {
    buildBlobPath,
    setThumbnail,
    regenerateThumbnail,
    registerMediaProtocol,
  };
}
