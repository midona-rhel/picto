import { describe, expect, it } from 'vitest';
import { platformFamily, uiFontStack } from './platform';

describe('platform UI font policy', () => {
  it('classifies the supported desktop platform families', () => {
    expect(platformFamily('MacIntel')).toBe('mac');
    expect(platformFamily('Win32')).toBe('windows');
    expect(platformFamily('Linux x86_64')).toBe('linux');
    expect(platformFamily('', 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')).toBe('mac');
  });

  it('uses the native Apple UI face only on macOS', () => {
    expect(uiFontStack('mac')).toContain('"SF Pro Text", -apple-system');
    expect(uiFontStack('windows')).toMatch(/^"Geist", system-ui/);
    expect(uiFontStack('linux')).toMatch(/^"Geist", system-ui/);
    expect(uiFontStack('windows')).not.toContain('Roboto');
    expect(uiFontStack('linux')).not.toContain('Roboto');
  });
});
