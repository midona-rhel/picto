import { useCallback, useEffect, useRef } from 'react';

import type { GridRuntimeAction } from '../runtime';
import type { LayoutItem } from '../VirtualGrid';
import type { MasonryImageItem } from '../shared';

export const MARQUEE_BUCKET_SIZE = 256;

export interface MarqueeRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface MarqueeTile {
  hash: string;
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface UseGridMarqueeSelectionArgs {
  boxActive: boolean;
  dispatch: React.Dispatch<GridRuntimeAction>;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  getCanvasOffsetTop: () => number;
  imagesRef: React.MutableRefObject<MasonryImageItem[]>;
}

export interface UseGridMarqueeSelectionResult {
  handleBoxPointerDown: (e: React.PointerEvent) => void;
  marqueeRectRef: React.MutableRefObject<MarqueeRect | null>;
  marqueeHitHashesRef: React.MutableRefObject<Set<string> | null>;
  scheduleRedrawRef: React.MutableRefObject<(() => void) | null>;
  canvasLayoutRef: React.MutableRefObject<LayoutItem[]>;
}

export function buildMarqueeTileCache(
  positions: LayoutItem[],
  images: MasonryImageItem[],
  bucketSize: number = MARQUEE_BUCKET_SIZE,
): {
  tiles: MarqueeTile[];
  buckets: Map<number, number[]>;
} {
  const tiles: MarqueeTile[] = [];
  const buckets = new Map<number, number[]>();

  for (let i = 0; i < positions.length && i < images.length; i++) {
    const pos = positions[i];
    tiles.push({
      hash: images[i].hash,
      left: pos.x,
      top: pos.y,
      right: pos.x + pos.w,
      bottom: pos.y + pos.h,
    });

    const startBucket = Math.floor(pos.y / bucketSize);
    const endBucket = Math.floor((pos.y + pos.h) / bucketSize);
    for (let b = startBucket; b <= endBucket; b++) {
      const existing = buckets.get(b);
      if (existing) existing.push(i);
      else buckets.set(b, [i]);
    }
  }

  return { tiles, buckets };
}

export function collectMarqueeHitHashes(
  tiles: MarqueeTile[],
  buckets: Map<number, number[]>,
  rect: { left: number; top: number; right: number; bottom: number },
  bucketSize: number = MARQUEE_BUCKET_SIZE,
): string[] {
  const out: string[] = [];
  const seen = new Set<number>();
  const startBucket = Math.floor(rect.top / bucketSize);
  const endBucket = Math.floor(rect.bottom / bucketSize);

  for (let b = startBucket; b <= endBucket; b++) {
    const indices = buckets.get(b);
    if (!indices) continue;
    for (const idx of indices) {
      if (seen.has(idx)) continue;
      seen.add(idx);
      const tile = tiles[idx];
      if (!tile) continue;
      const hit =
        tile.right > rect.left &&
        tile.left < rect.right &&
        tile.bottom > rect.top &&
        tile.top < rect.bottom;
      if (hit) out.push(tile.hash);
    }
  }

  return out;
}

export function useGridMarqueeSelection({
  boxActive,
  dispatch,
  scrollRef,
  getCanvasOffsetTop,
  imagesRef,
}: UseGridMarqueeSelectionArgs): UseGridMarqueeSelectionResult {
  const tileCacheRef = useRef<MarqueeTile[]>([]);
  const tileBucketIndexRef = useRef<Map<number, number[]>>(new Map());
  const tileStampRef = useRef<Uint32Array>(new Uint32Array(0));
  const tileStampValueRef = useRef(1);
  const marqueeHitSetScratchRef = useRef<Set<string>>(new Set());
  const boxStateRef = useRef<{ startX: number; startY: number; x: number; y: number } | null>(null);
  const rafIdRef = useRef<number>(0);
  const boxAutoScrollRafRef = useRef<number>(0);
  const boxHitHashesRef = useRef<string[]>([]);
  const boxPointerIdRef = useRef<number | null>(null);
  const boxPointerClientRef = useRef<{ clientX: number; clientY: number } | null>(null);

  const marqueeRectRef = useRef<MarqueeRect | null>(null);
  const marqueeHitHashesRef = useRef<Set<string> | null>(null);
  const scheduleRedrawRef = useRef<(() => void) | null>(null);
  const canvasLayoutRef = useRef<LayoutItem[]>([]);

  const snapshotTilesFromLayout = useCallback(() => {
    const { tiles, buckets } = buildMarqueeTileCache(canvasLayoutRef.current, imagesRef.current);
    tileCacheRef.current = tiles;
    tileBucketIndexRef.current = buckets;

    if (tileStampRef.current.length < tiles.length) {
      tileStampRef.current = new Uint32Array(tiles.length);
      tileStampValueRef.current = 1;
    }
  }, [imagesRef]);

  const handleBoxPointerDown = useCallback((e: React.PointerEvent) => {
    if (!e.isPrimary || e.button !== 0) return;

    const target = e.target as HTMLElement;
    if (target.closest('[data-subfolder-grid]')) return;

    const container = scrollRef.current;
    if (!container) return;
    const containerRect = container.getBoundingClientRect();
    const canvasOffsetTop = getCanvasOffsetTop();

    e.preventDefault();
    try {
      container.setPointerCapture(e.pointerId);
      boxPointerIdRef.current = e.pointerId;
    } catch {
      boxPointerIdRef.current = null;
    }

    dispatch({ type: 'CLEAR_SELECTION' });

    snapshotTilesFromLayout();
    boxHitHashesRef.current = [];

    const x = e.clientX - containerRect.left + container.scrollLeft;
    const y = e.clientY - containerRect.top + container.scrollTop - canvasOffsetTop;
    boxPointerClientRef.current = { clientX: e.clientX, clientY: e.clientY };
    boxStateRef.current = { startX: x, startY: y, x, y };
    dispatch({ type: 'SET_BOX_ACTIVE', active: true });
  }, [dispatch, getCanvasOffsetTop, scrollRef, snapshotTilesFromLayout]);

  useEffect(() => {
    if (!boxActive) return;
    const container = scrollRef.current;
    if (!container) return;

    let lastVisualUpdate = 0;
    const VISUAL_INTERVAL_MS = 16;

    const updateOverlayAndSelection = () => {
      const now = performance.now();
      if (now - lastVisualUpdate < VISUAL_INTERVAL_MS) return;
      lastVisualUpdate = now;

      const bs = boxStateRef.current;
      if (!bs) return;

      const left = Math.min(bs.startX, bs.x);
      const top = Math.min(bs.startY, bs.y);
      const right = Math.max(bs.startX, bs.x);
      const bottom = Math.max(bs.startY, bs.y);

      marqueeRectRef.current = { left, top, width: right - left, height: bottom - top };

      const hitHashes = boxHitHashesRef.current;
      hitHashes.length = 0;
      const hitSet = marqueeHitSetScratchRef.current;
      hitSet.clear();

      const buckets = tileBucketIndexRef.current;
      const tiles = tileCacheRef.current;
      const stamps = tileStampRef.current;
      let stamp = tileStampValueRef.current + 1;
      if (stamp === 0xffffffff) {
        stamps.fill(0);
        stamp = 1;
      }
      tileStampValueRef.current = stamp;

      const startBucket = Math.floor(top / MARQUEE_BUCKET_SIZE);
      const endBucket = Math.floor(bottom / MARQUEE_BUCKET_SIZE);
      for (let b = startBucket; b <= endBucket; b++) {
        const indices = buckets.get(b);
        if (!indices) continue;
        for (let j = 0; j < indices.length; j++) {
          const idx = indices[j];
          if (stamps[idx] === stamp) continue;
          stamps[idx] = stamp;
          const tile = tiles[idx];
          if (!tile) continue;
          const isHit =
            tile.right > left && tile.left < right && tile.bottom > top && tile.top < bottom;
          if (isHit) {
            hitHashes.push(tile.hash);
            hitSet.add(tile.hash);
          }
        }
      }
      marqueeHitHashesRef.current = hitSet;

      scheduleRedrawRef.current?.();
    };

    const queueSelectionFrame = () => {
      if (!rafIdRef.current) {
        rafIdRef.current = requestAnimationFrame(() => {
          rafIdRef.current = 0;
          updateOverlayAndSelection();
        });
      }
    };

    const handleMove = (e: PointerEvent) => {
      if (boxPointerIdRef.current != null && e.pointerId !== boxPointerIdRef.current) return;
      boxPointerClientRef.current = { clientX: e.clientX, clientY: e.clientY };
      const containerRect = container.getBoundingClientRect();
      const canvasOffsetTop = getCanvasOffsetTop();
      const x = e.clientX - containerRect.left + container.scrollLeft;
      const y = e.clientY - containerRect.top + container.scrollTop - canvasOffsetTop;
      boxStateRef.current = { ...boxStateRef.current!, x, y };

      queueSelectionFrame();
    };

    const handleScroll = () => {
      if (!boxStateRef.current) return;
      queueSelectionFrame();
    };

    const EDGE_THRESHOLD_PX = 72;
    const MAX_SCROLL_PX_PER_FRAME = 28;
    const autoScrollTick = () => {
      boxAutoScrollRafRef.current = 0;
      if (!boxStateRef.current || !boxPointerClientRef.current) return;

      const rect = container.getBoundingClientRect();
      const { clientX, clientY } = boxPointerClientRef.current;

      let dx = 0;
      let dy = 0;

      if (clientX < rect.left + EDGE_THRESHOLD_PX) {
        const closeness = 1 - (clientX - rect.left) / EDGE_THRESHOLD_PX;
        dx = -Math.ceil(Math.max(0, closeness) * MAX_SCROLL_PX_PER_FRAME);
      } else if (clientX > rect.right - EDGE_THRESHOLD_PX) {
        const closeness = 1 - (rect.right - clientX) / EDGE_THRESHOLD_PX;
        dx = Math.ceil(Math.max(0, closeness) * MAX_SCROLL_PX_PER_FRAME);
      }

      if (clientY < rect.top + EDGE_THRESHOLD_PX) {
        const closeness = 1 - (clientY - rect.top) / EDGE_THRESHOLD_PX;
        dy = -Math.ceil(Math.max(0, closeness) * MAX_SCROLL_PX_PER_FRAME);
      } else if (clientY > rect.bottom - EDGE_THRESHOLD_PX) {
        const closeness = 1 - (rect.bottom - clientY) / EDGE_THRESHOLD_PX;
        dy = Math.ceil(Math.max(0, closeness) * MAX_SCROLL_PX_PER_FRAME);
      }

      let scrolled = false;
      if (dx !== 0 || dy !== 0) {
        const prevLeft = container.scrollLeft;
        const prevTop = container.scrollTop;

        if (dx !== 0) {
          const maxLeft = Math.max(0, container.scrollWidth - container.clientWidth);
          container.scrollLeft = Math.max(0, Math.min(maxLeft, container.scrollLeft + dx));
        }
        if (dy !== 0) {
          const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
          container.scrollTop = Math.max(0, Math.min(maxTop, container.scrollTop + dy));
        }

        scrolled = container.scrollLeft !== prevLeft || container.scrollTop !== prevTop;
      }

      if (scrolled) {
        const canvasOffsetTop = getCanvasOffsetTop();
        const newX = clientX - rect.left + container.scrollLeft;
        const newY = clientY - rect.top + container.scrollTop - canvasOffsetTop;
        boxStateRef.current = { ...boxStateRef.current, x: newX, y: newY };
        queueSelectionFrame();
      }

      if (boxStateRef.current) {
        boxAutoScrollRafRef.current = requestAnimationFrame(autoScrollTick);
      }
    };
    boxAutoScrollRafRef.current = requestAnimationFrame(autoScrollTick);

    const handleUp = (e?: PointerEvent) => {
      if (e && boxPointerIdRef.current != null && e.pointerId !== boxPointerIdRef.current) return;
      if (rafIdRef.current) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = 0;
      }
      if (boxAutoScrollRafRef.current) {
        cancelAnimationFrame(boxAutoScrollRafRef.current);
        boxAutoScrollRafRef.current = 0;
      }

      const finalHashes = boxHitHashesRef.current;
      if (finalHashes.length > 0) {
        dispatch({ type: 'SELECT_HASHES', hashes: new Set(finalHashes) });
      }

      boxStateRef.current = null;
      tileCacheRef.current = [];
      boxHitHashesRef.current = [];
      boxPointerClientRef.current = null;
      marqueeRectRef.current = null;
      marqueeHitHashesRef.current = null;
      scheduleRedrawRef.current?.();
      if (boxPointerIdRef.current != null) {
        try {
          if (container.hasPointerCapture(boxPointerIdRef.current)) {
            container.releasePointerCapture(boxPointerIdRef.current);
          }
        } catch {
          // ignore
        }
      }
      boxPointerIdRef.current = null;
      dispatch({ type: 'SET_BOX_ACTIVE', active: false });
    };

    container.addEventListener('pointermove', handleMove);
    container.addEventListener('pointerup', handleUp);
    container.addEventListener('pointercancel', handleUp);
    container.addEventListener('scroll', handleScroll, { passive: true });

    return () => {
      container.removeEventListener('pointermove', handleMove);
      container.removeEventListener('pointerup', handleUp);
      container.removeEventListener('pointercancel', handleUp);
      container.removeEventListener('scroll', handleScroll);
      if (rafIdRef.current) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = 0;
      }
      if (boxAutoScrollRafRef.current) {
        cancelAnimationFrame(boxAutoScrollRafRef.current);
        boxAutoScrollRafRef.current = 0;
      }
      boxPointerClientRef.current = null;
      boxPointerIdRef.current = null;
    };
  }, [boxActive, dispatch, getCanvasOffsetTop, scrollRef]);

  return {
    handleBoxPointerDown,
    marqueeRectRef,
    marqueeHitHashesRef,
    scheduleRedrawRef,
    canvasLayoutRef,
  };
}
