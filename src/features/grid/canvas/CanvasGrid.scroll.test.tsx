import { act, render } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { CanvasGrid } from './CanvasGrid';

vi.mock('./useCanvasRedrawScheduler', () => ({
  useCanvasRedrawScheduler: () => ({ markDirty: vi.fn() }),
}));

vi.mock('./thumbnailPipeline', () => ({
  ThumbnailPipeline: class {
    clear() {}
    destroy() {}
    invalidate() {}
  },
}));

vi.mock('../../../shared/lib/thumbnailChanges', () => ({
  listenThumbnailChanged: vi.fn().mockResolvedValue(() => {}),
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

  it('keeps the sticky viewport inside the media scroll extent', () => {
    const view = render(
      <CanvasGrid
        items={[]}
        viewMode="grid"
        targetSize={180}
        showName
        showExtension={false}
        headerContent={<div>Subfolders</div>}
      />,
    );
    const canvasWrap = view.container.querySelector<HTMLElement>('[data-grid-layout]');
    const viewport = canvasWrap?.firstElementChild as HTMLElement | null;

    expect(viewport?.style.maxHeight).toBe(canvasWrap?.style.height);
  });
});
