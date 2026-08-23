/**
 * useGridArrowNav — arrow key + WASD + Home/End/PageUp/PageDown navigation for the image grid.
 *
 * ArrowLeft/A = −1, ArrowRight/D = +1, ArrowUp/W = −columnCount, ArrowDown/S = +columnCount.
 * Home = first image, End = last image.
 * PageUp/PageDown = ±visible rows.
 * Shift+key extends range selection from anchor.
 * Scrolls the target item into view.
 */

import { useEffect, useRef, type RefObject } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { LayoutResult } from '../layout/types';
import type { GridSelection, GridSelectionAction } from '../../../state/selection';

const GAP = 16;

/** Map WASD to arrow equivalents so both sets work. */
const WASD_MAP: Record<string, string> = {
  w: 'ArrowUp', a: 'ArrowLeft', s: 'ArrowDown', d: 'ArrowRight',
  W: 'ArrowUp', A: 'ArrowLeft', S: 'ArrowDown', D: 'ArrowRight',
};

type NavAction = 'left' | 'right' | 'up' | 'down' | 'first' | 'last' | 'pageUp' | 'pageDown';

const KEY_TO_ACTION: Record<string, NavAction> = {
  ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down',
  Home: 'first', End: 'last', PageUp: 'pageUp', PageDown: 'pageDown',
};

export function useGridArrowNav(opts: {
  items: CanonicalEntityGridItem[];
  layoutRef: RefObject<LayoutResult | null>;
  containerRef: RefObject<HTMLDivElement | null>;
  selectedItemIds: Set<number>;
  selection: GridSelection;
  dispatchSelection: (action: GridSelectionAction) => void;
  viewerOpen: boolean;
  containerWidth: number;
  targetSize: number;
}) {
  const optsRef = useRef(opts);
  optsRef.current = opts;

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      const { items, layoutRef, containerRef, selectedItemIds, selection, dispatchSelection, viewerOpen, containerWidth, targetSize } = optsRef.current;

      if (viewerOpen) return;
      if (items.length === 0) return;

      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      // Map WASD to arrow equivalents, then look up action
      const mappedKey = WASD_MAP[e.key] ?? e.key;
      const action = KEY_TO_ACTION[mappedKey];
      if (!action) return;

      e.preventDefault();

      const layout = layoutRef.current;
      if (!layout || layout.positions.length === 0) return;

      // Compute column count (must match computeLayout's formula)
      const snappedSize = Math.max(50, Math.round(targetSize / 50) * 50);
      const fullWidth = containerWidth;
      const minInnerWidth = fullWidth - 2 * GAP;
      const columnCount = Math.max(1, Math.round((minInnerWidth + GAP) / (snappedSize + GAP)));

      // Find current position
      let current = selection.anchor?.kind === 'item'
        ? items.findIndex((item) => item.item_id === selection.anchor!.id)
        : -1;
      if (current == null || current < 0 || current >= items.length) {
        for (let i = 0; i < items.length; i++) {
          if (selectedItemIds.has(items[i].item_id)) { current = i; break; }
        }
        if (current == null) current = 0;
      }

      // Compute visible rows for page up/down
      const container = containerRef.current;
      const visibleRows = container ? Math.max(1, Math.floor(container.clientHeight / (snappedSize + GAP))) : 5;

      let target: number;
      switch (action) {
        case 'left':     target = Math.max(0, current - 1); break;
        case 'right':    target = Math.min(items.length - 1, current + 1); break;
        case 'up':       target = Math.max(0, current - columnCount); break;
        case 'down':     target = Math.min(items.length - 1, current + columnCount); break;
        case 'first':    target = 0; break;
        case 'last':     target = items.length - 1; break;
        case 'pageUp':   target = Math.max(0, current - columnCount * visibleRows); break;
        case 'pageDown': target = Math.min(items.length - 1, current + columnCount * visibleRows); break;
        default: return;
      }

      if (target === current) return;

      if (e.shiftKey) {
        // Range select from anchor to target
        const anchorIndex = selection.anchor?.kind === 'item'
          ? items.findIndex((item) => item.item_id === selection.anchor!.id)
          : current;
        const anchor = anchorIndex >= 0 ? anchorIndex : current;
        const [lo, hi] = [Math.min(anchor, target), Math.max(anchor, target)];
        const next = new Set<number>();
        for (let i = lo; i <= hi; i++) {
          if (items[i]) next.add(items[i].item_id);
        }
        dispatchSelection({ type: 'range_items', itemIds: next });
      } else {
        const itemId = items[target]?.item_id;
        if (itemId != null) dispatchSelection({ type: 'replace_items', itemIds: new Set([itemId]), anchor: itemId });
      }

      // Scroll target into view
      const pos = layout.positions[target];
      if (pos && container) {
        const scrollTop = container.scrollTop;
        const viewportH = container.clientHeight;
        if (pos.y < scrollTop + GAP) {
          container.scrollTop = pos.y - GAP;
        } else if (pos.y + pos.h > scrollTop + viewportH - GAP) {
          container.scrollTop = pos.y + pos.h - viewportH + GAP;
        }
      }
    }

    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, []);
}
