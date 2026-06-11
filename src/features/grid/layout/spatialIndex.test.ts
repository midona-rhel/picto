import { describe, expect, it } from 'vitest';
import { buildTileSpatialIndex } from './spatialIndex';
import { computeLayout } from './layoutMath';
import type { GridViewMode, LayoutItem } from './types';

/** Deterministic PRNG so failures reproduce. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function bruteForceYRange(positions: ReadonlyArray<LayoutItem | undefined>, top: number, bottom: number): number[] {
  const result: number[] = [];
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (!pos) continue;
    if (pos.y + pos.h >= top && pos.y <= bottom) result.push(i);
  }
  return result;
}

/** Filter index candidates with the same precise test the consumers use. */
function preciseFilter(positions: ReadonlyArray<LayoutItem | undefined>, candidates: number[], top: number, bottom: number): number[] {
  return candidates.filter((i) => {
    const pos = positions[i];
    return pos != null && pos.y + pos.h >= top && pos.y <= bottom;
  });
}

describe('buildTileSpatialIndex', () => {
  const modes: GridViewMode[] = ['waterfall', 'grid', 'justified'];

  it('matches brute force across layout modes, sizes, and random bands', () => {
    const rand = mulberry32(7);
    for (const mode of modes) {
      for (const count of [0, 1, 500]) {
        const aspectRatios = Array.from({ length: count }, () => 0.4 + rand() * 2.2);
        const layout = computeLayout(aspectRatios, 1200, 240, 16, mode, 20);
        const index = buildTileSpatialIndex(layout.positions);
        const maxY = layout.totalHeight + 500;
        const bands: Array<[number, number]> = [
          [-2000, -1000],                      // entirely above content
          [maxY + 1000, maxY + 2000],          // entirely below content
          [-100, maxY],                        // full extent
          [0, 0],                              // zero-height band
        ];
        for (let q = 0; q < 30; q++) {
          const top = rand() * maxY - 200;
          bands.push([top, top + rand() * 1200]);
        }
        for (const [top, bottom] of bands) {
          const candidates = index.queryYRange(top, bottom, []);
          // No duplicates, ascending order
          for (let k = 1; k < candidates.length; k++) {
            expect(candidates[k]).toBeGreaterThan(candidates[k - 1]);
          }
          // Precise-filtered candidates equal brute force
          expect(preciseFilter(layout.positions, candidates, top, bottom))
            .toEqual(bruteForceYRange(layout.positions, top, bottom));
        }
      }
    }
  });

  it('returns nothing for empty positions', () => {
    const index = buildTileSpatialIndex([]);
    expect(index.queryYRange(0, 1000, [])).toEqual([]);
  });

  it('appends to the provided output buffer and returns it', () => {
    const positions: LayoutItem[] = [{ x: 0, y: 0, w: 100, h: 100 }];
    const index = buildTileSpatialIndex(positions);
    const out: number[] = [];
    expect(index.queryYRange(0, 50, out)).toBe(out);
    expect(out).toEqual([0]);
  });

  it('skips sparse (undefined) position slots', () => {
    const positions: Array<LayoutItem | undefined> = [
      { x: 0, y: 0, w: 100, h: 100 },
      undefined,
      { x: 0, y: 300, w: 100, h: 100 },
    ];
    const index = buildTileSpatialIndex(positions);
    expect(index.queryYRange(0, 500, [])).toEqual([0, 2]);
  });

  it('dedupes tiles spanning multiple bins', () => {
    // 1000px tall tile spans ~4 bins of 256px
    const positions: LayoutItem[] = [{ x: 0, y: 0, w: 100, h: 1000 }];
    const index = buildTileSpatialIndex(positions);
    expect(index.queryYRange(0, 1000, [])).toEqual([0]);
  });
});
