import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useReducer, useRef } from 'react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { resetShortcutRuntimeForTests } from '../../../runtime/shortcutRuntime';
import { emptyGridSelection, reduceGridSelection } from '../../../state/selection';
import { useGridArrowNav } from './useGridArrowNav';

const items = Array.from({ length: 10 }, (_, index): CanonicalEntityGridItem => ({
  root_id: index + 1,
  kind: 'media',
  lifecycle: 'active',
  name: `Item ${index + 1}`,
  cover_media_id: index + 1,
  content_hash: `file-${index + 1}`,
  mime: 'image/jpeg',
  width: 100,
  height: 100,
  duration_ms: null,
  frame_count: null,
  palette: [],
  imported_at_ms: index + 1,
  captured_at_ms: null,
  modified_at_ms: index + 1,
  rating: 'unrated',
  media_count: 1,
  total_size_bytes: 100,
}));

function GridArrowNavHarness({ initiallySelected = true }: { initiallySelected?: boolean }) {
  const [selection, dispatchSelection] = useReducer(
    reduceGridSelection,
    initiallySelected
      ? reduceGridSelection(emptyGridSelection(), {
          type: 'replace_items',
          itemIds: new Set([2]),
          anchor: 2,
        })
      : emptyGridSelection(),
  );
  const layoutRef = useRef({
    positions: items.map((_, index) => ({ x: index * 100, y: 0, w: 100, h: 100 })),
    totalHeight: 100,
  });
  const containerRef = useRef<HTMLDivElement>(null);

  useGridArrowNav({
    items,
    layoutRef,
    containerRef,
    selectedItemIds: selection.itemIds,
    selection,
    dispatchSelection,
    viewerOpen: false,
    containerWidth: 500,
    targetSize: 100,
  });

  return (
    <div ref={containerRef}>
      <button type="button" onClick={() => dispatchSelection({ type: 'toggle_item', itemId: 2 })}>
        Toggle 2
      </button>
      <button type="button" onClick={() => dispatchSelection({ type: 'toggle_item', itemId: 8 })}>
        Toggle 8
      </button>
      <output data-testid="selection">
        {[...selection.itemIds].sort((left, right) => left - right).join(',')}|{selection.anchor?.kind === 'item' ? selection.anchor.id : ''}
      </output>
    </div>
  );
}

describe('useGridArrowNav', () => {
  beforeEach(resetShortcutRuntimeForTests);
  afterEach(() => {
    cleanup();
    resetShortcutRuntimeForTests();
  });

  it('extends a Shift+Arrow range from its moving endpoint and keeps it bounded', () => {
    render(<GridArrowNavHarness />);

    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    expect(screen.getByTestId('selection')).toHaveTextContent('2,3,4,5,6|2');

    fireEvent.keyDown(window, { key: 'ArrowLeft', shiftKey: true });
    expect(screen.getByTestId('selection')).toHaveTextContent('2,3,4,5|2');
  });

  it('starts a keyboard range from the last Ctrl-clicked item, not selected extremes', () => {
    render(<GridArrowNavHarness initiallySelected={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Toggle 2' }), { ctrlKey: true });
    fireEvent.click(screen.getByRole('button', { name: 'Toggle 8' }), { ctrlKey: true });
    expect(screen.getByTestId('selection')).toHaveTextContent('2,8|8');

    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    expect(screen.getByTestId('selection')).toHaveTextContent('8,9|8');
  });

  it('starts Shift+Arrow from a valid anchor when the selection is empty', () => {
    render(<GridArrowNavHarness initiallySelected={false} />);

    fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });

    expect(screen.getByTestId('selection')).toHaveTextContent('1,2|1');
  });
});
