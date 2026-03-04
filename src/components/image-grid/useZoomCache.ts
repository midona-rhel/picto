import { useEffect, useLayoutEffect, useRef, type RefObject } from 'react';
import type { ImageSize, ZoomState } from './useImageZoom';

/**
 * Persists zoom state per-image in a Map (debounced writes)
 * and restores cached zoom or pre-calculates fitScale on navigation.
 */
export function useZoomCache(
  currentHash: string | null,
  imageSize: { width: number; height: number } | null,
  zoomState: ZoomState,
  setZoomState: (s: ZoomState) => void,
  calcFitScale: (size: ImageSize) => number,
  zoomCache: Map<string, ZoomState>,
  imageLoaded: boolean,
  containerRef?: RefObject<HTMLElement | null>,
  options?: { enabled?: boolean; onRestore?: () => void },
): void {
  const enabled = options?.enabled ?? true;

  // Reads container dimensions directly from the DOM to avoid stale
  // containerSize React state (which may still be {0,0} on first render).
  useLayoutEffect(() => {
    if (!currentHash) return;

    if (!imageSize) {
      setZoomState({ scale: 1, tx: 0, ty: 0 });
      return;
    }

    const el = containerRef?.current;
    const cw = el?.clientWidth ?? 0;
    const ch = el?.clientHeight ?? 0;

    const fitScale = cw > 0
      ? Math.min(cw / imageSize.width, ch / imageSize.height, 1)
      : calcFitScale(imageSize); // fallback to React-state-based calc

    setZoomState({ scale: fitScale, tx: 0, ty: 0 });
  }, [currentHash]); // eslint-disable-line react-hooks/exhaustive-deps

  const zoomCacheTimerRef = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => {
    if (!enabled || !currentHash || !imageLoaded) return;
    clearTimeout(zoomCacheTimerRef.current);
    zoomCacheTimerRef.current = setTimeout(() => {
      zoomCache.set(currentHash, zoomState);
    }, 200);
  }, [zoomState, currentHash, imageLoaded, enabled, zoomCache]);
}
