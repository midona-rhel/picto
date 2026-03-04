import { describe, expect, it } from 'vitest';
import { shouldInvalidateGridScope } from '../eventBridge';

describe('eventBridge grid scope matching', () => {
  it('preserves legacy wildcard behavior for system-only invalidations', () => {
    expect(shouldInvalidateGridScope('folder:12', ['system:all'])).toBe(true);
    expect(shouldInvalidateGridScope('smart:7', ['system:all'])).toBe(true);
  });

  it('treats system:all as literal when targeted non-system scopes are present', () => {
    const scopes = ['system:all', 'system:inbox', 'system:trash', 'smart:all', 'folder:12'];
    expect(shouldInvalidateGridScope('folder:12', scopes)).toBe(true);
    expect(shouldInvalidateGridScope('folder:99', scopes)).toBe(false);
    expect(shouldInvalidateGridScope('system:all', scopes)).toBe(true);
  });

  it('supports explicit smart and folder wildcard scopes', () => {
    expect(shouldInvalidateGridScope('smart:5', ['smart:all'])).toBe(true);
    expect(shouldInvalidateGridScope('folder:2', ['folder:all'])).toBe(true);
    expect(shouldInvalidateGridScope('folder:2', ['smart:all'])).toBe(false);
  });
});
