import { describe, expect, it } from 'vitest';
import { estimateGridScrollHeight, GridLayoutRuntime } from './gridLayoutModel';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

function item(hash: string, width = 100, height = 100): CanonicalEntityGridItem {
  return {
    entity_hash: hash,
    name: hash,
    mime_type: 'image/jpeg',
    pixel_width: width,
    pixel_height: height,
  } as CanonicalEntityGridItem;
}

describe('buildGridLayoutModel', () => {
  it('builds render, position, spatial, and hash lookup data atomically', () => {
    const model = new GridLayoutRuntime().update([item('a'), item('b', 200, 100)], {
      width: 500, targetSize: 180, gap: 16, viewMode: 'grid', textHeight: 20, scrollbarWidth: 8,
    });
    expect(model.items.map((entry) => entry.hash)).toEqual(['a', 'b']);
    expect(model.positions).toHaveLength(2);
    expect(model.hashToIndex).toEqual(new Map([['a', 0], ['b', 1]]));
    expect(model.totalHeight).toBeGreaterThan(0);
    expect(model.spatialIndex.queryYRange(0, model.totalHeight, [])).toEqual([0, 1]);
  });

  it('reuses adapted prefix objects only for a true page append', () => {
    const runtime = new GridLayoutRuntime();
    const config = { width: 500, targetSize: 180, gap: 16, viewMode: 'grid' as const, textHeight: 20, scrollbarWidth: 8 };
    const first = [item('a'), item('b')];
    const initial = runtime.update(first, config);
    const appended = runtime.update([...first, item('c')], config);
    expect(appended.items[0]).toBe(initial.items[0]);
    expect(appended.hashToIndex.get('c')).toBe(2);

    const reordered = runtime.update([first[1], first[0], item('c')], config);
    expect(reordered.items[0]).not.toBe(appended.items[1]);
    expect(reordered.hashToIndex).toEqual(new Map([['b', 0], ['a', 1], ['c', 2]]));
  });

  it.each(['grid', 'waterfall', 'justified'] as const)(
    'matches a clean %s layout after an append without mutating prior positions',
    (viewMode) => {
      const config = { width: 937, targetSize: 180, gap: 16, viewMode, textHeight: 20, scrollbarWidth: 8 };
      const first = Array.from({ length: 37 }, (_, index) => item(String(index), 100 + index * 7, 80 + (index % 9) * 13));
      const all = [...first, ...Array.from({ length: 17 }, (_, index) => item(String(37 + index), 180 + index * 11, 90 + index * 5))];
      const runtime = new GridLayoutRuntime();
      const before = runtime.update(first, config);
      const priorPositions = before.positions.map((position) => ({ ...position }));
      const appended = runtime.update(all, config);
      const clean = new GridLayoutRuntime().update(all, config);

      expect(appended.positions).toEqual(clean.positions);
      expect(appended.totalHeight).toBe(clean.totalHeight);
      expect(before.positions).toEqual(priorPositions);
    },
  );
});

describe('estimateGridScrollHeight', () => {
  it('preserves the full result scroll range from loaded layout density', () => {
    expect(estimateGridScrollHeight(2_000, 500, 10_000)).toBe(40_000);
  });

  it('never shrinks below real loaded content', () => {
    expect(estimateGridScrollHeight(2_000, 500, 400)).toBe(2_000);
    expect(estimateGridScrollHeight(2_000, 500, null)).toBe(2_000);
  });
});
