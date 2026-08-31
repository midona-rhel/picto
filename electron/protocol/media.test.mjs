import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { createMediaProtocolService, extToMime, isValidHash, parseMediaUrl, parseRange } from './media.mjs';

describe('media protocol helpers', () => {
  it('parses valid media URLs', () => {
    const hash = 'a'.repeat(64);
    expect(parseMediaUrl(`media://host/thumb/${hash}.jpg`)).toEqual({
      kind: 'thumb',
      hash,
      ext: 'jpg',
    });
    expect(parseMediaUrl(`media://host/file/${hash}.png`)).toEqual({
      kind: 'file',
      hash,
      ext: 'png',
    });
  });

  it('rejects invalid media URLs', () => {
    expect(parseMediaUrl('media://host/other/x')).toBeNull();
    expect(parseMediaUrl('media://host/file/not-a-hash.png')).toBeNull();
    expect(parseMediaUrl('media://host/thumb/abc.png')).toBeNull();
  });

  it('parses a library cover URL', () => {
    expect(parseMediaUrl(`media://host/library-cover/cover?library=${encodeURIComponent('/Pictures/Archive.library')}`)).toEqual({
      kind: 'library-cover',
      libraryRoot: '/Pictures/Archive.library',
    });
  });

  it('parses byte ranges', () => {
    expect(parseRange('bytes=0-99', 1000)).toEqual({ start: 0, end: 99 });
    expect(parseRange('bytes=100-', 1000)).toEqual({ start: 100, end: 999 });
    expect(parseRange('bytes=-50', 1000)).toEqual({ start: 950, end: 999 });
    expect(parseRange('bytes=2000-2100', 1000)).toBeNull();
  });

  it('maps mime types and validates hashes', () => {
    expect(extToMime('jpg')).toBe('image/jpeg');
    expect(extToMime('mp3')).toBe('audio/mpeg');
    expect(extToMime('m4a')).toBe('audio/mp4');
    expect(extToMime('ogg')).toBe('audio/ogg');
    expect(extToMime('opus')).toBe('audio/opus');
    expect(extToMime('pdf')).toBe('application/pdf');
    expect(extToMime('swf')).toBe('application/x-shockwave-flash');
    expect(extToMime('weird')).toBe('application/octet-stream');
    expect(isValidHash('a'.repeat(64))).toBe(true);
    expect(isValidHash('z'.repeat(64))).toBe(false);
  });

  it('atomically replaces a generated thumbnail with a captured PNG', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-current-frame-'));
    try {
      const service = createMediaProtocolService({
        protocol: { handle() {} },
        path,
        invoke: async () => null,
        isDev: true,
        getCurrentLibraryRoot: () => root,
      });
      const hash = 'a'.repeat(64);
      const directory = path.join(root, 'blobs', 't', 'aa', 'aa');
      await fs.mkdir(directory, { recursive: true });
      await fs.writeFile(path.join(directory, `${hash}.jpg`), 'old');
      const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);

      await service.setThumbnail(hash, png.toString('base64'));

      expect(await fs.readFile(path.join(directory, `${hash}.png`))).toEqual(png);
      await expect(fs.stat(path.join(directory, `${hash}.jpg`))).rejects.toMatchObject({ code: 'ENOENT' });
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('regenerates a missing thumbnail through the format-aware fallback', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-regenerate-thumb-'));
    const hash = 'b'.repeat(64);
    const originalDirectory = path.join(root, 'blobs', 'f', 'bb', 'bb');
    const thumbnailPath = path.join(root, 'blobs', 't', 'bb', 'bb', `${hash}.png`);
    try {
      await fs.mkdir(originalDirectory, { recursive: true });
      await fs.writeFile(path.join(originalDirectory, `${hash}.txt`), 'hello');
      const service = createMediaProtocolService({
        protocol: { handle() {} },
        path,
        invoke: async () => { throw new Error('No core thumbnail backend'); },
        isDev: true,
        getCurrentLibraryRoot: () => root,
        documentThumbnail: {
          async render({ outputPath }) {
            await fs.mkdir(path.dirname(outputPath), { recursive: true });
            await fs.writeFile(outputPath, 'document thumbnail');
          },
        },
      });

      await service.regenerateThumbnail(hash);

      expect(await fs.readFile(thumbnailPath, 'utf8')).toBe('document thumbnail');
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('queues a missing raster thumbnail durably without blocking the response', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-deferred-thumb-'));
    const hash = 'c'.repeat(64);
    let handler;
    let releaseRequest;
    const commands = [];
    try {
      const originalDirectory = path.join(root, 'blobs', 'f', 'cc', 'cc');
      await fs.mkdir(originalDirectory, { recursive: true });
      await fs.writeFile(path.join(originalDirectory, `${hash}.jpg`), 'jpeg');
      const service = createMediaProtocolService({
        protocol: { handle(_scheme, next) { handler = next; } },
        path,
        invoke: async (command, args) => {
          commands.push([command, args]);
          await new Promise((resolve) => { releaseRequest = resolve; });
          return { queued: 1 };
        },
        isDev: true,
        getCurrentLibraryRoot: () => root,
      });
      await service.registerMediaProtocol();

      const response = await handler(new Request(`media://localhost/thumb/${hash}.jpg`));

      expect(response.status).toBe(404);
      expect(commands).toEqual([['media.regenerate_thumbnails', { file_hashes: [hash] }]]);
      releaseRequest();
      const thumbnailDirectory = path.join(root, 'blobs', 't', 'cc', 'cc');
      await fs.mkdir(thumbnailDirectory, { recursive: true });
      await fs.writeFile(path.join(thumbnailDirectory, `${hash}.jpg`), 'thumbnail');
      service.invalidateThumbnail(hash);
      const generated = await handler(new Request(`media://localhost/thumb/${hash}.jpg`));
      expect(generated.status).toBe(200);
      expect(await generated.text()).toBe('thumbnail');
      expect(commands).toEqual([['media.regenerate_thumbnails', { file_hashes: [hash] }]]);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('does not serve a stale thumbnail when the original blob is missing', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-stale-thumb-'));
    const hash = 'd'.repeat(64);
    let handler;
    const commands = [];
    try {
      const thumbnailDirectory = path.join(root, 'blobs', 't', 'dd', 'dd');
      await fs.mkdir(thumbnailDirectory, { recursive: true });
      await fs.writeFile(path.join(thumbnailDirectory, `${hash}.jpg`), 'stale thumbnail');
      const service = createMediaProtocolService({
        protocol: { handle(_scheme, next) { handler = next; } },
        path,
        invoke: async (command) => { commands.push(command); },
        isDev: true,
        getCurrentLibraryRoot: () => root,
      });
      await service.registerMediaProtocol();

      const response = await handler(new Request(`media://localhost/thumb/${hash}.jpg`));

      expect(response.status).toBe(404);
      expect(commands).toEqual(['media.resolve_paths']);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it('serves an allow-listed inactive library root cover without switching libraries', async () => {
    const activeRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-active-library-'));
    const inactiveRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-inactive-library-'));
    let handler;
    try {
      await fs.writeFile(path.join(inactiveRoot, '.picto-library-cover.jpg'), 'inactive cover');
      const service = createMediaProtocolService({
        protocol: { handle(_scheme, next) { handler = next; } },
        path,
        invoke: async () => null,
        isDev: true,
        getCurrentLibraryRoot: () => activeRoot,
        getKnownLibraryRoots: () => [inactiveRoot],
      });
      await service.registerMediaProtocol();

      const allowed = await handler(new Request(`media://localhost/library-cover/cover?library=${encodeURIComponent(inactiveRoot)}`));
      const denied = await handler(new Request(`media://localhost/library-cover/cover?library=${encodeURIComponent('/unknown.library')}`));

      expect(allowed.status).toBe(200);
      expect(await allowed.text()).toBe('inactive cover');
      expect(denied.status).toBe(403);
    } finally {
      await fs.rm(activeRoot, { recursive: true, force: true });
      await fs.rm(inactiveRoot, { recursive: true, force: true });
    }
  });

  it('serves canonical originals and adjacent thumbnails outside the active library root', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-canonical-root-'));
    const sourceRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-canonical-source-'));
    const hash = 'f'.repeat(64);
    let handler;
    try {
      const originalPath = path.join(sourceRoot, 'blobs', 'f', 'ff', 'ff', `${hash}.jpg`);
      const thumbnailPath = path.join(sourceRoot, 'blobs', 't', 'ff', 'ff', `${hash}.jpg`);
      await fs.mkdir(path.dirname(originalPath), { recursive: true });
      await fs.mkdir(path.dirname(thumbnailPath), { recursive: true });
      await fs.writeFile(originalPath, 'canonical original');
      await fs.writeFile(thumbnailPath, 'canonical thumbnail');
      const service = createMediaProtocolService({
        protocol: { handle(_scheme, next) { handler = next; } },
        path,
        invoke: async (command) => command === 'media.resolve_paths'
          ? [{ file_hash: hash, path: originalPath }]
          : null,
        isDev: true,
        getCurrentLibraryRoot: () => root,
      });
      await service.registerMediaProtocol();

      const fileResponse = await handler(new Request(`media://localhost/file/${hash}.jpg`));
      const thumbResponse = await handler(new Request(`media://localhost/thumb/${hash}.jpg`));

      expect(fileResponse.status).toBe(200);
      expect(await fileResponse.text()).toBe('canonical original');
      expect(thumbResponse.status).toBe(200);
      expect(await thumbResponse.text()).toBe('canonical thumbnail');
    } finally {
      await fs.rm(root, { recursive: true, force: true });
      await fs.rm(sourceRoot, { recursive: true, force: true });
    }
  });

  it('stops serving a thumbnail after a cached original is removed', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-removed-original-'));
    const hash = 'e'.repeat(64);
    let handler;
    try {
      const originalDirectory = path.join(root, 'blobs', 'f', 'ee', 'ee');
      const thumbnailDirectory = path.join(root, 'blobs', 't', 'ee', 'ee');
      const originalPath = path.join(originalDirectory, `${hash}.jpg`);
      await fs.mkdir(originalDirectory, { recursive: true });
      await fs.mkdir(thumbnailDirectory, { recursive: true });
      await fs.writeFile(originalPath, 'original');
      await fs.writeFile(path.join(thumbnailDirectory, `${hash}.jpg`), 'thumbnail');
      const service = createMediaProtocolService({
        protocol: { handle(_scheme, next) { handler = next; } },
        path,
        invoke: async () => null,
        isDev: true,
        getCurrentLibraryRoot: () => root,
      });
      await service.registerMediaProtocol();

      expect((await handler(new Request(`media://localhost/thumb/${hash}.jpg`))).status).toBe(200);
      await fs.rm(originalPath);
      expect((await handler(new Request(`media://localhost/thumb/${hash}.jpg`))).status).toBe(404);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

});
