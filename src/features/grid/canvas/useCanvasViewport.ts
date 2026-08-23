import { useEffect, useRef, useState, type RefObject } from 'react';

export const GRID_RESIZE_SETTLE_MS = 180;

interface CanvasViewportRefs {
  container: RefObject<HTMLDivElement>;
  viewportLayer: RefObject<HTMLDivElement>;
  baseCanvas: RefObject<HTMLCanvasElement>;
  overlayCanvas: RefObject<HTMLCanvasElement>;
  header: RefObject<HTMLDivElement>;
  redraw: RefObject<() => void>;
}

/**
 * Stretch the existing bitmap during a live resize. This is deliberately a
 * CSS-only operation: backing-store resize, drawing, and width-dependent
 * layout happen once after the gesture settles.
 */
export function applyTransientViewportSize(
  container: HTMLDivElement,
  viewportLayer: HTMLDivElement | null,
  canvases: Array<HTMLCanvasElement | null>,
): void {
  const width = container.clientWidth;
  const height = container.clientHeight;
  if (viewportLayer) viewportLayer.style.height = `${height}px`;
  for (const canvas of canvases) {
    if (!canvas) continue;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }
}

export function useCanvasViewport(refs: CanvasViewportRefs, headerContent: unknown) {
  const [layoutWidth, setLayoutWidth] = useState({ width: 0, scrollbarWidth: 0 });
  const [headerHeight, setHeaderHeight] = useState(0);
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const container = refs.container.current;
    if (!container) return;

    const applyTransientSize = () => applyTransientViewportSize(
      container,
      refs.viewportLayer.current,
      [refs.baseCanvas.current, refs.overlayCanvas.current],
    );
    const settle = () => {
      applyTransientSize();
      const width = container.clientWidth;
      const scrollbarWidth = container.offsetWidth - width;
      setLayoutWidth((current) => (
        current.width === width && current.scrollbarWidth === scrollbarWidth
          ? current
          : { width, scrollbarWidth }
      ));
      refs.redraw.current?.();
    };
    const scheduleSettlement = () => {
      applyTransientSize();
      if (settleTimer.current) clearTimeout(settleTimer.current);
      settleTimer.current = setTimeout(() => {
        settleTimer.current = null;
        settle();
      }, GRID_RESIZE_SETTLE_MS);
    };

    settle();
    const observer = new ResizeObserver(scheduleSettlement);
    observer.observe(container);
    const dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    dprQuery.addEventListener('change', settle);

    return () => {
      observer.disconnect();
      dprQuery.removeEventListener('change', settle);
      if (settleTimer.current) clearTimeout(settleTimer.current);
    };
  }, [refs]);

  useEffect(() => {
    const header = refs.header.current;
    if (!header) {
      setHeaderHeight(0);
      return;
    }
    const measure = () => setHeaderHeight(header.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(header);
    return () => observer.disconnect();
  }, [headerContent, refs.header]);

  return { layoutWidth, headerHeight };
}
