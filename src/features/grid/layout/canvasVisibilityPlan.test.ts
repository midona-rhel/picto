import { describe, expect, it } from 'vitest';

import { buildBucketIndex } from '../layoutMath';
import { buildCanvasVisibilityPlan } from './canvasVisibilityPlan';

describe('buildCanvasVisibilityPlan', () => {
  it('uses bucket index for unsorted waterfall positions', () => {
    const positions = [
      { x: 0, y: 0, w: 100, h: 120 },
      { x: 100, y: 0, w: 100, h: 120 },
      { x: 0, y: 260, w: 100, h: 120 },
      { x: 100, y: 130, w: 100, h: 120 },
    ];

    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop: 121,
      viewportHeight: 80,
      isScrolling: true,
      queueDepth: 0,
      bucketIndex: buildBucketIndex(positions),
    });

    expect(plan.visibleIndices).toEqual([3]);
    expect(plan.visibleIterEnd).toBe(1);
  });
});
