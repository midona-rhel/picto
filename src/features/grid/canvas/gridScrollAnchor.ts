import type { LayoutItem } from '../layout/types';

interface HashItem {
  entity_hash: string;
}

export function resolveGridScrollAnchor(args: {
  previousPositions: LayoutItem[];
  nextPositions: LayoutItem[];
  previousItems: readonly HashItem[];
  nextItems: readonly HashItem[];
  selectedHashes: ReadonlySet<string>;
  scrollTop: number;
  viewportHeight: number;
}): number | null {
  const {
    previousPositions,
    nextPositions,
    previousItems,
    nextItems,
    selectedHashes,
    scrollTop,
    viewportHeight,
  } = args;
  const viewportBottom = scrollTop + viewportHeight;
  let anchorIndex = -1;
  let bestTop = Infinity;

  if (selectedHashes.size > 0) {
    for (let index = 0; index < previousPositions.length; index++) {
      const position = previousPositions[index];
      const item = previousItems[index];
      if (!position || !item || !selectedHashes.has(item.entity_hash)) continue;
      if (position.y + position.h < scrollTop || position.y > viewportBottom) continue;
      if (position.y < bestTop) {
        bestTop = position.y;
        anchorIndex = index;
      }
    }
  }

  if (anchorIndex < 0) {
    bestTop = Infinity;
    for (let index = 0; index < previousPositions.length; index++) {
      const position = previousPositions[index];
      if (!position) continue;
      if (position.y + position.h < scrollTop || position.y > viewportBottom) continue;
      if (position.y < bestTop) {
        bestTop = position.y;
        anchorIndex = index;
      }
    }
  }

  const anchorHash = previousItems[anchorIndex]?.entity_hash;
  if (!anchorHash) return null;
  const nextIndex = nextItems.findIndex((item) => item.entity_hash === anchorHash);
  const previousPosition = previousPositions[anchorIndex];
  const nextPosition = nextPositions[nextIndex];
  if (!previousPosition || !nextPosition) return null;

  const viewportOffset = previousPosition.y - scrollTop;
  return Math.max(0, nextPosition.y - viewportOffset);
}
