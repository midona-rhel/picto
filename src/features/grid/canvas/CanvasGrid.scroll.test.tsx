import { act, render } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { CanvasGrid } from './CanvasGrid';

vi.mock('./useCanvasRedrawScheduler', () => ({
  useCanvasRedrawScheduler: () => ({ markDirty: vi.fn() }),
}));

vi.mock('./thumbnailPipeline', () => ({
  ThumbnailPipeline: class {
    clear() {}
  },
}));

vi.mock('../../../controllers/zoomController', () => ({
  zoomController: { subscribe: () => () => {} },
}));

beforeAll(() => {
  class ResizeObserverStub {
    observe() {}
    disconnect() {}
  }
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
});

describe('CanvasGrid scroll freezing', () => {
  it('keeps the current scroll offset when an overlay pauses grid interaction', () => {
    let scrollContainer: HTMLDivElement | null = null;
    const props = {
      items: [],
      viewMode: 'grid' as const,
      targetSize: 180,
      showName: true,
      showExtension: false,
      onContainerRef: (element: HTMLDivElement | null) => { scrollContainer = element; },
    };
    const view = render(<CanvasGrid {...props} interactive />);

    act(() => { scrollContainer!.scrollTop = 360; });
    view.rerender(<CanvasGrid {...props} interactive={false} />);

    expect(scrollContainer!.scrollTop).toBe(360);
  });
});
