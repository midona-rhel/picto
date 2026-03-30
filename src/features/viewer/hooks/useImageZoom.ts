/**
 * useImageZoom — zoom/pan state for the media viewer.
 *
 * - Wheel zoom with focal point (trackpad-friendly, non-passive)
 * - Click-drag pan (grab → grabbing cursor)
 * - Scale range: 5% to 800%
 * - Fit-to-window and fit-actual (1:1)
 * - Per-image zoom cache (restores zoom on re-navigation)
 * - Interactive transform: direct DOM writes during drag/wheel,
 *   debounced React state commit (96ms) to avoid cascading renders.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import { computeNavigatorRect, type NavigatorRect } from './navigatorMath';

export interface ZoomState {
  scale: number;
  tx: number;
  ty: number;
}

export interface ImageSize {
  width: number;
  height: number;
}

const MIN_SCALE = 0.05;
const MAX_SCALE = 8.0;
const INTERACTIVE_COMMIT_MS = 96;

export function useImageZoom(
  containerRef: RefObject<HTMLDivElement | null>,
  imageSize: ImageSize | null,
  transformTargets: Array<RefObject<HTMLElement | null>> = [],
) {
  const [state, setState] = useState<ZoomState>({ scale: 1, tx: 0, ty: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);
  const liveStateRef = useRef(state);
  const commitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const frameRef = useRef<number | null>(null);

  /** Invoked every interactive frame — used by navigator renderer. */
  const onLiveFrameRef = useRef<((s: ZoomState) => void) | null>(null);
  /** Second callback for live zoom updates (e.g. toolbar slider). */
  const onLiveScaleRef = useRef<((s: ZoomState) => void) | null>(null);

  const applyTransform = useCallback((next: ZoomState) => {
    const transform = `translate(calc(-50% + ${next.tx}px), calc(-50% + ${next.ty}px)) scale(${next.scale})`;
    for (const targetRef of transformTargets) {
      const el = targetRef.current;
      if (el) el.style.transform = transform;
    }
    onLiveFrameRef.current?.(next);
    onLiveScaleRef.current?.(next);
  }, [transformTargets]);

  const flushCommittedState = useCallback((next?: ZoomState) => {
    const resolved = next ?? liveStateRef.current;
    if (commitTimerRef.current) {
      clearTimeout(commitTimerRef.current);
      commitTimerRef.current = null;
    }
    setState((prev) =>
      prev.scale === resolved.scale && prev.tx === resolved.tx && prev.ty === resolved.ty
        ? prev
        : resolved,
    );
  }, []);

  const scheduleInteractiveTransform = useCallback(() => {
    if (frameRef.current != null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      applyTransform(liveStateRef.current);
    });
  }, [applyTransform]);

  const updateZoomState = useCallback(
    (nextOrUpdater: ZoomState | ((prev: ZoomState) => ZoomState), interactive = false) => {
      const prev = liveStateRef.current;
      const next = typeof nextOrUpdater === 'function' ? nextOrUpdater(prev) : nextOrUpdater;
      liveStateRef.current = next;

      if (!interactive) {
        applyTransform(next);
        flushCommittedState(next);
        return;
      }

      scheduleInteractiveTransform();
      if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
      commitTimerRef.current = setTimeout(() => {
        commitTimerRef.current = null;
        flushCommittedState(liveStateRef.current);
      }, INTERACTIVE_COMMIT_MS);
    },
    [applyTransform, flushCommittedState, scheduleInteractiveTransform],
  );

  // ── Container size (cached to avoid DOM reads during zoom frames) ──
  const [containerSize, setContainerSize] = useState({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const el = containerRef.current;
    if (el) setContainerSize({ w: el.clientWidth, h: el.clientHeight });
  }, [containerRef]);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setContainerSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [containerRef]);

  const getFitScale = useCallback(() => {
    if (!imageSize || containerSize.w === 0) return 1;
    return Math.min(containerSize.w / imageSize.width, containerSize.h / imageSize.height);
  }, [containerSize, imageSize]);

  const calcFitScale = useCallback(
    (imgSize: ImageSize) => {
      if (containerSize.w === 0) return 1;
      return Math.min(containerSize.w / imgSize.width, containerSize.h / imgSize.height);
    },
    [containerSize],
  );

  const fitToWindow = useCallback(() => {
    updateZoomState({ scale: getFitScale(), tx: 0, ty: 0 });
  }, [getFitScale, updateZoomState]);

  const fitActual = useCallback(() => {
    updateZoomState({ scale: 1, tx: 0, ty: 0 });
  }, [updateZoomState]);

  const zoomTo = useCallback(
    (targetScale: number, focalX?: number, focalY?: number) => {
      updateZoomState((prev) => {
        const clamped = Math.min(MAX_SCALE, Math.max(MIN_SCALE, targetScale));
        if (focalX !== undefined && focalY !== undefined) {
          const ratio = clamped / prev.scale;
          return {
            scale: clamped,
            tx: focalX - ratio * (focalX - prev.tx),
            ty: focalY - ratio * (focalY - prev.ty),
          };
        }
        return { ...prev, scale: clamped };
      });
    },
    [updateZoomState],
  );

  // ── Wheel zoom (non-passive for Mac trackpad) ──
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = container.getBoundingClientRect();
      const focalX = e.clientX - rect.left - rect.width / 2;
      const focalY = e.clientY - rect.top - rect.height / 2;
      const multiplier = Math.exp(-e.deltaY * 0.004);

      updateZoomState((prev) => {
        const newScale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, prev.scale * multiplier));
        const ratio = newScale / prev.scale;
        return {
          scale: newScale,
          tx: focalX - ratio * (focalX - prev.tx),
          ty: focalY - ratio * (focalY - prev.ty),
        };
      }, true);
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [containerRef, updateZoomState]);

  // ── Click-drag pan ──
  const onMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    setIsDragging(true);
    const cur = liveStateRef.current;
    dragStartRef.current = { x: e.clientX, y: e.clientY, tx: cur.tx, ty: cur.ty };
  }, []);

  useEffect(() => {
    if (!isDragging) return;

    const handleMove = (e: MouseEvent) => {
      const start = dragStartRef.current;
      if (!start) return;
      updateZoomState(
        (prev) => ({ ...prev, tx: start.tx + (e.clientX - start.x), ty: start.ty + (e.clientY - start.y) }),
        true,
      );
    };
    const handleUp = () => {
      setIsDragging(false);
      dragStartRef.current = null;
    };

    window.addEventListener('mousemove', handleMove);
    window.addEventListener('mouseup', handleUp);
    return () => {
      window.removeEventListener('mousemove', handleMove);
      window.removeEventListener('mouseup', handleUp);
    };
  }, [isDragging, updateZoomState]);

  // ── Navigator rect (pure computation, no DOM reads) ──
  const navigatorRect: NavigatorRect | null = useMemo(
    () => {
      if (!imageSize || containerSize.w === 0) return null;
      return computeNavigatorRect(state, imageSize, containerSize);
    },
    [imageSize, containerSize, state],
  );

  const panToNormalized = useCallback(
    (nx: number, ny: number) => {
      if (!imageSize || containerSize.w === 0) return;
      updateZoomState((prev) => ({
        ...prev,
        tx: containerSize.w / 2 - nx * imageSize.width * prev.scale,
        ty: containerSize.h / 2 - ny * imageSize.height * prev.scale,
      }));
    },
    [containerSize, imageSize, updateZoomState],
  );

  // ── Sync DOM transforms on committed state ──
  useLayoutEffect(() => {
    if (commitTimerRef.current == null) {
      liveStateRef.current = state;
    }
    applyTransform(liveStateRef.current);
  }, [applyTransform, state]);

  // Cleanup
  useEffect(
    () => () => {
      if (commitTimerRef.current) clearTimeout(commitTimerRef.current);
      if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
    },
    [],
  );

  return {
    state,
    setState: updateZoomState,
    isDragging,
    getFitScale,
    calcFitScale,
    containerSize,
    fitToWindow,
    fitActual,
    zoomTo,
    navigatorRect,
    panToNormalized,
    onLiveFrameRef,
    onLiveScaleRef,
    handlers: { onMouseDown },
  };
}
