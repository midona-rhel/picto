import { useEffect, type RefObject } from 'react';

export function useCanvasLoadMore(args: {
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  onLoadMore?: (() => void) | null;
  onLoadMoreRef: { current: (() => void) | undefined };
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
  layoutRef: { current: { totalHeight: number } };
  threshold: number;
}) {
  const { scrollContainerRef, onLoadMore, onLoadMoreRef, getScrollMetrics, layoutRef, threshold } = args;

  useEffect(() => {
    const scrollElement = scrollContainerRef?.current;
    if (!scrollElement || !onLoadMore) return;
    const onScroll = () => {
      const metrics = getScrollMetrics();
      if (metrics.localScrollTop + metrics.viewportHeight > layoutRef.current.totalHeight - threshold) {
        onLoadMoreRef.current?.();
      }
    };
    scrollElement.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scrollElement.removeEventListener('scroll', onScroll);
    };
  }, [getScrollMetrics, layoutRef, onLoadMore, onLoadMoreRef, scrollContainerRef, threshold]);
}
