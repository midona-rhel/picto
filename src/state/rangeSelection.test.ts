import { describe, expect, it } from 'vitest';
import { addSpan, mergeSpans, rangeCount, reconcileRange, toggleInRange, type RangeSpan } from './rangeSelection';

const span = (from: [number, number], to: [number, number]): RangeSpan => ({
  anchor: { id: from[0], index: from[1] },
  focus: { id: to[0], index: to[1] },
});

describe('range selection', () => {
  it('merges overlapping and touching spans but keeps separate ones apart', () => {
    expect(mergeSpans([span([9, 90], [5, 50]), span([1, 10], [2, 20]), span([3, 21], [4, 30])])).toEqual([
      span([1, 10], [4, 30]),
      span([5, 50], [9, 90]),
    ]);
  });

  it('counts spans, additions and exclusions without double counting', () => {
    const range = { spans: [span([1, 0], [2, 9]), span([3, 20], [4, 29])], includedItemIds: new Set([99]) };
    expect(rangeCount(range, new Set([5]))).toBe(20 - 1 + 1);
  });

  it('toggles inside a span as an exclusion and outside as an addition', () => {
    const range = { spans: [span([1, 0], [2, 9])], includedItemIds: new Set<number>() };
    const inside = toggleInRange(range, new Set(), 7, 5);
    expect([...inside.excluded]).toEqual([7]);
    const outside = toggleInRange(inside.range, inside.excluded, 8, 50);
    expect([...outside.range.includedItemIds]).toEqual([8]);
    const back = toggleInRange(outside.range, outside.excluded, 8, 50);
    expect(back.range.includedItemIds.size).toBe(0);
  });

  it('absorbs earlier selections a new span covers and keeps the rest', () => {
    const added = addSpan([], new Set([1, 2, 3]), new Set([4]), span([10, 100], [20, 200]),
      new Map([[1, 150], [2, 5], [4, 120]]));
    expect([...added.range.includedItemIds].sort()).toEqual([2, 3]);
    expect(added.excluded.size).toBe(0);
  });

  it('keeps roots whose position is unknown and drops those that left the view', () => {
    const range = { spans: [span([1, 0], [2, 9])], includedItemIds: new Set([30, 31]) };
    const next = reconcileRange(range, new Set([5, 6]), new Map([[30, null], [31, 4], [5, 3], [6, 40]]));
    // 31 moved inside the span (now covered), 30 left the view; 6 moved outside the span.
    expect(next.range.includedItemIds.size).toBe(0);
    expect([...next.excluded]).toEqual([5]);
    expect(next.droppedSpans).toBe(0);
  });
});
