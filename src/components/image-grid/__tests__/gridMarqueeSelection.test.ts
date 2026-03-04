import { describe, expect, it } from 'vitest';

import {
  MARQUEE_BUCKET_SIZE,
  buildMarqueeTileCache,
  collectMarqueeHitHashes,
} from '../hooks/useGridMarqueeSelection';

describe('marquee selection helpers', () => {
  it('indexes tiles into vertical buckets and returns only intersecting hits', () => {
    const positions = [
      { x: 0, y: 0, w: 100, h: 100 },
      { x: 200, y: 0, w: 100, h: 100 },
      { x: 0, y: 400, w: 100, h: 100 },
    ];
    const images = [
      { hash: 'a' },
      { hash: 'b' },
      { hash: 'c' },
    ] as any;

    const { tiles, buckets } = buildMarqueeTileCache(positions as any, images, MARQUEE_BUCKET_SIZE);

    expect(tiles).toHaveLength(3);
    expect(buckets.size).toBeGreaterThan(0);

    const topHits = collectMarqueeHitHashes(
      tiles,
      buckets,
      { left: -10, top: -10, right: 150, bottom: 150 },
      MARQUEE_BUCKET_SIZE,
    );
    expect(topHits).toEqual(['a']);

    const allHits = collectMarqueeHitHashes(
      tiles,
      buckets,
      { left: -10, top: -10, right: 400, bottom: 600 },
      MARQUEE_BUCKET_SIZE,
    );
    expect(allHits).toEqual(['a', 'b', 'c']);
  });
});
