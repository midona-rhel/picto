import { describe, expect, it } from 'vitest';
import {
  BUCKET_SIZE,
  buildBucketIndexEntries,
  computeLayoutFromAspectRatios,
} from '../layoutMath';

describe('layoutMath', () => {
  it('computes deterministic waterfall layout with padding', () => {
    const layout = computeLayoutFromAspectRatios(
      [1, 2, 0.5, 1.5],
      600,
      180,
      12,
      'waterfall',
      24,
      16,
    );

    expect(layout.positions).toHaveLength(4);
    expect(layout.positions[0]?.x).toBeGreaterThanOrEqual(16);
    expect(layout.positions[0]?.y).toBe(2);
    expect(layout.totalHeight).toBeGreaterThan(0);
  });

  it('computes deterministic grid total height', () => {
    const layout = computeLayoutFromAspectRatios(
      [1, 1, 1, 1, 1],
      420,
      120,
      10,
      'grid',
      20,
      0,
    );

    expect(layout.positions).toHaveLength(5);
    expect(layout.positions[0]).toMatchObject({ x: 0, y: 2 });
    expect(layout.totalHeight).toBeGreaterThan(layout.positions[4].y);
  });

  it('builds bucket index entries for visible lookup windows', () => {
    const layout = computeLayoutFromAspectRatios(
      new Array(12).fill(1),
      480,
      140,
      8,
      'waterfall',
      20,
      8,
    );
    const entries = buildBucketIndexEntries(layout.positions);

    expect(entries.length).toBeGreaterThan(0);
    expect(entries[0].bucket).toBe(Math.floor(layout.positions[0].y / BUCKET_SIZE));
    expect(entries.some((entry) => entry.indices.includes(0))).toBe(true);
  });
});
