import { describe, expect, it } from 'vitest';
import { shouldShowScrollToTop } from './useScrollToTop';

describe('shouldShowScrollToTop', () => {
  it('stays hidden until the scroll threshold is reached', () => {
    expect(shouldShowScrollToTop(199, 200)).toBe(false);
    expect(shouldShowScrollToTop(200, 200)).toBe(true);
  });

  it('supports the deeper sidebar threshold', () => {
    expect(shouldShowScrollToTop(359, 360)).toBe(false);
    expect(shouldShowScrollToTop(360, 360)).toBe(true);
  });
});
