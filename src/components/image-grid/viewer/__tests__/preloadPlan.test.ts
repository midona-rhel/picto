import { describe, it, expect } from 'vitest';
import { buildNeighborDecodePlan } from '../preloadPlan';

function makeItem(hash: string, mime: string) {
  return { hash, mime };
}

describe('buildNeighborDecodePlan', () => {
  it('prioritizes nearest neighbors as high priority', () => {
    const images = [
      makeItem('a', 'image/jpeg'),
      makeItem('b', 'image/jpeg'),
      makeItem('c', 'image/jpeg'),
      makeItem('d', 'image/jpeg'),
      makeItem('e', 'image/jpeg'),
    ];
    const plan = buildNeighborDecodePlan(images, 2);
    expect(plan.thumbs.map((x) => x.hash)).toEqual(['b', 'd', 'a', 'e']);
    expect(plan.thumbs[0]?.priority).toBe('high');
    expect(plan.thumbs[1]?.priority).toBe('high');
    expect(plan.thumbs[2]?.priority).toBe('normal');
  });

  it('skips video neighbors for full decode prefetch', () => {
    const images = [
      makeItem('a', 'image/jpeg'),
      makeItem('b', 'video/mp4'),
      makeItem('c', 'image/jpeg'),
      makeItem('d', 'image/webp'),
      makeItem('e', 'video/webm'),
      makeItem('f', 'image/png'),
    ];
    const plan = buildNeighborDecodePlan(images, 2);
    expect(plan.fulls.map((x) => x.hash)).not.toContain('b');
    expect(plan.fulls.map((x) => x.hash)).not.toContain('e');
  });

  it('caps heavy mime full prefetch tasks', () => {
    const images = [
      makeItem('a', 'image/webp'),
      makeItem('b', 'image/avif'),
      makeItem('c', 'image/jpeg'),
      makeItem('d', 'image/webp'),
      makeItem('e', 'image/avif'),
      makeItem('f', 'image/png'),
      makeItem('g', 'image/jpeg'),
    ];
    const plan = buildNeighborDecodePlan(images, 3);
    const heavy = plan.fulls.filter((x) => x.mime === 'image/webp' || x.mime === 'image/avif');
    expect(heavy.length).toBeLessThanOrEqual(2);
  });
});
