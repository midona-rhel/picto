import { useCallback, useRef, useState, useEffect, useLayoutEffect, useMemo } from 'react';
import { useGlobalPointerDrag } from '../../shared/hooks/useGlobalPointerDrag';

export interface ZoomState {
  scale: number;
  tx: number;
  ty: number;
}

export interface ImageSize {
  width: number;
  height: number;
}

export interface NavigatorRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface UseImageZoomOptions {
  minScale?: number;
  maxScale?: number;
}

const MIN_SCALE = 0.05;
const MAX_SCALE = 8.0;

export function useImageZoom(
  containerRef: React.RefObject<HTMLDivElement | null>,
  imageSize: ImageSize | null,
  options: UseImageZoomOptions = {},
) {
  const { minScale = MIN_SCALE, maxScale = MAX_SCALE } = options;

  const [state, setState] = useState<ZoomState>({ scale: 1, tx: 0, ty: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number; tx: number; ty: number } | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;

  // Cache container dimensions — avoids DOM reads during zoom frames.
  // useLayoutEffect for the initial measurement so containerSize is available
  // before useEffect callbacks (e.g. useZoomCache) read it via calcFitScale.
  const [containerSize, setContainerSize] = useState({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    setContainerSize({ w: container.clientWidth, h: container.clientHeight });
  }, [containerRef]);
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const ro = new ResizeObserver(() => {
      setContainerSize({ w: container.clientWidth, h: container.clientHeight });
    });
    ro.observe(container);
    return () => ro.disconnect();
  }, [containerRef]);

  const getFitScale = useCallback(() => {
    if (!imageSize || containerSize.w === 0) return 1;
    return Math.min(containerSize.w / imageSize.width, containerSize.h / imageSize.height, 1);
  }, [containerSize, imageSize]);

  // Calculate fit scale for an arbitrary image size (used during navigation before imageSize updates)
  const calcFitScale = useCallback((imgSize: ImageSize) => {
    if (containerSize.w === 0) return 1;
    return Math.min(containerSize.w / imgSize.width, containerSize.h / imgSize.height, 1);
  }, [containerSize]);

  const fitToWindow = useCallback(() => {
    setState({ scale: getFitScale(), tx: 0, ty: 0 });
  }, [getFitScale]);

  const fitActual = useCallback(() => {
    setState({ scale: 1, tx: 0, ty: 0 });
  }, []);

  const zoomTo = useCallback((targetScale: number, focalX?: number, focalY?: number) => {
    setState(prev => {
      const clamped = Math.min(maxScale, Math.max(minScale, targetScale));
      if (focalX !== undefined && focalY !== undefined) {
        const ratio = clamped / prev.scale;
        return { scale: clamped, tx: focalX - ratio * (focalX - prev.tx), ty: focalY - ratio * (focalY - prev.ty) };
      }
      return { ...prev, scale: clamped };
    });
  }, [minScale, maxScale]);

  // Native non-passive wheel listener for smooth Mac trackpad zoom.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();

      const rect = container.getBoundingClientRect();
      const focalX = e.clientX - rect.left - rect.width / 2;
      const focalY = e.clientY - rect.top - rect.height / 2;

      const sensitivity = 0.004;
      const multiplier = Math.exp(-e.deltaY * sensitivity);

      setState(prev => {
        const newScale = Math.min(maxScale, Math.max(minScale, prev.scale * multiplier));
        const ratio = newScale / prev.scale;
        return {
          scale: newScale,
          tx: focalX - ratio * (focalX - prev.tx),
          ty: focalY - ratio * (focalY - prev.ty),
        };
      });
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [containerRef, minScale, maxScale]);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    setIsDragging(true);
    const cur = stateRef.current;
    dragStartRef.current = { x: e.clientX, y: e.clientY, tx: cur.tx, ty: cur.ty };
  }, []);

  const handleDragMove = useCallback((e: MouseEvent) => {
    const start = dragStartRef.current;
    if (!start) return;
    setState(prev => ({ ...prev, tx: start.tx + (e.clientX - start.x), ty: start.ty + (e.clientY - start.y) }));
  }, []);
  const handleDragEnd = useCallback(() => {
    setIsDragging(false);
    dragStartRef.current = null;
  }, []);
  useGlobalPointerDrag({ onMove: handleDragMove, onEnd: handleDragEnd }, isDragging);

  // Navigator rect: pure computation from cached dimensions — no DOM reads
  const navigatorRect: NavigatorRect | null = useMemo(() => {
    if (!imageSize || containerSize.w === 0) return null;
    const cw = containerSize.w;
    const ch = containerSize.h;
    if (imageSize.width * state.scale < cw + 1 && imageSize.height * state.scale < ch + 1) return null;

    const viewW = cw / state.scale;
    const viewH = ch / state.scale;
    const cx = (cw / 2 - state.tx) / state.scale;
    const cy = (ch / 2 - state.ty) / state.scale;

    const x = (cx - viewW / 2) / imageSize.width;
    const y = (cy - viewH / 2) / imageSize.height;
    const w = viewW / imageSize.width;
    const h = viewH / imageSize.height;

    return {
      x: Math.max(0, Math.min(1 - w, x)),
      y: Math.max(0, Math.min(1 - h, y)),
      w: Math.min(1, w),
      h: Math.min(1, h),
    };
  }, [imageSize, containerSize, state.scale, state.tx, state.ty]);

  const panToNormalized = useCallback((nx: number, ny: number) => {
    if (!imageSize || containerSize.w === 0) return;
    setState(prev => ({
      ...prev,
      tx: containerSize.w / 2 - nx * imageSize.width * prev.scale,
      ty: containerSize.h / 2 - ny * imageSize.height * prev.scale,
    }));
  }, [containerSize, imageSize]);

  return {
    state,
    setState,
    isDragging,
    getFitScale,
    calcFitScale,
    containerSize,
    fitToWindow,
    fitActual,
    zoomTo,
    navigatorRect,
    panToNormalized,
    handlers: { onMouseDown },
  };
}
