import { describe, expect, it } from 'vitest';
import {
  computePlanFingerprint,
  sortPlanTilesByViewportDistance,
  type PlanTile,
} from './thumbnailPlan';

const THRESHOLD = 752;

function tile(fileHash: string, overrides: Partial<PlanTile> = {}): PlanTile {
  return { fileHash, mime: 'image/png', w: 200, h: 200, cy: 100, ...overrides };
}

describe('computePlanFingerprint', () => {
  it('is deterministic for the same plan', () => {
    const tiles = [tile('aaaa1111'), tile('bbbb2222')];
    expect(computePlanFingerprint(tiles, THRESHOLD)).toBe(computePlanFingerprint(tiles, THRESHOLD));
  });

  it('always returns an unsigned value distinct from the -1 sentinel', () => {
    expect(computePlanFingerprint([], THRESHOLD)).toBeGreaterThanOrEqual(0);
    expect(computePlanFingerprint([tile('aaaa1111')], THRESHOLD)).toBeGreaterThanOrEqual(0);
  });

  it('changes when a hash changes', () => {
    const a = computePlanFingerprint([tile('aaaa1111'), tile('bbbb2222')], THRESHOLD);
    const b = computePlanFingerprint([tile('aaaa1111'), tile('cccc3333')], THRESHOLD);
    expect(a).not.toBe(b);
  });

  it('changes when order changes', () => {
    const a = computePlanFingerprint([tile('aaaa1111'), tile('bbbb2222')], THRESHOLD);
    const b = computePlanFingerprint([tile('bbbb2222'), tile('aaaa1111')], THRESHOLD);
    expect(a).not.toBe(b);
  });

  it('changes when a tile crosses the quality-tier threshold', () => {
    const small = computePlanFingerprint([tile('aaaa1111', { w: 400 })], THRESHOLD);
    const large = computePlanFingerprint([tile('aaaa1111', { w: 800 })], THRESHOLD);
    expect(small).not.toBe(large);
  });

  it('changes when the count changes', () => {
    const one = computePlanFingerprint([tile('aaaa1111')], THRESHOLD);
    const two = computePlanFingerprint([tile('aaaa1111'), tile('aaaa1111')], THRESHOLD);
    expect(one).not.toBe(two);
  });

  it('distinguishes sets the old approximate fingerprint conflated', () => {
    // Same count, first, middle, and last — different interior tile.
    const base = Array.from({ length: 100 }, (_, i) => tile(`hash${String(i).padStart(4, '0')}`));
    const variant = base.map((t, i) => (i === 25 ? tile('zzzz9999') : t));
    expect(computePlanFingerprint(base, THRESHOLD)).not.toBe(computePlanFingerprint(variant, THRESHOLD));
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
