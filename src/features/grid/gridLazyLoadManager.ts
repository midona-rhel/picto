import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { RefObject, SyntheticEvent } from 'react';

const thumbReadyCache = new Set<string>();
const activatedThumbCache = new Set<string>();
const thumbLoadedFromCache = new Set<string>();

const ROOT_MARGIN = '50px';
const THRESHOLD = [0, 0.01, 0.1];
const MAX_CONCURRENT_LOADS = 6;

type PendingLoad = {
  element: HTMLElement;
  priority: number;
};

type ActiveLoad = {
  element: HTMLElement;
  img: HTMLImageElement;
  src: string;
};

function getTileId(element: HTMLElement): string | null {
  return element.dataset.hash ?? null;
}

function getTileSrc(element: HTMLElement): string | null {
  const img = element.querySelector('img[data-src]') as HTMLImageElement | null;
  return img?.dataset.src ?? null;
}

function setTileState(element: HTMLElement, state: 'idle' | 'loading' | 'loaded' | 'error') {
  element.dataset.thumbState = state;
}

function clearCacheState(element: HTMLElement) {
  delete element.dataset.thumbCache;
}

function markLoaded(element: HTMLElement, src: string, fromCache: boolean) {
  thumbReadyCache.add(src);
  if (fromCache) {
    thumbLoadedFromCache.add(src);
    element.dataset.thumbCache = '1';
  } else {
    thumbLoadedFromCache.delete(src);
    clearCacheState(element);
  }
  setTileState(element, 'loaded');
}

function computePriority(element: HTMLElement, root: HTMLElement | Window | null): number {
  const posY = Number(element.dataset.posY ?? Number.NaN);
  if (!Number.isFinite(posY)) return 0;

  const scrollTop = root instanceof HTMLElement
    ? root.scrollTop
    : window.scrollY;

  return 10000 - Math.abs(posY - scrollTop);
}

export function isThumbReady(src: string): boolean {
  return thumbReadyCache.has(src);
}

export function hasActivatedThumb(src: string): boolean {
  return activatedThumbCache.has(src);
}

export function wasThumbLoadedFromCache(src: string): boolean {
  return thumbLoadedFromCache.has(src);
}

export function useGridLazyLoadManager(
  scrollRootRef?: RefObject<HTMLDivElement | null>,
) {
  const observerRef = useRef<IntersectionObserver | null>(null);
  const rootRef = useRef<HTMLElement | Window | null>(null);
  const observedRef = useRef(new Set<HTMLElement>());
  const pendingQueueRef = useRef<PendingLoad[]>([]);
  const activeLoadsRef = useRef(new Map<string, ActiveLoad>());

  const removePending = useCallback((element: HTMLElement) => {
    const queue = pendingQueueRef.current;
    for (let index = queue.length - 1; index >= 0; index--) {
      if (queue[index].element === element) {
        queue.splice(index, 1);
      }
    }
  }, []);

  const processNextInQueue = useCallback(() => {
    if (activeLoadsRef.current.size >= MAX_CONCURRENT_LOADS) return;

    while (pendingQueueRef.current.length > 0) {
      const next = pendingQueueRef.current.shift()!;
      if (!next.element.isConnected) continue;
      if (next.element.dataset.thumbState === 'loaded') continue;

      const src = getTileSrc(next.element);
      const img = next.element.querySelector('img[data-src]') as HTMLImageElement | null;
      const id = getTileId(next.element);
      if (!src || !img || !id) continue;

      setTileState(next.element, 'loading');
      clearCacheState(next.element);
      activatedThumbCache.add(src);
      activeLoadsRef.current.set(id, { element: next.element, img, src });
      img.src = src;

      if (img.complete && img.naturalHeight > 0) {
        activeLoadsRef.current.delete(id);
        markLoaded(next.element, src, true);
        observerRef.current?.unobserve(next.element);
      }
      return;
    }
  }, []);

  const cancelLoad = useCallback((element: HTMLElement) => {
    removePending(element);

    const id = getTileId(element);
    if (!id) return;

    const active = activeLoadsRef.current.get(id);
    if (!active) return;

    activeLoadsRef.current.delete(id);
    activatedThumbCache.delete(active.src);
    active.img.removeAttribute('src');
    setTileState(element, 'idle');
    clearCacheState(element);
    processNextInQueue();
  }, [processNextInQueue, removePending]);

  const scheduleLoad = useCallback((element: HTMLElement, intersectionRatio: number) => {
    const src = getTileSrc(element);
    const img = element.querySelector('img[data-src]') as HTMLImageElement | null;
    const id = getTileId(element);
    if (!src || !img || !id) return;
    if (activeLoadsRef.current.has(id)) return;
    if (element.dataset.thumbState === 'loaded') return;

    if (thumbReadyCache.has(src)) {
      if (!img.src) img.src = src;
      markLoaded(element, src, true);
      observerRef.current?.unobserve(element);
      return;
    }

    const priorityBase = computePriority(element, rootRef.current);
    const priority = priorityBase + Math.round(intersectionRatio * 1000);

    if (activeLoadsRef.current.size >= MAX_CONCURRENT_LOADS) {
      if (!pendingQueueRef.current.some((entry) => entry.element === element)) {
        pendingQueueRef.current.push({ element, priority });
        pendingQueueRef.current.sort((a, b) => b.priority - a.priority);
      }
      return;
    }

    setTileState(element, 'loading');
    clearCacheState(element);
    activatedThumbCache.add(src);
    activeLoadsRef.current.set(id, { element, img, src });
    img.src = src;

    if (img.complete && img.naturalHeight > 0) {
      activeLoadsRef.current.delete(id);
      markLoaded(element, src, true);
      observerRef.current?.unobserve(element);
      processNextInQueue();
    }
  }, [processNextInQueue]);

  const handleIntersections = useCallback((entries: IntersectionObserverEntry[]) => {
    const toLoad: Array<{ element: HTMLElement; ratio: number }> = [];
    const toCancel: HTMLElement[] = [];

    for (const entry of entries) {
      const element = entry.target as HTMLElement;
      if (!element.isConnected) {
        observerRef.current?.unobserve(element);
        continue;
      }

      if (entry.isIntersecting) {
        if (element.dataset.thumbState !== 'loaded' && element.dataset.thumbState !== 'loading') {
          toLoad.push({ element, ratio: entry.intersectionRatio });
        }
      } else if (element.dataset.thumbState === 'loading') {
        toCancel.push(element);
      }
    }

    for (const element of toCancel) cancelLoad(element);

    toLoad
      .sort((a, b) => b.ratio - a.ratio)
      .forEach(({ element, ratio }) => scheduleLoad(element, ratio));
  }, [cancelLoad, scheduleLoad]);

  useEffect(() => {
    const nextRoot = scrollRootRef?.current ?? window;
    if (rootRef.current === nextRoot && observerRef.current) return;

    observerRef.current?.disconnect();
    rootRef.current = nextRoot;
    observerRef.current = new IntersectionObserver(handleIntersections, {
      root: nextRoot instanceof HTMLElement ? nextRoot : null,
      rootMargin: ROOT_MARGIN,
      threshold: THRESHOLD,
    });

    for (const element of observedRef.current) {
      observerRef.current.observe(element);
    }
  });

  useEffect(() => () => {
    observerRef.current?.disconnect();
    observerRef.current = null;
  }, []);

  const observeTile = useCallback((element: HTMLElement | null) => {
    if (!element) return;
    observedRef.current.add(element);
    const src = getTileSrc(element);
    if (src && thumbReadyCache.has(src)) {
      markLoaded(element, src, wasThumbLoadedFromCache(src));
      return;
    }
    setTileState(element, 'idle');
    clearCacheState(element);
    observerRef.current?.observe(element);
  }, []);

  const unobserveTile = useCallback((element: HTMLElement | null) => {
    if (!element) return;
    observedRef.current.delete(element);
    observerRef.current?.unobserve(element);
    cancelLoad(element);
  }, [cancelLoad]);

  const handleImageLoad = useCallback((event: SyntheticEvent<HTMLImageElement>) => {
    const img = event.target as HTMLImageElement;
    const element = img.closest('[data-hash]') as HTMLElement | null;
    if (!element) return;

    const id = getTileId(element);
    if (id) activeLoadsRef.current.delete(id);
    markLoaded(element, img.src, false);
    observerRef.current?.unobserve(element);
    processNextInQueue();
  }, [processNextInQueue]);

  const handleImageError = useCallback((event: SyntheticEvent<HTMLImageElement>) => {
    const img = event.target as HTMLImageElement;
    const element = img.closest('[data-hash]') as HTMLElement | null;
    if (!element) return;

    const id = getTileId(element);
    if (id) activeLoadsRef.current.delete(id);
    activatedThumbCache.delete(img.dataset.src ?? img.src);
    setTileState(element, 'error');
    clearCacheState(element);
    observerRef.current?.unobserve(element);
    processNextInQueue();
  }, [processNextInQueue]);

  return useMemo(() => ({
    observeTile,
    unobserveTile,
    handleImageLoad,
    handleImageError,
  }), [handleImageError, handleImageLoad, observeTile, unobserveTile]);
}
