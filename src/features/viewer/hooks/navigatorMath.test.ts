import { describe, expect, it } from 'vitest';
import { computeNavigatorRect } from './navigatorMath';

describe('computeNavigatorRect', () => {
  it('ignores fractional fit overflow caused by native window rounding', () => {
    expect(computeNavigatorRect(
      { scale: 1, tx: 0, ty: 0 },
      { width: 1002.5, height: 752.5 },
      { w: 1000, h: 750 },
    )).toBeNull();
  });

  it('shows the navigator for meaningful overflow', () => {
    expect(computeNavigatorRect(
      { scale: 1, tx: 0, ty: 0 },
      { width: 1004, height: 754 },
      { w: 1000, h: 750 },
    )).not.toBeNull();
  });
});
