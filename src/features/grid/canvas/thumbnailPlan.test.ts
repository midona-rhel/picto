import { describe, expect, it } from 'vitest';
import {
  computePlanFingerprint,
  fullQualityDecodeSize,
  shouldLoadFullQualityOriginal,
  sortPlanTilesByViewportDistance,
  type PlanTile,
} from './thumbnailPlan';

const THUMBNAIL_LONG_EDGE = 512;

function tile(fileHash: string, overrides: Partial<PlanTile> = {}): PlanTile {
  return { fileHash, mime: 'image/png', w: 200, h: 200, cy: 100, ...overrides };
}

describe('computePlanFingerprint', () => {
  it('is deterministic for the same plan', () => {
    const tiles = [tile('aaaa1111'), tile('bbbb2222')];
    expect(computePlanFingerprint(tiles, THUMBNAIL_LONG_EDGE)).toBe(computePlanFingerprint(tiles, THUMBNAIL_LONG_EDGE));
  });

  it('always returns an unsigned value distinct from the -1 sentinel', () => {
    expect(computePlanFingerprint([], THUMBNAIL_LONG_EDGE)).toBeGreaterThanOrEqual(0);
    expect(computePlanFingerprint([tile('aaaa1111')], THUMBNAIL_LONG_EDGE)).toBeGreaterThanOrEqual(0);
  });

  it('changes when a hash changes', () => {
    const a = computePlanFingerprint([tile('aaaa1111'), tile('bbbb2222')], THUMBNAIL_LONG_EDGE);
    const b = computePlanFingerprint([tile('aaaa1111'), tile('cccc3333')], THUMBNAIL_LONG_EDGE);
    expect(a).not.toBe(b);
  });

  it('changes when order changes', () => {
    const a = computePlanFingerprint([tile('aaaa1111'), tile('bbbb2222')], THUMBNAIL_LONG_EDGE);
    const b = computePlanFingerprint([tile('bbbb2222'), tile('aaaa1111')], THUMBNAIL_LONG_EDGE);
    expect(a).not.toBe(b);
  });

  it('changes when a tile crosses the quality-tier threshold', () => {
    const small = computePlanFingerprint([tile('aaaa1111', { w: 400 })], THUMBNAIL_LONG_EDGE);
    const large = computePlanFingerprint([tile('aaaa1111', { w: 800 })], THUMBNAIL_LONG_EDGE);
    expect(small).not.toBe(large);
  });

  it('changes when an already-promoted tile needs a larger decode', () => {
    const first = computePlanFingerprint([tile('aaaa1111', { w: 600 })], THUMBNAIL_LONG_EDGE);
    const larger = computePlanFingerprint([tile('aaaa1111', { w: 700 })], THUMBNAIL_LONG_EDGE);
    expect(first).not.toBe(larger);
  });

  it('changes when the count changes', () => {
    const one = computePlanFingerprint([tile('aaaa1111')], THUMBNAIL_LONG_EDGE);
    const two = computePlanFingerprint([tile('aaaa1111'), tile('aaaa1111')], THUMBNAIL_LONG_EDGE);
    expect(one).not.toBe(two);
  });

  it('distinguishes sets the old approximate fingerprint conflated', () => {
    // Same count, first, middle, and last — different interior tile.
    const base = Array.from({ length: 100 }, (_, i) => tile(`hash${String(i).padStart(4, '0')}`));
    const variant = base.map((t, i) => (i === 25 ? tile('zzzz9999') : t));
    expect(computePlanFingerprint(base, THUMBNAIL_LONG_EDGE)).not.toBe(computePlanFingerprint(variant, THUMBNAIL_LONG_EDGE));
  });
});

describe('shouldLoadFullQualityOriginal', () => {
  it('uses large browser-decodable image originals', () => {
    expect(shouldLoadFullQualityOriginal(tile('png', { w: 513 }), THUMBNAIL_LONG_EDGE)).toBe(true);
  });

  it('uses device pixels when deciding whether a thumbnail would be upscaled', () => {
    expect(shouldLoadFullQualityOriginal(
      tile('retina', { w: 300, h: 200 }),
      THUMBNAIL_LONG_EDGE,
      2,
    )).toBe(true);
    expect(shouldLoadFullQualityOriginal(
      tile('retina-small', { w: 250, h: 200 }),
      THUMBNAIL_LONG_EDGE,
      2,
    )).toBe(false);
  });

  it('sizes full-quality decodes to the physical grid footprint', () => {
    expect(fullQualityDecodeSize(tile('portrait', {
      w: 474,
      h: 723,
      sourceWidth: 2755,
      sourceHeight: 4200,
      fit: 'cover',
    }))).toEqual({ width: 475, height: 723 });
    expect(fullQualityDecodeSize(tile('portrait-retina', {
      w: 474,
      h: 723,
      sourceWidth: 2755,
      sourceHeight: 4200,
      fit: 'cover',
    }), 2)).toEqual({ width: 949, height: 1446 });
  });

  it('keeps JPEG XL on its generated raster thumbnail at every size', () => {
    expect(shouldLoadFullQualityOriginal(
      tile('jxl', { mime: 'image/jxl', w: 1200, h: 1200 }),
      THUMBNAIL_LONG_EDGE,
    )).toBe(false);
  });
});

describe('sortPlanTilesByViewportDistance', () => {
  it('orders tiles nearest the viewport center first', () => {
    const tiles = [tile('far', { cy: 2000 }), tile('near', { cy: 510 }), tile('mid', { cy: 900 })];
    sortPlanTilesByViewportDistance(tiles, 500);
    expect(tiles.map((t) => t.fileHash)).toEqual(['near', 'mid', 'far']);
  });

  it('keeps input order on distance ties', () => {
    const tiles = [tile('above', { cy: 400 }), tile('below', { cy: 600 })];
    sortPlanTilesByViewportDistance(tiles, 500);
    expect(tiles.map((t) => t.fileHash)).toEqual(['above', 'below']);
  });

  it('sorts in place', () => {
    const tiles = [tile('b', { cy: 900 }), tile('a', { cy: 100 })];
    const ref = tiles;
    sortPlanTilesByViewportDistance(tiles, 0);
    expect(ref).toBe(tiles);
    expect(tiles[0].fileHash).toBe('a');
  });

  it('handles an empty array', () => {
    const tiles: PlanTile[] = [];
    sortPlanTilesByViewportDistance(tiles, 500);
    expect(tiles).toEqual([]);
  });
});
