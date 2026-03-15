import { useEffect, useLayoutEffect, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize, ZoomState, NavigatorRect } from './useImageZoom';

const DEFAULT_NAV_SIZE = 120;

/**
 * Computes navigator viewport rect from zoom state — pure math, no DOM reads.
 */
function computeNavRect(
  zoomState: ZoomState,
  imageSize: ImageSize,
  containerSize: { w: number; h: number },
): NavigatorRect | null {
  const cw = containerSize.w;
  const ch = containerSize.h;
  if (imageSize.width * zoomState.scale < cw + 1 && imageSize.height * zoomState.scale < ch + 1) return null;

  const viewW = cw / zoomState.scale;
  const viewH = ch / zoomState.scale;
  const cx = (cw / 2 - zoomState.tx) / zoomState.scale;
  const cy = (ch / 2 - zoomState.ty) / zoomState.scale;

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
}

/**
 * Applies a NavigatorRect to the minimap DOM elements.
 */
function applyNavRect(
  nav: HTMLDivElement,
  vp: HTMLDivElement,
  rect: NavigatorRect | null,
  imageSize: ImageSize,
  navSize: number,
): void {
  if (rect) {
    nav.style.display = '';
    const imgAspect = imageSize.width / imageSize.height;
    const wide = imgAspect > 1;
    const displayW = wide ? navSize : navSize * imgAspect;
    const displayH = wide ? navSize / imgAspect : navSize;
    const offsetX = wide ? 0 : (navSize - displayW) / 2;
    const offsetY = wide ? (navSize - displayH) / 2 : 0;
    vp.style.display = '';
    vp.style.left = `${offsetX + rect.x * displayW}px`;
    vp.style.top = `${offsetY + rect.y * displayH}px`;
    vp.style.width = `${rect.w * displayW}px`;
    vp.style.height = `${rect.h * displayH}px`;
  } else {
    nav.style.display = 'none';
  }
}

/**
 * Direct DOM write effect that updates navigator viewport rect.
 *
 * Updates on both:
 * 1. React commits (useLayoutEffect) — for non-interactive changes
 * 2. Live RAF frames (onLiveFrameRef callback) — real-time during zoom/pan
 */
export function useNavigatorRenderer(
  _imgRef: RefObject<HTMLImageElement | null>,
  navRef: RefObject<HTMLDivElement | null>,
  vpRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  _zoomState: ZoomState,
  navigatorRect: NavigatorRect | null,
  navSize: number = DEFAULT_NAV_SIZE,
  _thumbRef?: RefObject<HTMLImageElement | null>,
  onLiveFrameRef?: MutableRefObject<((s: ZoomState) => void) | null>,
  containerSize?: { w: number; h: number },
): void {
  // Update from committed React state (covers non-interactive changes like fitToWindow)
  useLayoutEffect(() => {
    const nav = navRef.current;
    const vp = vpRef.current;
    if (!nav || !vp || !imageSizeRef.current) return;
    applyNavRect(nav, vp, navigatorRect, imageSizeRef.current, navSize);
  });

  // Register live-frame callback for real-time updates during interactive zoom/pan
  useEffect(() => {
    if (!onLiveFrameRef) return;
    onLiveFrameRef.current = (liveState: ZoomState) => {
      const nav = navRef.current;
      const vp = vpRef.current;
      const imgSize = imageSizeRef.current;
      if (!nav || !vp || !imgSize || !containerSize || containerSize.w === 0) return;
      const rect = computeNavRect(liveState, imgSize, containerSize);
      applyNavRect(nav, vp, rect, imgSize, navSize);
    };
    return () => {
      if (onLiveFrameRef.current) onLiveFrameRef.current = null;
    };
  }, [onLiveFrameRef, navRef, vpRef, imageSizeRef, containerSize, navSize]);
}
