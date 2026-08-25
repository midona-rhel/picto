import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { snapshotViewport } from './canvasViewportUtils';
import { applyCommittedViewportSize, GRID_RESIZE_SETTLE_MS, useCanvasViewport } from './useCanvasViewport';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

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

  it('keeps the complete frame frozen during live resize and commits once after settlement', () => {
    vi.useFakeTimers();
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
    expect(viewport.style.width).toBe('720px');
    expect(viewport.style.height).toBe('400px');
    expect(contentFrame.style.width).toBe('720px');
    expect(base.style.width).toBe('720px');
    expect(base.width).toBe(720);
    act(() => { vi.advanceTimersByTime(GRID_RESIZE_SETTLE_MS - 1); });
    expect(viewport.style.height).toBe('400px');
    act(() => { vi.advanceTimersByTime(1); });
    expect(viewport.style.width).toBe('880px');
    expect(viewport.style.height).toBe('560px');
    expect(contentFrame.style.width).toBe('880px');
    expect(base.style.width).toBe('880px');
    expect(base.width).toBe(880);
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
