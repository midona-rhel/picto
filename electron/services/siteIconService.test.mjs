import { createHash } from 'node:crypto';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { createSiteIconService } from './siteIconService.mjs';

const PNG = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 0]);

describe('site icon cache', () => {
  it('fetches a catalog domain once and reuses the durable cache', async () => {
    const cacheDirectory = await mkdtemp(path.join(tmpdir(), 'picto-site-icons-'));
    const fetchImpl = vi.fn(async () => new Response(PNG, { headers: { 'content-type': 'image/png' } }));
    const first = createSiteIconService({ cacheDirectory, fetchImpl });

    expect(await first.get('Pixiv.net')).toBe(`data:image/png;base64,${PNG.toString('base64')}`);
    expect(fetchImpl).toHaveBeenCalledOnce();
    expect(fetchImpl).toHaveBeenCalledWith('https://pixiv.net/apple-touch-icon.png', expect.any(Object));

    const secondFetch = vi.fn();
    const second = createSiteIconService({ cacheDirectory, fetchImpl: secondFetch });
    expect(await second.get('pixiv.net')).toBe(`data:image/png;base64,${PNG.toString('base64')}`);
    expect(secondFetch).not.toHaveBeenCalled();
    const cacheKey = createHash('sha256').update('pixiv.net').digest('hex');
    expect(JSON.parse(await readFile(path.join(cacheDirectory, `${cacheKey}.json`), 'utf8'))).toMatchObject({ mime: 'image/png' });
  });

  it('falls back without caching HTML or oversized responses', async () => {
    const cacheDirectory = await mkdtemp(path.join(tmpdir(), 'picto-site-icons-'));
    const html = createSiteIconService({
      cacheDirectory,
      fetchImpl: async () => new Response('<html />', { headers: { 'content-type': 'text/html' } }),
    });
    expect(await html.get('example.com')).toBeNull();

    const oversized = createSiteIconService({
      cacheDirectory,
      fetchImpl: async () => new Response(PNG, { headers: { 'content-length': String(513 * 1024), 'content-type': 'image/png' } }),
    });
    expect(await oversized.get('large.example.com')).toBeNull();
  });

  it('falls back to the conventional favicon when no touch icon exists', async () => {
    const cacheDirectory = await mkdtemp(path.join(tmpdir(), 'picto-site-icons-'));
    const fetchImpl = vi.fn(async (url) => url.endsWith('/apple-touch-icon.png')
      ? new Response('', { status: 404 })
      : new Response(PNG, { headers: { 'content-type': 'image/png' } }));
    const service = createSiteIconService({ cacheDirectory, fetchImpl });

    expect(await service.get('newgrounds.com')).toBe(`data:image/png;base64,${PNG.toString('base64')}`);
    expect(fetchImpl.mock.calls.map(([url]) => url)).toEqual([
      'https://newgrounds.com/apple-touch-icon.png',
      'https://newgrounds.com/favicon.ico',
    ]);
  });

  it('rejects paths and malformed domains', () => {
    const service = createSiteIconService({ cacheDirectory: '/tmp/unused', fetchImpl: vi.fn() });
    expect(() => service.get('example.com/path')).toThrow('Invalid site icon domain');
    expect(() => service.get('example..com')).toThrow('Invalid site icon domain');
  });
});
