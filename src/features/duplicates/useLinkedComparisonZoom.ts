import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from 'react';

export type ComparisonSide = 'left' | 'right';

export interface ComparisonImageSize {
  width: number;
  height: number;
}

interface PaneSize {
  width: number;
  height: number;
}

interface LinkedComparisonZoomInput {
  leftContainerRef: RefObject<HTMLDivElement | null>;
  rightContainerRef: RefObject<HTMLDivElement | null>;
  leftImageSize: ComparisonImageSize | null;
  rightImageSize: ComparisonImageSize | null;
  pairKey: string;
}

const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function useLinkedComparisonZoom({
  leftContainerRef,
  rightContainerRef,
  leftImageSize,
  rightImageSize,
  pairKey,
}: LinkedComparisonZoomInput) {
  const [paneSizes, setPaneSizes] = useState<Record<ComparisonSide, PaneSize>>({
    left: { width: 0, height: 0 },
    right: { width: 0, height: 0 },
  });
  const [zoom, setZoom] = useState(1);
  const [center, setCenter] = useState({ x: 0.5, y: 0.5 });
  const [draggingSide, setDraggingSide] = useState<ComparisonSide | null>(null);
  const dragStartRef = useRef<{
    side: ComparisonSide;
    pointerX: number;
    pointerY: number;
    centerX: number;
    centerY: number;
  } | null>(null);

  const imageSizes = useMemo<Record<ComparisonSide, ComparisonImageSize | null>>(
    () => ({ left: leftImageSize, right: rightImageSize }),
    [leftImageSize, rightImageSize],
  );

  useLayoutEffect(() => {
    const elements = {
      left: leftContainerRef.current,
      right: rightContainerRef.current,
    };
    const measure = () => {
      setPaneSizes({
        left: {
          width: elements.left?.clientWidth ?? 0,
          height: elements.left?.clientHeight ?? 0,
        },
        right: {
          width: elements.right?.clientWidth ?? 0,
          height: elements.right?.clientHeight ?? 0,
        },
      });
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    if (elements.left) observer.observe(elements.left);
    if (elements.right) observer.observe(elements.right);
    return () => observer.disconnect();
  }, [leftContainerRef, pairKey, rightContainerRef]);

  const fitScales = useMemo<Record<ComparisonSide, number>>(() => {
    const scales: Record<ComparisonSide, number> = { left: 1, right: 1 };
    for (const side of ['left', 'right'] as const) {
      const pane = paneSizes[side];
      const image = imageSizes[side];
      if (pane.width > 0 && pane.height > 0 && image?.width && image.height) {
        scales[side] = Math.min(pane.width / image.width, pane.height / image.height);
      }
    }
    return scales;
  }, [imageSizes, paneSizes]);

  useLayoutEffect(() => {
    setZoom(1);
    setCenter({ x: 0.5, y: 0.5 });
  }, [pairKey]);

  const zoomTo = useCallback((nextZoom: number) => {
    setZoom(clamp(nextZoom, MIN_ZOOM, MAX_ZOOM));
  }, []);
  const setZoomPercent = useCallback((nextPercent: number) => {
    zoomTo(nextPercent / 100);
  }, [zoomTo]);
  const zoomIn = useCallback(() => zoomTo(zoom * 1.25), [zoom, zoomTo]);
  const zoomOut = useCallback(() => zoomTo(zoom / 1.25), [zoom, zoomTo]);
  const fit = useCallback(() => {
    setZoom(1);
    setCenter({ x: 0.5, y: 0.5 });
  }, []);

  const onWheel = useCallback((side: ComparisonSide, event: WheelEvent) => {
    event.preventDefault();
    const image = imageSizes[side];
    if (!image) return;
    const pane = side === 'left' ? leftContainerRef.current : rightContainerRef.current;
    if (!pane) return;
    const rect = pane.getBoundingClientRect();
    const dx = event.clientX - rect.left - rect.width / 2;
    const dy = event.clientY - rect.top - rect.height / 2;
    const fitScale = fitScales[side];
    const displayedScale = fitScale * zoom;
    const nextZoom = clamp(zoom * Math.exp(-event.deltaY * 0.003), MIN_ZOOM, MAX_ZOOM);
    const nextDisplayedScale = fitScale * nextZoom;
    const focusX = center.x + dx / (image.width * displayedScale);
    const focusY = center.y + dy / (image.height * displayedScale);
    setCenter({
      x: clamp(focusX - dx / (image.width * nextDisplayedScale), 0, 1),
      y: clamp(focusY - dy / (image.height * nextDisplayedScale), 0, 1),
    });
    setZoom(nextZoom);
  }, [center, fitScales, imageSizes, leftContainerRef, rightContainerRef, zoom]);

  useEffect(() => {
    const left = leftContainerRef.current;
    const right = rightContainerRef.current;
    const handleLeft = (event: WheelEvent) => onWheel('left', event);
    const handleRight = (event: WheelEvent) => onWheel('right', event);
    left?.addEventListener('wheel', handleLeft, { passive: false });
    right?.addEventListener('wheel', handleRight, { passive: false });
    return () => {
      left?.removeEventListener('wheel', handleLeft);
      right?.removeEventListener('wheel', handleRight);
    };
  }, [leftContainerRef, onWheel, pairKey, rightContainerRef]);

  const onPointerDown = useCallback((side: ComparisonSide, event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragStartRef.current = {
      side,
      pointerX: event.clientX,
      pointerY: event.clientY,
      centerX: center.x,
      centerY: center.y,
    };
    setDraggingSide(side);
  }, [center]);

  const onPointerMove = useCallback((side: ComparisonSide, event: ReactPointerEvent<HTMLDivElement>) => {
    const start = dragStartRef.current;
    const image = imageSizes[side];
    if (!start || start.side !== side || !image) return;
    const displayedScale = fitScales[side] * zoom;
    setCenter({
      x: clamp(start.centerX - (event.clientX - start.pointerX) / (image.width * displayedScale), 0, 1),
      y: clamp(start.centerY - (event.clientY - start.pointerY) / (image.height * displayedScale), 0, 1),
    });
  }, [fitScales, imageSizes, zoom]);

  const onPointerUp = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragStartRef.current = null;
    setDraggingSide(null);
  }, []);

  const frameStyle = useCallback((side: ComparisonSide): CSSProperties => {
    const image = imageSizes[side];
    const pane = paneSizes[side];
    if (!image) return { inset: 0 };
    const displayedScale = fitScales[side] * zoom;
    const displayedWidth = image.width * displayedScale;
    const displayedHeight = image.height * displayedScale;
    const maxTx = Math.max(0, (displayedWidth - pane.width) / 2);
    const maxTy = Math.max(0, (displayedHeight - pane.height) / 2);
    const tx = clamp((0.5 - center.x) * displayedWidth, -maxTx, maxTx);
    const ty = clamp((0.5 - center.y) * displayedHeight, -maxTy, maxTy);
    return {
      width: image.width,
      height: image.height,
      transform: `translate(calc(-50% + ${tx}px), calc(-50% + ${ty}px)) scale(${displayedScale})`,
    };
  }, [center, fitScales, imageSizes, paneSizes, zoom]);

  return {
    zoomPercent: Math.round(zoom * 100),
    isFit: Math.abs(zoom - 1) < 0.001,
    draggingSide,
    zoomIn,
    zoomOut,
    setZoomPercent,
    fit,
    frameStyle,
    handlers: { onPointerDown, onPointerMove, onPointerUp },
  };
}
