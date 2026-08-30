/**
 * useNavigatorRenderer — direct DOM writes for the navigator minimap.
 *
 * The navigator is always mounted in the DOM (display: none initially).
 * This hook toggles visibility and positions the viewport rect via direct
 * DOM writes — no React re-renders during interactive zoom/pan.
 */

import { useEffect, useLayoutEffect, useRef, type RefObject, type MutableRefObject } from 'react';
import { computeNavigatorRect, type NavigatorRect } from './navigatorMath';
import type { ZoomState, ImageSize } from './useImageZoom';

const DEFAULT_NAV_SIZE = 120;
const NAVIGATOR_SHOW_DELAY_MS = 80;

function positionNavRect(
  vp: HTMLDivElement,
  rect: NavigatorRect,
  imageSize: ImageSize,
  navSize: number,
): void {
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
}

export function useNavigatorRenderer(
  navRef: RefObject<HTMLDivElement | null>,
  vpRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  navigatorRect: NavigatorRect | null,
  navSize: number = DEFAULT_NAV_SIZE,
  onLiveFrameRef?: MutableRefObject<((s: ZoomState) => void) | null>,
  containerSize?: { w: number; h: number },
): void {
  const showTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRectRef = useRef<NavigatorRect | null>(null);

  const applyNavRect = (
    nav: HTMLDivElement,
    vp: HTMLDivElement,
    rect: NavigatorRect | null,
    imageSize: ImageSize,
  ) => {
    pendingRectRef.current = rect;
    if (!rect) {
      if (showTimerRef.current) clearTimeout(showTimerRef.current);
      showTimerRef.current = null;
      nav.style.display = 'none';
      return;
    }

    positionNavRect(vp, rect, imageSize, navSize);
    if (nav.style.display !== 'none' || showTimerRef.current) return;
    showTimerRef.current = setTimeout(() => {
      showTimerRef.current = null;
      if (pendingRectRef.current && nav.isConnected) nav.style.display = '';
    }, NAVIGATOR_SHOW_DELAY_MS);
  };

  // Committed state updates (fitToWindow, fitActual, etc.)
  useLayoutEffect(() => {
    const nav = navRef.current;
    const vp = vpRef.current;
    if (!nav || !vp || !imageSizeRef.current) return;
    applyNavRect(nav, vp, navigatorRect, imageSizeRef.current);
  });

  // Live RAF callback for real-time updates during interactive zoom/pan
  useEffect(() => {
    if (!onLiveFrameRef) return;
    onLiveFrameRef.current = (liveState: ZoomState) => {
      const nav = navRef.current;
      const vp = vpRef.current;
      const imgSize = imageSizeRef.current;
      if (!nav || !vp || !imgSize || !containerSize || containerSize.w === 0) return;
      const rect = computeNavigatorRect(liveState, imgSize, containerSize);
      applyNavRect(nav, vp, rect, imgSize);
    };
    return () => { if (onLiveFrameRef.current) onLiveFrameRef.current = null; };
  }, [onLiveFrameRef, navRef, vpRef, imageSizeRef, containerSize, navSize]);

  useEffect(() => () => {
    if (showTimerRef.current) clearTimeout(showTimerRef.current);
  }, []);
}
