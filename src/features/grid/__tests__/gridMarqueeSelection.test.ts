import { describe, expect, it } from 'vitest';

describe('marquee selection', () => {
  it('AABB intersection logic detects overlapping tiles', () => {
    // Simple AABB test matching the inline logic in useGridMarqueeSelection
    const tiles = [
      { x: 0, y: 0, w: 100, h: 100, hash: 'a' },
      { x: 200, y: 0, w: 100, h: 100, hash: 'b' },
      { x: 0, y: 400, w: 100, h: 100, hash: 'c' },
    ];

    const testRect = (left: number, top: number, right: number, bottom: number) => {
      const hits: string[] = [];
      for (const t of tiles) {
        if (t.x + t.w > left && t.x < right && t.y + t.h > top && t.y < bottom) {
          hits.push(t.hash);
        }
      }
      return hits;
    };

    expect(testRect(-10, -10, 150, 150)).toEqual(['a']);
    expect(testRect(-10, -10, 400, 600)).toEqual(['a', 'b', 'c']);
    expect(testRect(500, 500, 600, 600)).toEqual([]);
  });
});
