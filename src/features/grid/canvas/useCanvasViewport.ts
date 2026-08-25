import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';

export const GRID_RESIZE_SETTLE_MS = 180;

export interface CommittedViewportSize {
  width: number;
  height: number;
  dpr: number;
}

export function verticalResizeScrollDelta(
  previousScreenY: number,
  nextScreenY: number,
  previousHeight: number,
  nextHeight: number,
): number {
  const windowTopDelta = nextScreenY - previousScreenY;
  const heightDelta = nextHeight - previousHeight;
  if (windowTopDelta === 0 || heightDelta === 0) return 0;
  return Math.abs(windowTopDelta + heightDelta) <= 2 ? windowTopDelta : 0;
}

interface CanvasViewportRefs {
  container: RefObject<HTMLDivElement>;
  contentFrame: RefObject<HTMLDivElement>;
  viewportLayer: RefObject<HTMLDivElement>;
  baseCanvas: RefObject<HTMLCanvasElement>;
  overlayCanvas: RefObject<HTMLCanvasElement>;
  header: RefObject<HTMLDivElement>;
  redraw: RefObject<() => void>;
  redrawNow: RefObject<() => void>;
  previewResize: RefObject<() => boolean>;
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
    let lastWindowScreenY = window.screenY;
    let resizeFrame: number | null = null;
    let pendingHeight: { height: number; dpr: number; screenY: number } | null = null;

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
      const committed = committedSizeRef.current;
      const nextWidth = container.clientWidth;
      const nextHeight = container.clientHeight;
      const nextDpr = window.devicePixelRatio || 1;
      const nextWindowScreenY = window.screenY;

      // Height does not participate in grid layout. Coalesce raw observer
      // notifications to one atomic resize-and-paint per display frame.
      if (committed.width > 0 && committed.dpr === nextDpr
        && nextHeight > 0 && committed.height !== nextHeight) {
        pendingHeight = { height: nextHeight, dpr: nextDpr, screenY: nextWindowScreenY };
        if (resizeFrame === null) {
          resizeFrame = requestAnimationFrame(() => {
            resizeFrame = null;
            const pending = pendingHeight;
            pendingHeight = null;
            if (!pending) return;
            const current = committedSizeRef.current;
            const scrollDelta = verticalResizeScrollDelta(
              lastWindowScreenY,
              pending.screenY,
              current.height,
              pending.height,
            );
            if (scrollDelta !== 0) {
              container.scrollTop = Math.max(0, container.scrollTop + scrollDelta);
            }
            lastWindowScreenY = pending.screenY;
            committedSizeRef.current = { ...current, height: pending.height };
            if (refs.viewportLayer.current) {
              refs.viewportLayer.current.style.width = `${current.width}px`;
              refs.viewportLayer.current.style.height = `${pending.height}px`;
            }
            if (refs.previewResize.current?.() ?? true) refs.redrawNow.current?.();
          });
        }
      }

      // Height-only resize is complete. Width and DPR changes retain the
      // settled commit because either one affects horizontal layout.
      if (committed.width === nextWidth && committed.dpr === nextDpr) {
        if (settleTimer.current) {
          clearTimeout(settleTimer.current);
          settleTimer.current = null;
        }
        return;
      }
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
      if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
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
