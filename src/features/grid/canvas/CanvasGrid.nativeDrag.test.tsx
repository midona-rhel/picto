import { act, cleanup, fireEvent, render } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { CanvasGrid } from './CanvasGrid';

const mocks = vi.hoisted(() => ({
  resolveTargetFilePaths: vi.fn(),
  startNativeDrag: vi.fn(),
  drag: {
    active: false,
    state: {
      active: false,
      ownerId: 1,
      itemIds: [1],
      sourceScope: { kind: 'all' as const } as { kind: 'all' } | null,
      startX: 0,
      startY: 0,
      currentX: 0,
      currentY: 0,
      dropTarget: null,
    },
  },
}));

vi.mock('../dragState', () => ({
  cancelDrag: () => { mocks.drag.active = false; },
  createDragOwnerId: () => 1,
  endDrag: () => { mocks.drag.active = false; },
  getDragState: () => mocks.drag.state,
  isDragActive: () => mocks.drag.active,
  isDragOwnedBy: () => mocks.drag.active,
  moveDrag: vi.fn(),
  setDropTarget: vi.fn(),
  setInternalDragOrigin: vi.fn(),
  startDrag: vi.fn(),
  startNativeDrag: (hashes: string[], icon: string) => {
    mocks.startNativeDrag(hashes, icon);
    mocks.drag.active = false;
  },
}));

vi.mock('../../../platform/shellApi', () => ({
  resolveTargetFilePaths: (...args: unknown[]) => mocks.resolveTargetFilePaths(...args),
}));

vi.mock('../dragGhostSpec', () => ({
  createNativeDragImageUrl: () => 'data:image/png;base64,drag',
}));

vi.mock('./thumbnailRevealTracker', () => ({
  ThumbnailRevealTracker: class {
    clear() {}
    updateViewport() {}
  },
}));

vi.mock('./useCanvasRedrawScheduler', () => ({
  useCanvasRedrawScheduler: () => ({ markDirty: vi.fn() }),
}));

vi.mock('./thumbnailPipeline', () => ({
  ThumbnailPipeline: class {
    clear() {}
    destroy() {}
    get() { return null; }
    invalidate() {}
    updatePlan() {}
  },
}));

vi.mock('./useCanvasViewport', () => ({
  useCanvasViewport: () => ({
    layoutWidth: { width: 500, scrollbarWidth: 0 },
    headerHeight: 0,
    committedSizeRef: { current: { width: 500, height: 500, dpr: 1 } },
  }),
}));

vi.mock('../../../shared/lib/thumbnailChanges', () => ({
  listenThumbnailChanged: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('../../../controllers/zoomController', () => ({
  zoomController: { subscribe: () => () => {} },
}));

const item = (rootId: number, hash = `hash-${rootId}`): CanonicalEntityGridItem => ({
  root_id: rootId,
  kind: 'media',
  lifecycle: 'active',
  name: `Item ${rootId}`,
  cover_media_id: rootId,
  content_hash: hash,
  mime: 'image/jpeg',
  width: 100,
  height: 100,
  duration_ms: null,
  frame_count: null,
  palette: [],
  imported_at_ms: rootId,
  captured_at_ms: null,
  modified_at_ms: rootId,
  rating: 'unrated',
  media_count: 1,
  total_size_bytes: 100,
});

beforeEach(() => {
  mocks.resolveTargetFilePaths.mockReset();
  mocks.startNativeDrag.mockReset();
  mocks.drag.active = false;
  (window as any).picto = { webview: { startNativeDrag: mocks.startNativeDrag } };
});

afterEach(() => {
  mocks.drag.active = false;
  cleanup();
});

function renderGrid(props: Partial<ComponentProps<typeof CanvasGrid>> = {}) {
  const items = props.items ?? [item(1)];
  return render(
    <CanvasGrid
      items={items}
      viewMode="grid"
      targetSize={180}
      showName
      showExtension={false}
      dragSourceScope={{ kind: 'all' }}
      {...props}
    />,
  );
}

function beginNativeDrag(itemIds = [1], sourceScope: { kind: 'all' } | null = { kind: 'all' }) {
  mocks.drag.state = {
    ...mocks.drag.state,
    active: true,
    itemIds,
    sourceScope,
  };
  mocks.drag.active = true;
  fireEvent.mouseMove(window, { clientX: -1, clientY: 20 });
  if (sourceScope) expect(mocks.resolveTargetFilePaths).toHaveBeenCalledOnce();
}

function pendingResolution() {
  let finish: ((value: Array<{ file_hash: string; path: string }>) => void) | undefined;
  mocks.resolveTargetFilePaths.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  return async () => {
    await act(async () => {
      finish?.([{ file_hash: 'resolved-hash', path: '/library/resolved.jpg' }]);
      await Promise.resolve();
    });
  };
}

describe('CanvasGrid native drag cancellation', () => {
  it('hands every resolved file to the OS, including files outside the loaded window', async () => {
    mocks.resolveTargetFilePaths.mockResolvedValue([
      { file_hash: 'hash-1', path: '/library/one.jpg' },
      { file_hash: 'hash-999', path: '/library/unloaded.jpg' },
    ]);
    renderGrid({ items: [item(1)] });
    beginNativeDrag([1, 999]);
    await act(async () => { await Promise.resolve(); });

    expect(mocks.resolveTargetFilePaths).toHaveBeenCalledWith({ kind: 'explicit', root_ids: [1, 999] });
    expect(mocks.startNativeDrag).toHaveBeenCalledWith(
      ['hash-1', 'hash-999'],
      'data:image/png;base64,drag',
    );
  });

  it('does not start a stale OS drag after mouseup', async () => {
    const finish = pendingResolution();
    renderGrid();
    beginNativeDrag();

    fireEvent.mouseUp(window, { clientX: 100, clientY: 100 });
    await finish();

    expect(mocks.startNativeDrag).not.toHaveBeenCalled();
  });

  it('does not start a stale OS drag after Escape', async () => {
    const finish = pendingResolution();
    renderGrid();
    beginNativeDrag();

    fireEvent.keyDown(window, { key: 'Escape' });
    await finish();

    expect(mocks.startNativeDrag).not.toHaveBeenCalled();
  });

  it('does not start a stale OS drag after unmount', async () => {
    const finish = pendingResolution();
    const view = renderGrid();
    beginNativeDrag();

    view.unmount();
    await finish();

    expect(mocks.startNativeDrag).not.toHaveBeenCalled();
  });

  it('does not start a stale OS drag after the source window changes', async () => {
    const finish = pendingResolution();
    const view = renderGrid();
    beginNativeDrag();

    view.rerender(
      <CanvasGrid
        items={[item(2)]}
        viewMode="grid"
        targetSize={180}
        showName
        showExtension={false}
        dragSourceScope={{ kind: 'all' }}
        windowStart={100}
      />,
    );
    await finish();

    expect(mocks.startNativeDrag).not.toHaveBeenCalled();
  });

  it('drags reusable group media by their loaded physical hashes without backend resolution', () => {
    renderGrid({
      items: [item(1001, 'group-member-hash')],
      dragSourceScope: null,
      onReorder: vi.fn(),
    });
    beginNativeDrag([1001], null);

    expect(mocks.resolveTargetFilePaths).not.toHaveBeenCalled();
    expect(mocks.startNativeDrag).toHaveBeenCalledWith(
      ['group-member-hash'],
      'data:image/png;base64,drag',
    );
  });
});
