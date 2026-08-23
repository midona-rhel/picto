import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { applyTransientViewportHeight, GRID_RESIZE_SETTLE_MS, useCanvasViewport } from './useCanvasViewport';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('canvas viewport resize', () => {
  it('updates only CSS height without reallocating canvas backing stores', () => {
    const layer = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    base.width = 400;
    base.height = 300;
    overlay.width = 400;
    overlay.height = 300;

    applyTransientViewportHeight(640, layer, [base, overlay]);

    expect(layer.style.height).toBe('640px');
    expect(base.style.width).toBe('');
    expect(base.style.height).toBe('640px');
    expect(overlay.style.width).toBe('');
    expect(base.width).toBe(400);
    expect(base.height).toBe(300);
  });

  it('defers canvas redraw and width layout until resize settlement', () => {
    vi.useFakeTimers();
    let resize: ((entries: ResizeObserverEntry[]) => void) | undefined;
    class ResizeObserverStub {
      constructor(callback: (entries: ResizeObserverEntry[]) => void) { resize = callback; }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));
    const container = document.createElement('div');
    const layer = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    let width = 800;
    let height = 600;
    const readWidth = vi.fn(() => width);
    Object.defineProperties(container, {
      clientWidth: { get: readWidth, configurable: true },
      clientHeight: { get: () => height, configurable: true },
      offsetWidth: { get: () => width + 12, configurable: true },
    });
    const redraw = vi.fn();
    const refs = {
      container: { current: container }, viewportLayer: { current: layer },
      baseCanvas: { current: base }, overlayCanvas: { current: overlay },
      header: { current: null }, redraw: { current: redraw },
    };
    const view = renderHook(() => useCanvasViewport(refs, null));
    expect(view.result.current.layoutWidth).toEqual({ width: 800, scrollbarWidth: 12 });
    expect(redraw).toHaveBeenCalledTimes(1);
    const widthReadsAfterInitialLayout = readWidth.mock.calls.length;

    width = 920;
    act(() => resize?.([{ contentRect: { height: 600 } } as ResizeObserverEntry]));
    expect(readWidth).toHaveBeenCalledTimes(widthReadsAfterInitialLayout);
    expect(redraw).toHaveBeenCalledTimes(1);
    expect(view.result.current.layoutWidth.width).toBe(800);

    height = 680;
    act(() => resize?.([{ contentRect: { height: 680 } } as ResizeObserverEntry]));
    expect(layer.style.height).toBe('680px');
    expect(redraw).toHaveBeenCalledTimes(1);
    expect(view.result.current.layoutWidth.width).toBe(800);

    act(() => vi.advanceTimersByTime(GRID_RESIZE_SETTLE_MS));
    expect(redraw).toHaveBeenCalledTimes(2);
    expect(view.result.current.layoutWidth).toEqual({ width: 920, scrollbarWidth: 12 });
  });
});
