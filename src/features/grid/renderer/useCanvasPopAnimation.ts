import { useEffect, type RefObject } from 'react';
import type { LayoutItem } from '../layoutMath';
import type { MasonryImageItem } from '../shared';

export function useCanvasPopAnimation(args: {
  popHash?: string | null;
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  onPopComplete?: () => void;
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
  layoutRef: { current: { positions: LayoutItem[] } };
  imagesRef: { current: MasonryImageItem[] };
}) {
  const { popHash, scrollContainerRef, onPopComplete, getScrollMetrics, layoutRef, imagesRef } = args;

  useEffect(() => {
    if (!popHash) return;
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) {
      onPopComplete?.();
      return;
    }

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const idx = imgs.findIndex((img) => img.hash === popHash);
    if (idx === -1 || !positions[idx]) {
      onPopComplete?.();
      return;
    }

    const pos = positions[idx];
    const metrics = getScrollMetrics();
    const viewportH = metrics.viewportHeight;
    const scrollTop = metrics.localScrollTop;

    if (pos.y < scrollTop || pos.y + pos.h > scrollTop + viewportH) {
      const targetLocalScroll = pos.y - viewportH / 2 + pos.h / 2;
      scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + targetLocalScroll);
    }
    onPopComplete?.();
  }, [getScrollMetrics, imagesRef, layoutRef, onPopComplete, popHash, scrollContainerRef]);
}
