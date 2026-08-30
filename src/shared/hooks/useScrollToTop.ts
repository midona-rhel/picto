import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

export function shouldShowScrollToTop(scrollTop: number, threshold: number): boolean {
  return scrollTop >= threshold;
}

export function useScrollToTop(
  containerRef: RefObject<HTMLElement | null>,
  threshold: number,
) {
  const sentinelRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    const sentinel = sentinelRef.current;
    if (!container || !sentinel) return;

    const update = () => {
      const next = shouldShowScrollToTop(container.scrollTop, threshold);
      setVisible((current) => current === next ? current : next);
    };

    update();
    if (typeof IntersectionObserver === 'undefined') {
      container.addEventListener('scroll', update, { passive: true });
      return () => container.removeEventListener('scroll', update);
    }

    const observer = new IntersectionObserver(update, {
      root: container,
      threshold: 0,
    });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [containerRef, threshold]);

  const scrollToTop = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    container.scrollTop = 0;
    setVisible(false);
  }, [containerRef]);

  return { sentinelRef, visible, scrollToTop };
}
