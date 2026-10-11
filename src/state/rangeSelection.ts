/**
 * Shift-click selections over a view the renderer only partly holds.
 *
 * A range selection is the union of spans in the view's order, plus roots
 * added one by one outside them, minus roots toggled off inside them. The
 * backend resolves it (`kind: 'range'` targets); the renderer keeps the same
 * three sets so its highlights and count match what an operation changes.
 * Invariants: spans never overlap, excluded roots lie inside a span, added
 * roots lie outside every span. Positions that drift after an edit are
 * brought back in line by `reconcileRange` from backend positions.
 */

/** One end of a span: the root and where the view shows it. */
export interface RangeEnd {
  id: number;
  index: number;
}

export interface RangeSpan {
  anchor: RangeEnd;
  focus: RangeEnd;
}

export interface RangeSelection {
  spans: RangeSpan[];
  includedItemIds: Set<number>;
}

/** Backend positions by root id: a number, or null when the view lost it. */
export type RootPositions = ReadonlyMap<number, number | null>;

function bounds(span: RangeSpan): [RangeEnd, RangeEnd] {
  return span.anchor.index <= span.focus.index ? [span.anchor, span.focus] : [span.focus, span.anchor];
}

export function spansContain(spans: readonly RangeSpan[], index: number): boolean {
  return spans.some((span) => {
    const [low, high] = bounds(span);
    return low.index <= index && index <= high.index;
  });
}

/** Sorted, with overlapping or touching spans merged into one. */
export function mergeSpans(spans: readonly RangeSpan[]): RangeSpan[] {
  const sorted = spans.map(bounds).sort((left, right) => left[0].index - right[0].index);
  const merged: Array<[RangeEnd, RangeEnd]> = [];
  for (const [low, high] of sorted) {
    const last = merged[merged.length - 1];
    if (last && low.index <= last[1].index + 1) {
      if (high.index > last[1].index) last[1] = high;
    } else {
      merged.push([low, high]);
    }
  }
  return merged.map(([low, high]) => ({ anchor: low, focus: high }));
}

export function rangeCount(range: RangeSelection, excluded: ReadonlySet<number>): number {
  const spanned = range.spans.reduce((total, span) => {
    const [low, high] = bounds(span);
    return total + high.index - low.index + 1;
  }, 0);
  return Math.max(0, spanned - excluded.size + range.includedItemIds.size);
}

/** Cmd/Ctrl-click at a known view position. */
export function toggleInRange(
  range: RangeSelection,
  excluded: ReadonlySet<number>,
  itemId: number,
  index: number,
): { range: RangeSelection; excluded: Set<number> } {
  const nextExcluded = new Set(excluded);
  const included = new Set(range.includedItemIds);
  if (spansContain(range.spans, index)) {
    if (!nextExcluded.delete(itemId)) nextExcluded.add(itemId);
  } else if (!included.delete(itemId)) {
    included.add(itemId);
  }
  return { range: { spans: range.spans, includedItemIds: included }, excluded: nextExcluded };
}

/**
 * Add `span` to a selection. `previous` are the roots selected one by one
 * before (an explicit selection or a range's additions); `excluded` the roots
 * toggled off. `positions` tells which of them the new span covers.
 */
export function addSpan(
  spans: readonly RangeSpan[],
  previous: ReadonlySet<number>,
  excluded: ReadonlySet<number>,
  span: RangeSpan,
  positions: RootPositions,
): { range: RangeSelection; excluded: Set<number> } {
  const nextSpans = mergeSpans([...spans, span]);
  const inside = (id: number) => {
    const position = positions.get(id);
    return position != null && spansContain([span], position);
  };
  return {
    range: {
      spans: nextSpans,
      includedItemIds: new Set([...previous].filter((id) => !inside(id))),
    },
    excluded: new Set([...excluded].filter((id) => !inside(id))),
  };
}

/** Every root whose current position a reconcile needs. */
export function rangeRootIds(range: RangeSelection, excluded: ReadonlySet<number>): number[] {
  const ids = new Set<number>();
  for (const span of range.spans) {
    ids.add(span.anchor.id);
    ids.add(span.focus.id);
  }
  range.includedItemIds.forEach((id) => ids.add(id));
  excluded.forEach((id) => ids.add(id));
  return [...ids];
}

/**
 * Move every span end, addition and exclusion to its current position. A
 * span whose end left the view is dropped (`droppedSpans` reports how many),
 * since there is no longer a run of the view it describes; additions and
 * exclusions that left the view, or that now fall on the wrong side of the
 * spans, are dropped too. Roots without a position in `positions` are kept.
 */
export function reconcileRange(
  range: RangeSelection,
  excluded: ReadonlySet<number>,
  positions: RootPositions,
): { range: RangeSelection; excluded: Set<number>; droppedSpans: number } {
  const moved = (end: RangeEnd): RangeEnd | null => {
    if (!positions.has(end.id)) return end;
    const index = positions.get(end.id);
    return index == null ? null : { id: end.id, index };
  };
  let droppedSpans = 0;
  const spans: RangeSpan[] = [];
  for (const span of range.spans) {
    const anchor = moved(span.anchor);
    const focus = moved(span.focus);
    if (anchor && focus) spans.push({ anchor, focus });
    else droppedSpans += 1;
  }
  const merged = mergeSpans(spans);
  const keep = (id: number, insideWanted: boolean) => {
    if (!positions.has(id)) return true;
    const index = positions.get(id);
    return index != null && spansContain(merged, index) === insideWanted;
  };
  return {
    range: {
      spans: merged,
      includedItemIds: new Set([...range.includedItemIds].filter((id) => keep(id, false))),
    },
    excluded: new Set([...excluded].filter((id) => keep(id, true))),
    droppedSpans,
  };
}
