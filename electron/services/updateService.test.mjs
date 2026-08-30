import { describe, expect, test, vi } from 'vitest';
import { createUpdateService } from './updateService.mjs';

function app(packaged = true) {
  return { isPackaged: packaged, getVersion: () => '0.6.0-alpha' };
}

describe('update service', () => {
  test('does not contact update servers in development', async () => {
    const fetch = vi.fn();
    const service = createUpdateService({ app: app(false), net: { fetch }, sendToAllWindows: vi.fn(), platform: 'darwin' });
    expect((await service.check()).status).toBe('unavailable');
    expect(fetch).not.toHaveBeenCalled();
  });

  test('finds prerelease updates on macOS and preserves their release notes', async () => {
    const sendToAllWindows = vi.fn();
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [{
        draft: false,
        tag_name: 'v0.6.1-alpha',
        name: 'Picto 0.6.1 Alpha',
        body: '- Faster imports',
        published_at: '2026-08-30T00:00:00Z',
        html_url: 'https://example.test/release',
      }],
    });
    const service = createUpdateService({ app: app(), net: { fetch }, sendToAllWindows, platform: 'darwin' });
    const state = await service.check();
    expect(state).toMatchObject({ status: 'available', version: '0.6.1-alpha', releaseNotes: '- Faster imports' });
    expect(sendToAllWindows).toHaveBeenLastCalledWith('picto:update-state', expect.objectContaining({ status: 'available' }));
  });

  test('reports the current version when no newer Mac release exists', async () => {
    const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => [{ draft: false, tag_name: 'v0.6.0-alpha' }] });
    const service = createUpdateService({ app: app(), net: { fetch }, sendToAllWindows: vi.fn(), platform: 'darwin' });
    expect((await service.check()).status).toBe('current');
  });
});
