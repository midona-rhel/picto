import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';

export const GRID_RESIZE_SETTLE_MS = 180;

export interface CommittedViewportSize {
  width: number;
  height: number;
  dpr: number;
}

interface CanvasViewportRefs {
  container: RefObject<HTMLDivElement>;
  contentFrame: RefObject<HTMLDivElement>;
  viewportLayer: RefObject<HTMLDivElement>;
  baseCanvas: RefObject<HTMLCanvasElement>;
  overlayCanvas: RefObject<HTMLCanvasElement>;
  header: RefObject<HTMLDivElement>;
  redraw: RefObject<() => void>;
}

/** Commit the visible viewport and both backing buffers as one settled frame. */
export function applyCommittedViewportSize(
  width: number,
  height: number,
  contentFrame: HTMLDivElement | null,
  viewportLayer: HTMLDivElement | null,
  canvases: Array<HTMLCanvasElement | null>,
  dpr: number,
): boolean {
  let changed = false;
  if (contentFrame) contentFrame.style.width = `${width}px`;
  if (viewportLayer) {
    viewportLayer.style.width = `${width}px`;
    viewportLayer.style.height = `${height}px`;
  }
  for (const canvas of canvases) {
    if (!canvas) continue;
    const backingWidth = Math.round(width * dpr);
    const backingHeight = Math.round(height * dpr);
    if (canvas.width !== backingWidth) {
      canvas.width = backingWidth;
      changed = true;
    }
    if (canvas.height !== backingHeight) {
      canvas.height = backingHeight;
      changed = true;
    }
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }
  return changed;
}

export function useCanvasViewport(
  refs: CanvasViewportRefs,
  headerContent: unknown,
  immediateCommitKey: unknown,
) {
  const [layoutWidth, setLayoutWidth] = useState({ width: 0, scrollbarWidth: 0 });
  const [headerHeight, setHeaderHeight] = useState(0);
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const committedSizeRef = useRef<CommittedViewportSize>({ width: 0, height: 0, dpr: 1 });

  useLayoutEffect(() => {
    const container = refs.container.current;
    if (!container) return;

    const settle = () => {
      const width = container.clientWidth;
      const height = container.clientHeight;
      const dpr = window.devicePixelRatio || 1;
      committedSizeRef.current = { width, height, dpr };
      applyCommittedViewportSize(
        width,
        height,
        refs.contentFrame.current,
        refs.viewportLayer.current,
        [refs.baseCanvas.current, refs.overlayCanvas.current],
        dpr,
      );
      const scrollbarWidth = container.offsetWidth - width;
      setLayoutWidth((current) => (
        current.width === width && current.scrollbarWidth === scrollbarWidth
          ? current
          : { width, scrollbarWidth }
      ));
      refs.redraw.current?.();
    };
    const scheduleSettlement = () => {
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
  }, [immediateCommitKey, refs]);

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

  return { layoutWidth, headerHeight, committedSizeRef };
}
