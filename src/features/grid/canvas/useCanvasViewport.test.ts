import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { applyTransientViewportSize, GRID_RESIZE_SETTLE_MS, useCanvasViewport } from './useCanvasViewport';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('applyTransientViewportSize', () => {
  it('updates CSS geometry without reallocating canvas backing stores', () => {
    const container = document.createElement('div');
    const layer = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    Object.defineProperties(container, {
      clientWidth: { value: 920 },
      clientHeight: { value: 640 },
    });
    base.width = 400;
    base.height = 300;
    overlay.width = 400;
    overlay.height = 300;

    applyTransientViewportSize(container, layer, [base, overlay]);

    expect(layer.style.height).toBe('640px');
    expect(base.style.width).toBe('920px');
    expect(base.style.height).toBe('640px');
    expect(overlay.style.width).toBe('920px');
    expect(base.width).toBe(400);
    expect(base.height).toBe(300);
  });

  it('defers canvas redraw and width layout until resize settlement', () => {
    vi.useFakeTimers();
    let resize: (() => void) | undefined;
    class ResizeObserverStub {
      constructor(callback: () => void) { resize = callback; }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('matchMedia', () => ({ addEventListener() {}, removeEventListener() {} }));
    const container = document.createElement('div');
    const layer = document.createElement('div');
    const base = document.createElement('canvas');
    const overlay = document.createElement('canvas');
    Object.defineProperties(container, {
      clientWidth: { value: 800, configurable: true },
      clientHeight: { value: 600, configurable: true },
      offsetWidth: { value: 812, configurable: true },
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

    Object.defineProperties(container, {
      clientWidth: { value: 920, configurable: true },
      clientHeight: { value: 680, configurable: true },
      offsetWidth: { value: 932, configurable: true },
    });
    act(() => resize?.());
    expect(layer.style.height).toBe('680px');
    expect(redraw).toHaveBeenCalledTimes(1);
    expect(view.result.current.layoutWidth.width).toBe(800);

    act(() => vi.advanceTimersByTime(GRID_RESIZE_SETTLE_MS));
    expect(redraw).toHaveBeenCalledTimes(2);
    expect(view.result.current.layoutWidth).toEqual({ width: 920, scrollbarWidth: 12 });
  });
});
