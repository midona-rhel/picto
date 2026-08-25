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

interface ImageZoomOptions {
  macTrackpadGestures?: boolean;
}

const MIN_SCALE = 0.05;
const MAX_SCALE = 8.0;
const INTERACTIVE_COMMIT_MS = 96;

export function useImageZoom(
  containerRef: RefObject<HTMLDivElement | null>,
  imageSize: ImageSize | null,
  transformTargets: Array<RefObject<HTMLElement | null>> = [],
  options: ImageZoomOptions = {},
) {
  const [state, setState] = useState<ZoomState>({ scale: 1, tx: 0, ty: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);
  const liveStateRef = useRef(state);
  const transformTargetsRef = useRef(transformTargets);
  transformTargetsRef.current = transformTargets;
  const commitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const frameRef = useRef<number | null>(null);

  /** Invoked every interactive frame — used by navigator renderer. */
  const onLiveFrameRef = useRef<((s: ZoomState) => void) | null>(null);
  /** Second callback for live zoom updates (e.g. toolbar slider). */
  const onLiveScaleRef = useRef<((s: ZoomState) => void) | null>(null);

  const applyTransform = useCallback((next: ZoomState) => {
    const transform = `translate(calc(-50% + ${next.tx}px), calc(-50% + ${next.ty}px)) scale(${next.scale})`;
    for (const targetRef of transformTargetsRef.current) {
      const el = targetRef.current;
      if (el) el.style.transform = transform;
    }
    onLiveFrameRef.current?.(next);
    onLiveScaleRef.current?.(next);
  }, []);

  const subscribeLiveScale = useCallback((listener: (scale: number) => void) => {
    const callback = (next: ZoomState) => listener(next.scale);
    onLiveScaleRef.current = callback;
    listener(liveStateRef.current.scale);
    return () => {
      if (onLiveScaleRef.current === callback) onLiveScaleRef.current = null;
    };
  }, []);

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
    (
      nextOrUpdater: ZoomState | ((prev: ZoomState) => ZoomState),
      interactive = false,
      alreadyInAnimationFrame = false,
    ) => {
      const prev = liveStateRef.current;
      const next = typeof nextOrUpdater === 'function' ? nextOrUpdater(prev) : nextOrUpdater;
      liveStateRef.current = next;

      if (!interactive) {
        applyTransform(next);
        flushCommittedState(next);
        return;
      }

      if (alreadyInAnimationFrame) applyTransform(next);
      else scheduleInteractiveTransform();
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

  // ── Animated zoom (smooth transition for keyboard +/- and discrete scroll) ──
  const animTargetRef = useRef<{ scale: number; tx: number; ty: number } | null>(null);
  const animStartRef = useRef<{ scale: number; tx: number; ty: number } | null>(null);
  const animStartTimeRef = useRef(0);
  const animRafRef = useRef<number | null>(null);
  const ANIM_DURATION_MS = 150;

  const animateZoomTo = useCallback(
    (targetScale: number, focalX?: number, focalY?: number) => {
      // The target accumulates on each call (rapid +++ → 1.25 → 1.56 → 1.95).
      // The animation always lerps from wherever we are NOW to the new target,
      // resetting the timer so we always have ANIM_DURATION_MS left to arrive.
      const cur = liveStateRef.current;
      const pendingTarget = animTargetRef.current;

      // Rebase: caller passes committedState * multiplier, but we want
      // pendingTarget * multiplier so rapid presses accumulate.
      let effectiveTargetScale = targetScale;
      if (pendingTarget && focalX === undefined) {
        const committed = state.scale;
        if (committed > 0) {
          const multiplier = targetScale / committed;
          effectiveTargetScale = pendingTarget.scale * multiplier;
        }
      }
      const clamped = Math.min(MAX_SCALE, Math.max(MIN_SCALE, effectiveTargetScale));
      const base = pendingTarget ?? cur;
      let target: ZoomState;
      if (focalX !== undefined && focalY !== undefined) {
        const ratio = clamped / cur.scale;
        target = {
          scale: clamped,
          tx: focalX - ratio * (focalX - cur.tx),
          ty: focalY - ratio * (focalY - cur.ty),
        };
      } else {
        const ratio = clamped / base.scale;
        target = { scale: clamped, tx: base.tx * ratio, ty: base.ty * ratio };
      }

      // Start from current position, reset timer — always ANIM_DURATION_MS from now
      animStartRef.current = { ...cur };
      animTargetRef.current = target;
      animStartTimeRef.current = performance.now();

      if (animRafRef.current != null) return; // already running
      const tick = () => {
        const start = animStartRef.current;
        const end = animTargetRef.current;
        if (!start || !end) { animRafRef.current = null; return; }

        const elapsed = performance.now() - animStartTimeRef.current;
        const t = Math.min(1, elapsed / ANIM_DURATION_MS);
        // Ease out cubic
        const ease = 1 - Math.pow(1 - t, 3);

        const s: ZoomState = {
          scale: start.scale + (end.scale - start.scale) * ease,
          tx: start.tx + (end.tx - start.tx) * ease,
          ty: start.ty + (end.ty - start.ty) * ease,
        };

        // The animation tick already runs inside RAF; apply on this frame
        // instead of scheduling a second RAF and displaying one frame late.
        updateZoomState(s, true, true);

        if (t < 1) {
          animRafRef.current = requestAnimationFrame(tick);
        } else {
          animRafRef.current = null;
          animStartRef.current = null;
          animTargetRef.current = null;
          updateZoomState(end);
        }
      };
      animRafRef.current = requestAnimationFrame(tick);
    },
    [updateZoomState, state.scale],
  );

  // ── Wheel zoom (non-passive for Mac trackpad) ──
  // Trackpad sends many small deltaY events (smooth already).
  // Discrete mouse wheel sends large deltaY — we animate those.
  const wheelAccumRef = useRef({ targetScale: 0, focalX: 0, focalY: 0, timer: null as ReturnType<typeof setTimeout> | null });

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const useMacTrackpadGestures = options.macTrackpadGestures === true
      && typeof navigator !== 'undefined'
      && /Mac|iPhone|iPad/.test(navigator.platform);

    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (useMacTrackpadGestures && !e.ctrlKey) {
        updateZoomState((prev) => ({
          ...prev,
          tx: prev.tx - e.deltaX,
          ty: prev.ty - e.deltaY,
        }), true);
        return;
      }
      const rect = container.getBoundingClientRect();
      const focalX = e.clientX - rect.left - rect.width / 2;
      const focalY = e.clientY - rect.top - rect.height / 2;

      // Trackpad: deltaMode 0, small deltaY. Mouse wheel: deltaMode 0 but large deltaY.
      const isDiscrete = Math.abs(e.deltaY) >= 40 && !e.ctrlKey;

      if (isDiscrete) {
        // Accumulate into animated target
        const acc = wheelAccumRef.current;
        const base = acc.timer ? acc.targetScale : liveStateRef.current.scale;
        const direction = e.deltaY > 0 ? 1 / 1.25 : 1.25;
        acc.targetScale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, base * direction));
        acc.focalX = focalX;
        acc.focalY = focalY;
        if (acc.timer) clearTimeout(acc.timer);
        animateZoomTo(acc.targetScale, focalX, focalY);
        acc.timer = setTimeout(() => { acc.timer = null; }, 200);
      } else {
        // Smooth trackpad — apply directly per event
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
      }
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [containerRef, options.macTrackpadGestures, updateZoomState, animateZoomTo]);

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
        tx: (0.5 - nx) * imageSize.width * prev.scale,
        ty: (0.5 - ny) * imageSize.height * prev.scale,
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
      if (animRafRef.current != null) cancelAnimationFrame(animRafRef.current);
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
    animateZoomTo,
    navigatorRect,
    panToNormalized,
    onLiveFrameRef,
    subscribeLiveScale,
    handlers: { onMouseDown },
  };
}
