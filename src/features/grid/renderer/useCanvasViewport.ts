import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

function useMeasuredContainerWidth() {
  const [width, setWidth] = useState(0);
  const roRef = useRef<ResizeObserver | null>(null);

  const ref = useCallback((element: HTMLDivElement | null) => {
    if (roRef.current) {
      roRef.current.disconnect();
      roRef.current = null;
    }
    if (!element) return;

    const observer = new ResizeObserver(([entry]) => {
      setWidth(Math.round(entry.contentRect.width));
    });

    observer.observe(element);
    roRef.current = observer;
  }, []);

  return { ref, width };
}

export function useCanvasViewport(args: {
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  onContainerWidthChange?: (width: number) => void;
  frozen: boolean;
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
  isScrollingRef: { current: boolean };
  pendingAtlasDirtyRef: { current: boolean };
  dismissHoverPreviewRef: { current: () => void };
  dismissVideoScrubRef: { current: () => void };
}) {
  const {
    scrollContainerRef,
    onContainerWidthChange,
    frozen,
    markDirty,
    isScrollingRef,
    pendingAtlasDirtyRef,
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
  } = args;

  const { ref: measureContainerRef, width: containerWidth } = useMeasuredContainerWidth();
  const containerElRef = useRef<HTMLDivElement | null>(null);
  const scrollTopRef = useRef(0);
  const viewportHeightRef = useRef(0);
  const [canvasHeight, setCanvasHeight] = useState(0);
  const [frozenCanvasWidth, setFrozenCanvasWidth] = useState<number | null>(null);
  const [frozenLayoutWidth, setFrozenLayoutWidth] = useState<number | null>(null);
  const wasFrozenRef = useRef(false);
  const unfreezeSettleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const containerRef = useCallback((element: HTMLDivElement | null) => {
    containerElRef.current = element;
    measureContainerRef(element);
  }, [measureContainerRef]);

  const layoutWidth = frozenLayoutWidth ?? containerWidth;

  useEffect(() => {
    if (frozen || frozenLayoutWidth != null) return;
    onContainerWidthChange?.(containerWidth);
  }, [containerWidth, frozen, frozenLayoutWidth, onContainerWidthChange]);

  const getScrollMetrics = useCallback(() => {
    const scrollElement = scrollContainerRef?.current;
    if (!scrollElement) {
      return { localScrollTop: 0, canvasTopInScroll: 0, viewportHeight: 0 };
    }

    const viewportHeight = scrollElement.clientHeight;
    const globalScrollTop = scrollElement.scrollTop;
    const containerElement = containerElRef.current;
    if (!containerElement) {
      return {
        localScrollTop: globalScrollTop,
        canvasTopInScroll: 0,
        viewportHeight,
      };
    }

    const scrollRect = scrollElement.getBoundingClientRect();
    const containerRect = containerElement.getBoundingClientRect();
    const canvasTopInScroll = globalScrollTop + (containerRect.top - scrollRect.top);
    const localScrollTop = Math.max(0, globalScrollTop - canvasTopInScroll);
    return {
      localScrollTop,
      canvasTopInScroll,
      viewportHeight,
    };
  }, [scrollContainerRef]);

  useEffect(() => {
    if (unfreezeSettleTimerRef.current) {
      clearTimeout(unfreezeSettleTimerRef.current);
      unfreezeSettleTimerRef.current = null;
    }

    if (frozen && !wasFrozenRef.current) {
      const resolvedWidth = containerElRef.current?.clientWidth ?? containerWidth;
      const nextFrozenWidth = resolvedWidth > 0 ? resolvedWidth : null;
      setFrozenCanvasWidth(nextFrozenWidth);
      setFrozenLayoutWidth(nextFrozenWidth);
      dismissVideoScrubRef.current();
    } else if (!frozen && wasFrozenRef.current) {
      setFrozenCanvasWidth(null);
      unfreezeSettleTimerRef.current = setTimeout(() => {
        setFrozenLayoutWidth(null);
        unfreezeSettleTimerRef.current = null;
      }, 140);
    }

    wasFrozenRef.current = frozen;
  }, [containerWidth, dismissVideoScrubRef, frozen]);

  useEffect(() => {
    const scrollElement = scrollContainerRef?.current;
    if (!scrollElement) return;

    const initialMetrics = getScrollMetrics();
    viewportHeightRef.current = initialMetrics.viewportHeight;
    scrollTopRef.current = initialMetrics.localScrollTop;
    setCanvasHeight(initialMetrics.viewportHeight);
    markDirty('both');

    let rafId = 0;
    let scrollIdleTimer = 0;

    const onScroll = () => {
      isScrollingRef.current = true;
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
      scrollIdleTimer = window.setTimeout(() => {
        isScrollingRef.current = false;
        if (pendingAtlasDirtyRef.current) {
          pendingAtlasDirtyRef.current = false;
        }
        markDirty('both');
      }, 120);

      dismissHoverPreviewRef.current();
      dismissVideoScrubRef.current();

      if (rafId) return;
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        const metrics = getScrollMetrics();
        scrollTopRef.current = metrics.localScrollTop;
        viewportHeightRef.current = metrics.viewportHeight;
        markDirty('both');
      });
    };

    const onResize = () => {
      const metrics = getScrollMetrics();
      viewportHeightRef.current = metrics.viewportHeight;
      setCanvasHeight(metrics.viewportHeight);
      markDirty('both');
    };

    scrollElement.addEventListener('scroll', onScroll, { passive: true });
    const observer = new ResizeObserver(onResize);
    observer.observe(scrollElement);

    return () => {
      scrollElement.removeEventListener('scroll', onScroll);
      observer.disconnect();
      if (rafId) cancelAnimationFrame(rafId);
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
      isScrollingRef.current = false;
    };
  }, [
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
    getScrollMetrics,
    isScrollingRef,
    markDirty,
    pendingAtlasDirtyRef,
    scrollContainerRef,
  ]);

  useEffect(() => {
    return () => {
      if (unfreezeSettleTimerRef.current) {
        clearTimeout(unfreezeSettleTimerRef.current);
      }
    };
  }, []);

  return {
    containerRef,
    containerElRef,
    containerWidth,
    layoutWidth,
    canvasHeight,
    frozenCanvasWidth,
    getScrollMetrics,
    scrollTopRef,
    viewportHeightRef,
  };
}
