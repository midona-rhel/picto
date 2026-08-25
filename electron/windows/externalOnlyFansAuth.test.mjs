import { describe, expect, it } from 'vitest';
import { createManualOnlyFansCredential, parseCookieHeader } from './externalOnlyFansAuth.mjs';

describe('OnlyFans external authentication', () => {
  it('keeps only the session cookies required by the runner', () => {
    expect(parseCookieHeader('analytics=x; sess=session; auth_id=42; auth_uid=42')).toEqual({
      sess: 'session',
      auth_id: '42',
      auth_uid: '42',
    });
  });

  it('requires the complete manual session contract', () => {
    expect(() => createManualOnlyFansCredential({
      cookie: 'sess=session; auth_id=42',
      user_agent: 'Chrome',
      x_bc: '',
    })).toThrow('OnlyFans X-BC is required.');

    expect(createManualOnlyFansCredential({
      cookie: 'sess=session; auth_id=42',
      user_agent: 'Chrome',
      x_bc: 'signature',
    })).toMatchObject({
      cookies: { sess: 'session', auth_id: '42' },
      headers: { 'user-agent': 'Chrome', 'x-bc': 'signature' },
    });
  });
});
