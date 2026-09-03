import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  canvasScrollBufferIsExhausted,
  canvasScrollBufferNeedsRecenter,
  canvasScrollBufferTransform,
  snapshotViewport,
} from './canvasViewportUtils';
import {
  applyCommittedViewportSize,
  GRID_RESIZE_SETTLE_MS,
  useCanvasViewport,
  verticalResizeScrollDelta,
} from './useCanvasViewport';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function stubAnimationFrame() {
  let callback: FrameRequestCallback | null = null;
  vi.stubGlobal('requestAnimationFrame', (next: FrameRequestCallback) => {
    callback = next;
    return 1;
  });
  vi.stubGlobal('cancelAnimationFrame', () => { callback = null; });
  return () => {
    const next = callback;
    callback = null;
    next?.(0);
  };
}

describe('applyCommittedViewportSize', () => {
  it('commits CSS and both backing dimensions together', () => {
    const frame = document.createElement('div');
    const viewport = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    base.style.width = '720px';
    overlay.style.width = '720px';

    base.width = 1440;
    overlay.width = 1440;

    const changed = applyCommittedViewportSize(720, 480, frame, viewport, [base, overlay], 2);

    expect(changed).toBe(true);
    expect(frame.style.width).toBe('720px');
    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('480px');
    expect(base.style.height).toBe('480px');
    expect(overlay.style.height).toBe('480px');
    expect(base.height).toBe(960);
    expect(overlay.height).toBe(960);
    expect(base.width).toBe(1440);
    expect(overlay.width).toBe(1440);
    expect(base.style.width).toBe('720px');
    expect(overlay.style.width).toBe('720px');
  });

  it('does not clear a canvas again when the viewport height is unchanged', () => {
    const base = document.createElement('canvas');
    base.height = 960;

    base.width = 1440;
    expect(applyCommittedViewportSize(720, 480, null, null, [base], 2)).toBe(false);
    expect(base.height).toBe(960);
  });
});

describe('verticalResizeScrollDelta', () => {
  it('anchors top-edge growth while bottom-edge growth stays top-anchored', () => {
    expect(verticalResizeScrollDelta(300, 200, 400, 500)).toBe(-100);
    expect(verticalResizeScrollDelta(300, 300, 400, 500)).toBe(0);
  });

  it('ignores unrelated window movement and internal height changes', () => {
    expect(verticalResizeScrollDelta(300, 250, 400, 420)).toBe(0);
    expect(verticalResizeScrollDelta(300, 300, 400, 420)).toBe(0);
  });
});

describe('retained canvas scroll buffer', () => {
  it('translates completed pixels with the scroll delta', () => {
    expect(canvasScrollBufferTransform(1_000, 1_120, 500)).toBe(-620);
    expect(canvasScrollBufferTransform(1_000, 900, 500)).toBe(-400);
  });

  it('recenters before the retained margin is exhausted', () => {
    expect(canvasScrollBufferNeedsRecenter(1_000, 600, 1_349, 600, 350)).toBe(false);
    expect(canvasScrollBufferNeedsRecenter(1_000, 600, 1_351, 600, 350)).toBe(true);
    expect(canvasScrollBufferNeedsRecenter(1_000, 600, 649, 600, 350)).toBe(true);
  });

  it('reserves the full margin for safe asynchronous repainting', () => {
    expect(canvasScrollBufferIsExhausted(1_000, 600, 1_499, 600, 500)).toBe(false);
    expect(canvasScrollBufferIsExhausted(1_000, 600, 1_501, 600, 500)).toBe(true);
  });
});

describe('useCanvasViewport', () => {
  it('keeps scroll draws on the last committed dimensions', () => {
    const container = document.createElement('div');
    Object.defineProperties(container, {
      clientHeight: { value: 620 },
      clientWidth: { value: 900 },
      scrollTop: { value: 240, writable: true },
    });

    expect(snapshotViewport(container, { width: 720, height: 480, dpr: 2 })).toEqual({
      scrollTop: 240,
      viewportHeight: 480,
      containerWidth: 720,
      dpr: 2,
    });
  });

  it('updates height immediately while keeping width frozen until settlement', () => {
    vi.useFakeTimers();
    const paintFrame = stubAnimationFrame();
    let observedResize: (() => void) | null = null;
    class ResizeObserverStub {
      constructor(callback: () => void) { if (!observedResize) observedResize = callback; }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));

    let width = 720;
    let height = 400;
    const container = document.createElement('div');
    Object.defineProperties(container, {
      clientHeight: { get: () => height },
      clientWidth: { get: () => width },
      offsetWidth: { get: () => width + 8 },
    });
    const viewport = document.createElement('div');
    const contentFrame = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    const refs = {
      container: { current: container },
      contentFrame: { current: contentFrame },
      viewportLayer: { current: viewport },
      baseCanvas: { current: base },
      overlayCanvas: { current: overlay },
      header: { current: null },
      redraw: { current: vi.fn() },
      redrawNow: { current: vi.fn() },
      previewResize: { current: () => true },
    };

    renderHook(() => useCanvasViewport(refs, null, false));
    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('400px');
    expect(contentFrame.style.width).toBe('720px');
    expect(base.style.width).toBe('720px');
    expect(base.width).toBe(720);

    width = 880;
    height = 560;
    act(() => { observedResize?.(); });
    act(() => { paintFrame(); });
    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('560px');
    expect(contentFrame.style.width).toBe('720px');
    expect(base.style.width).toBe('720px');
    expect(base.width).toBe(720);
    // Height-only preview changes the clip; CanvasGrid owns its pre-rendered
    // backing buffer and recenters it only when the reserve is exhausted.
    expect(base.height).toBe(400);
    act(() => { vi.advanceTimersByTime(GRID_RESIZE_SETTLE_MS - 1); });
    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('560px');
    act(() => { vi.advanceTimersByTime(1); });
    expect(viewport.style.width).toBe('880px');
    expect(viewport.style.height).toBe('560px');
    expect(contentFrame.style.width).toBe('880px');
    expect(base.style.width).toBe('880px');
    expect(base.width).toBe(880);
  });

  it('does not schedule a settled layout commit for a height-only resize', () => {
    vi.useFakeTimers();
    const paintFrame = stubAnimationFrame();
    let observedResize: (() => void) | null = null;
    class ResizeObserverStub {
      constructor(callback: () => void) { if (!observedResize) observedResize = callback; }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));

    let height = 400;
    const container = document.createElement('div');
    Object.defineProperties(container, {
      clientHeight: { get: () => height },
      clientWidth: { value: 720 },
      offsetWidth: { value: 728 },
    });
    const viewport = document.createElement('div');
    const redraw = vi.fn();
    const redrawNow = vi.fn();
    const refs = {
      container: { current: container },
      contentFrame: { current: document.createElement('div') },
      viewportLayer: { current: viewport },
      baseCanvas: { current: document.createElement('canvas') },
      overlayCanvas: { current: document.createElement('canvas') },
      header: { current: null },
      redraw: { current: redraw },
      redrawNow: { current: redrawNow },
      previewResize: { current: () => true },
    };

    renderHook(() => useCanvasViewport(refs, null, false));
    height = 560;
    act(() => { observedResize?.(); });
    act(() => { paintFrame(); });

    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('560px');
    expect(redrawNow).toHaveBeenCalledTimes(1);
    act(() => { vi.advanceTimersByTime(GRID_RESIZE_SETTLE_MS); });
    expect(redrawNow).toHaveBeenCalledTimes(1);
    expect(redraw).toHaveBeenCalledTimes(1);
  });

  it('reveals earlier rows when the native top edge grows upward', () => {
    vi.useFakeTimers();
    const paintFrame = stubAnimationFrame();
    let observedResize: (() => void) | null = null;
    class ResizeObserverStub {
      constructor(callback: () => void) { if (!observedResize) observedResize = callback; }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));

    let height = 400;
    let screenY = 300;
    vi.spyOn(window, 'screenY', 'get').mockImplementation(() => screenY);
    const container = document.createElement('div');
    Object.defineProperties(container, {
      clientHeight: { get: () => height },
      clientWidth: { value: 720 },
      offsetWidth: { value: 728 },
      scrollTop: { value: 500, writable: true },
    });
    const refs = {
      container: { current: container },
      contentFrame: { current: document.createElement('div') },
      viewportLayer: { current: document.createElement('div') },
      baseCanvas: { current: document.createElement('canvas') },
      overlayCanvas: { current: document.createElement('canvas') },
      header: { current: null },
      redraw: { current: vi.fn() },
      redrawNow: { current: vi.fn() },
      previewResize: { current: () => true },
    };

    renderHook(() => useCanvasViewport(refs, null, false));
    height = 500;
    screenY = 200;
    act(() => { observedResize?.(); });
    act(() => { paintFrame(); });

    expect(container.scrollTop).toBe(400);
  });

  it('commits an intentional application layout change immediately', () => {
    vi.useFakeTimers();
    class ResizeObserverStub { observe() {} disconnect() {} }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));

    let height = 500;
    const container = document.createElement('div');
    Object.defineProperties(container, {
      clientHeight: { get: () => height },
      clientWidth: { get: () => 720 },
      offsetWidth: { get: () => 728 },
    });
    const viewport = document.createElement('div');
    const refs = {
      container: { current: container },
      contentFrame: { current: document.createElement('div') },
      viewportLayer: { current: viewport },
      baseCanvas: { current: document.createElement('canvas') },
      overlayCanvas: { current: document.createElement('canvas') },
      header: { current: null },
      redraw: { current: vi.fn() },
      redrawNow: { current: vi.fn() },
      previewResize: { current: () => true },
    };
    const { rerender } = renderHook(
      ({ commitKey }) => useCanvasViewport(refs, null, commitKey),
      { initialProps: { commitKey: 'filter:0:sidebar:0:inspector:0' } },
    );

    height = 468;
    rerender({ commitKey: 'filter:0:sidebar:1:inspector:1' });
    expect(viewport.style.height).toBe('468px');
  });
});
