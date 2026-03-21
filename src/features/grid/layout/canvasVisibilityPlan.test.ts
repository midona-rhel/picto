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
      scrollPhase: 'fast',
      scrollDirection: 'forward',
      queueDepth: 0,
      bucketIndex: buildBucketIndex(positions),
    });

    expect(plan.visibleIndices).toEqual([3]);
    expect(plan.visibleIterEnd).toBe(1);
  });

  it('does not emit prefetch indices while scrolling', () => {
    const positions = [
      { x: 0, y: 0, w: 100, h: 120 },
      { x: 0, y: 130, w: 100, h: 120 },
      { x: 0, y: 260, w: 100, h: 120 },
      { x: 0, y: 390, w: 100, h: 120 },
    ];

    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop: 0,
      viewportHeight: 150,
      scrollPhase: 'fast',
      scrollDirection: 'forward',
      queueDepth: 0,
    });

    expect(plan.prefetchIndices).toEqual([]);
  });

  it('keeps a larger forward-biased cancel window during fast scrolling', () => {
    const positions = [
      { x: 0, y: 0, w: 100, h: 120 },
      { x: 0, y: 130, w: 100, h: 120 },
      { x: 0, y: 260, w: 100, h: 120 },
      { x: 0, y: 390, w: 100, h: 120 },
    ];

    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop: 150,
      viewportHeight: 250,
      scrollPhase: 'fast',
      scrollDirection: 'forward',
      queueDepth: 0,
    });

    expect(plan.cancelTop).toBe(25);
    expect(plan.cancelBottom).toBe(650);
  });

  it('adds a small forward-biased near-ahead window during slow scrolling', () => {
    const positions = Array.from({ length: 16 }, (_, index) => ({
      x: 0,
      y: index * 130,
      w: 100,
      h: 120,
    }));

    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop: 0,
      viewportHeight: 150,
      scrollPhase: 'slow',
      scrollDirection: 'forward',
      queueDepth: 0,
    });

    expect(plan.prefetchIndices.length).toBeGreaterThan(0);
    expect(plan.prefetchIndices.every((index) => index >= plan.endIdx)).toBe(true);
  });
});
