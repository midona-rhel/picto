import { describe, expect, it, vi } from 'vitest';
import { verifyAuthRoute } from './verify-auth-routes.mjs';

function response({ status = 200, body = '', url = 'https://example.test/login' } = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    url,
    text: vi.fn().mockResolvedValue(body),
  };
}

describe('verifyAuthRoute', () => {
  it('rejects an empty successful response', async () => {
    const result = await verifyAuthRoute(
      { site: 'empty', loginUrl: 'https://example.test/login' },
      vi.fn().mockResolvedValue(response()),
    );

    expect(result.status).toBe('failed');
  });

  it('accepts a rendered login page', async () => {
    const result = await verifyAuthRoute(
      { site: 'working', loginUrl: 'https://example.test/login' },
      vi.fn().mockResolvedValue(response({ body: '<title>Login</title><form><input type="password"></form>' })),
    );

    expect(result.status).toBe('passed');
  });

  it('leaves bot-gated routes for attended verification', async () => {
    const result = await verifyAuthRoute(
      { site: 'gated', loginUrl: 'https://example.test/login' },
      vi.fn().mockResolvedValue(response({ status: 403 })),
    );

    expect(result.status).toBe('manual');
  });
});
