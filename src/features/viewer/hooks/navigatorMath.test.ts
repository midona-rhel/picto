import { describe, expect, it } from 'vitest';

import { computeNavigatorRect } from './navigatorMath';

describe('computeNavigatorRect', () => {
  it('tracks bottom-right pan instead of collapsing toward center', () => {
    const rect = computeNavigatorRect(
      { scale: 1, tx: -250, ty: -350 },
      { width: 1000, height: 1000 },
      { w: 500, h: 300 },
    );

    expect(rect).not.toBeNull();
    expect(rect!.x).toBeCloseTo(0.5, 3);
    expect(rect!.y).toBeCloseTo(0.7, 3);
    expect(rect!.w).toBeCloseTo(0.5, 3);
    expect(rect!.h).toBeCloseTo(0.3, 3);
  });
});
