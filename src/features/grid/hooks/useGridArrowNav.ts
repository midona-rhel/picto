import { useEffect, useRef, type RefObject } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridSelection, GridSelectionAction } from '../../../state/selection';
import { GRID_GAP } from '../gridAppearance';
import type { LayoutResult } from '../layout/types';

const NAV_KEY: Record<string, 'left' | 'right' | 'up' | 'down' | 'first' | 'last' | 'pageUp' | 'pageDown'> = {
  ArrowLeft: 'left', a: 'left', A: 'left',
  ArrowRight: 'right', d: 'right', D: 'right',
  ArrowUp: 'up', w: 'up', W: 'up',
  ArrowDown: 'down', s: 'down', S: 'down',
  Home: 'first', End: 'last', PageUp: 'pageUp', PageDown: 'pageDown',
};

interface GridArrowNavOptions {
  items: CanonicalEntityGridItem[];
  layoutRef: RefObject<LayoutResult | null>;
  containerRef: RefObject<HTMLDivElement | null>;
  selectedHashes: Set<string>;
  selection: GridSelection;
  dispatchSelection: (action: GridSelectionAction) => void;
  viewerOpen: boolean;
  containerWidth: number;
  targetSize: number;
}

export function useGridArrowNav(options: GridArrowNavOptions) {
  const optionsRef = useRef(options);
  optionsRef.current = options;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const action = NAV_KEY[event.key];
      if (!action || event.metaKey || event.ctrlKey || event.altKey) return;
      const { items, layoutRef, containerRef, selectedHashes, selection, dispatchSelection,
        viewerOpen, containerWidth, targetSize } = optionsRef.current;
      if (viewerOpen || !items.length || ['INPUT', 'TEXTAREA', 'SELECT'].includes((event.target as HTMLElement)?.tagName)) return;
      const layout = layoutRef.current;
      if (!layout?.positions.length) return;
      event.preventDefault();

      const size = Math.max(50, Math.round(targetSize / 50) * 50);
      const columns = Math.max(1, Math.round((containerWidth - GRID_GAP) / (size + GRID_GAP)));
      const container = containerRef.current;
      const rows = container ? Math.max(1, Math.floor(container.clientHeight / (size + GRID_GAP))) : 5;
      let current = selection.anchor?.kind === 'entity'
        ? items.findIndex((item) => item.entity_hash === selection.anchor!.id)
        : items.findIndex((item) => selectedHashes.has(item.entity_hash));
      if (current < 0) current = 0;

      const deltas = { left: -1, right: 1, up: -columns, down: columns, pageUp: -columns * rows, pageDown: columns * rows };
      const target = action === 'first' ? 0 : action === 'last' ? items.length - 1
        : Math.max(0, Math.min(items.length - 1, current + deltas[action]));
      if (target === current) return;
      const hash = items[target]?.entity_hash;
      if (!hash) return;

      if (event.shiftKey) {
        const anchorIndex = selection.anchor?.kind === 'entity'
          ? items.findIndex((item) => item.entity_hash === selection.anchor!.id) : current;
        const [from, to] = [Math.min(Math.max(anchorIndex, 0), target), Math.max(Math.max(anchorIndex, 0), target)];
        dispatchSelection({ type: 'range_entities', hashes: new Set(items.slice(from, to + 1).map((item) => item.entity_hash)) });
      } else {
        dispatchSelection({ type: 'replace_entities', hashes: new Set([hash]), anchor: hash });
      }

      const position = layout.positions[target];
      if (!container || !position) return;
      const bottom = container.scrollTop + container.clientHeight;
      if (position.y < container.scrollTop + GRID_GAP) container.scrollTop = position.y - GRID_GAP;
      else if (position.y + position.h > bottom - GRID_GAP) {
        container.scrollTop = position.y + position.h - container.clientHeight + GRID_GAP;
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
