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

/** Height is the only live canvas adjustment; CSS already owns width. */
export function applyTransientViewportHeight(
  height: number,
  viewportLayer: HTMLDivElement | null,
  canvases: Array<HTMLCanvasElement | null>,
): void {
  if (viewportLayer) viewportLayer.style.height = `${height}px`;
  for (const canvas of canvases) {
    if (!canvas) continue;
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

    let appliedHeight = -1;
    const applyHeight = (height: number) => {
      if (height === appliedHeight) return;
      appliedHeight = height;
      applyTransientViewportHeight(height, refs.viewportLayer.current, [refs.baseCanvas.current, refs.overlayCanvas.current]);
    };
    const settle = () => {
      applyHeight(container.clientHeight);
      const width = container.clientWidth;
      const scrollbarWidth = container.offsetWidth - width;
      setLayoutWidth((current) => (
        current.width === width && current.scrollbarWidth === scrollbarWidth
          ? current
          : { width, scrollbarWidth }
      ));
      refs.redraw.current?.();
    };
    const scheduleSettlement = (entries: ResizeObserverEntry[]) => {
      const height = entries[0]?.contentRect.height;
      if (Number.isFinite(height)) applyHeight(height);
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
