import { useCallback, useEffect, useRef } from 'react';

import type { GridRuntimeAction } from '../runtime';
import type { LayoutItem } from '../gridLayout';
import type { MasonryItem } from '../shared';

export interface MarqueeRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface UseGridMarqueeSelectionArgs {
  boxActive: boolean;
  dispatch: React.Dispatch<GridRuntimeAction>;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  getCanvasOffsetTop: () => number;
  imagesRef: React.MutableRefObject<MasonryItem[]>;
  selectedHashesRef: React.MutableRefObject<Set<string>>;
}

export interface UseGridMarqueeSelectionResult {
  handleBoxPointerDown: (e: React.PointerEvent) => void;
  marqueeRectRef: React.MutableRefObject<MarqueeRect | null>;
  marqueeHitHashesRef: React.MutableRefObject<Set<string> | null>;
  scheduleRedrawRef: React.MutableRefObject<(() => void) | null>;
  canvasLayoutRef: React.MutableRefObject<LayoutItem[]>;
}

const EDGE_PX = 50;
const SCROLL_PER_FRAME = 12;

export function useGridMarqueeSelection({
  boxActive,
  dispatch,
  scrollRef,
  getCanvasOffsetTop,
  imagesRef,
  selectedHashesRef,
}: UseGridMarqueeSelectionArgs): UseGridMarqueeSelectionResult {
  const boxStateRef = useRef<{ startX: number; startY: number; x: number; y: number } | null>(null);
  const rafRef = useRef(0);
  const scrollRafRef = useRef(0);
  const pointerIdRef = useRef<number | null>(null);
  const pointerClientRef = useRef<{ clientX: number; clientY: number } | null>(null);
  const priorSelectionRef = useRef<Set<string> | null>(null);

  const marqueeRectRef = useRef<MarqueeRect | null>(null);
  const marqueeHitHashesRef = useRef<Set<string> | null>(null);
  const scheduleRedrawRef = useRef<(() => void) | null>(null);
  const canvasLayoutRef = useRef<LayoutItem[]>([]);

  /* ---- hit-test all tiles via simple AABB ---- */
  const computeHits = useCallback(() => {
    const bs = boxStateRef.current;
    if (!bs) return;

    const left = Math.min(bs.startX, bs.x);
    const top = Math.min(bs.startY, bs.y);
    const right = Math.max(bs.startX, bs.x);
    const bottom = Math.max(bs.startY, bs.y);

    marqueeRectRef.current = { left, top, width: right - left, height: bottom - top };

    const positions = canvasLayoutRef.current;
    const images = imagesRef.current;
    const hits = new Set<string>();

    const len = Math.min(positions.length, images.length);
    for (let i = 0; i < len; i++) {
      const p = positions[i];
      if (
        p.x + p.w > left &&
        p.x < right &&
        p.y + p.h > top &&
        p.y < bottom
      ) {
        hits.add(images[i].hash);
      }
    }

    marqueeHitHashesRef.current = hits;
    scheduleRedrawRef.current?.();
  }, [imagesRef]);

  /* ---- pointerdown: record origin, activate marquee ---- */
  const handleBoxPointerDown = useCallback((e: React.PointerEvent) => {
    if (!e.isPrimary || e.button !== 0) return;
    if ((e.target as HTMLElement).closest('[data-subfolder-grid]')) return;
    // Don't capture clicks on interactive elements (empty state buttons, etc.)
    if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return;

    const container = scrollRef.current;
    if (!container) return;
    const cr = container.getBoundingClientRect();
    const offsetTop = getCanvasOffsetTop();

    e.preventDefault();
    try {
      container.setPointerCapture(e.pointerId);
      pointerIdRef.current = e.pointerId;
    } catch {
      pointerIdRef.current = null;
    }

    if (e.metaKey || e.ctrlKey) {
      priorSelectionRef.current = new Set(selectedHashesRef.current);
    } else {
      priorSelectionRef.current = null;
      dispatch({ type: 'CLEAR_SELECTION' });
    }

    const x = e.clientX - cr.left + container.scrollLeft;
    const y = e.clientY - cr.top + container.scrollTop - offsetTop;
    pointerClientRef.current = { clientX: e.clientX, clientY: e.clientY };
    boxStateRef.current = { startX: x, startY: y, x, y };
    dispatch({ type: 'SET_BOX_ACTIVE', active: true });
  }, [dispatch, getCanvasOffsetTop, scrollRef, selectedHashesRef]);

  /* ---- effect: wire move / up / auto-scroll while active ---- */
  useEffect(() => {
    if (!boxActive) return;
    const container = scrollRef.current;
    if (!container) return;

    const scheduleHitTest = () => {
      if (!rafRef.current) {
        rafRef.current = requestAnimationFrame(() => {
          rafRef.current = 0;
          computeHits();
        });
      }
    };

    const handleMove = (e: PointerEvent) => {
      if (pointerIdRef.current != null && e.pointerId !== pointerIdRef.current) return;
      pointerClientRef.current = { clientX: e.clientX, clientY: e.clientY };
      const cr = container.getBoundingClientRect();
      const offsetTop = getCanvasOffsetTop();
      const x = e.clientX - cr.left + container.scrollLeft;
      const y = e.clientY - cr.top + container.scrollTop - offsetTop;
      boxStateRef.current = { ...boxStateRef.current!, x, y };
      scheduleHitTest();
    };

    const handleScroll = () => {
      if (boxStateRef.current) scheduleHitTest();
    };

    /* simple auto-scroll: fixed px per frame when near edge */
    const autoScrollTick = () => {
      scrollRafRef.current = 0;
      if (!boxStateRef.current || !pointerClientRef.current) return;

      const cr = container.getBoundingClientRect();
      const { clientY } = pointerClientRef.current;
      let dy = 0;

      if (clientY < cr.top + EDGE_PX) dy = -SCROLL_PER_FRAME;
      else if (clientY > cr.bottom - EDGE_PX) dy = SCROLL_PER_FRAME;

      if (dy !== 0) {
        const prev = container.scrollTop;
        const max = Math.max(0, container.scrollHeight - container.clientHeight);
        container.scrollTop = Math.max(0, Math.min(max, container.scrollTop + dy));
        if (container.scrollTop !== prev) {
          const offsetTop = getCanvasOffsetTop();
          const { clientX } = pointerClientRef.current;
          const newX = clientX - cr.left + container.scrollLeft;
          const newY = clientY - cr.top + container.scrollTop - offsetTop;
          boxStateRef.current = { ...boxStateRef.current, x: newX, y: newY };
          scheduleHitTest();
        }
      }

      if (boxStateRef.current) {
        scrollRafRef.current = requestAnimationFrame(autoScrollTick);
      }
    };
    scrollRafRef.current = requestAnimationFrame(autoScrollTick);

    const handleUp = (e?: PointerEvent) => {
      if (e && pointerIdRef.current != null && e.pointerId !== pointerIdRef.current) return;

      if (rafRef.current) { cancelAnimationFrame(rafRef.current); rafRef.current = 0; }
      if (scrollRafRef.current) { cancelAnimationFrame(scrollRafRef.current); scrollRafRef.current = 0; }

      const hits = marqueeHitHashesRef.current;
      if (hits && hits.size > 0) {
        const prior = priorSelectionRef.current;
        if (prior && prior.size > 0) {
          const merged = new Set(prior);
          for (const h of hits) merged.add(h);
          dispatch({ type: 'SELECT_HASHES', hashes: merged });
        } else {
          dispatch({ type: 'SELECT_HASHES', hashes: hits });
        }
      }

      boxStateRef.current = null;
      pointerClientRef.current = null;
      priorSelectionRef.current = null;
      marqueeRectRef.current = null;
      marqueeHitHashesRef.current = null;
      scheduleRedrawRef.current?.();

      if (pointerIdRef.current != null) {
        try {
          if (container.hasPointerCapture(pointerIdRef.current)) {
            container.releasePointerCapture(pointerIdRef.current);
          }
        } catch { /* ignore */ }
      }
      pointerIdRef.current = null;
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
      if (rafRef.current) { cancelAnimationFrame(rafRef.current); rafRef.current = 0; }
      if (scrollRafRef.current) { cancelAnimationFrame(scrollRafRef.current); scrollRafRef.current = 0; }
      pointerClientRef.current = null;
      pointerIdRef.current = null;
    };
  }, [boxActive, computeHits, dispatch, getCanvasOffsetTop, scrollRef]);

  return {
    handleBoxPointerDown,
    marqueeRectRef,
    marqueeHitHashesRef,
    scheduleRedrawRef,
    canvasLayoutRef,
  };
}
