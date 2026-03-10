import { useLayoutEffect, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize, ZoomState, NavigatorRect } from './useImageZoom';

const DEFAULT_NAV_SIZE = 120;

/**
 * Direct DOM write effect that updates `img.style.transform` and
 * navigator viewport rect. Runs synchronously before paint via
 * useLayoutEffect so the image is always centered on first render.
 */
export function useNavigatorRenderer(
  imgRef: RefObject<HTMLImageElement | null>,
  navRef: RefObject<HTMLDivElement | null>,
  vpRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  zoomState: ZoomState,
  navigatorRect: NavigatorRect | null,
  navSize: number = DEFAULT_NAV_SIZE,
  thumbRef?: RefObject<HTMLImageElement | null>,
): void {
  // Apply transform synchronously before paint — transform is composited (no layout thrash)
  useLayoutEffect(() => {
    const transform = `translate(calc(-50% + ${zoomState.tx}px), calc(-50% + ${zoomState.ty}px)) scale(${zoomState.scale})`;
    const img = imgRef.current;
    if (img) img.style.transform = transform;
    const thumb = thumbRef?.current;
    if (thumb) thumb.style.transform = transform;

    const nav = navRef.current;
    const vp = vpRef.current;
    if (!nav || !vp || !imageSizeRef.current) return;

    if (navigatorRect) {
      nav.style.display = '';
      const imgAspect = imageSizeRef.current.width / imageSizeRef.current.height;
      const wide = imgAspect > 1;
      const displayW = wide ? navSize : navSize * imgAspect;
      const displayH = wide ? navSize / imgAspect : navSize;
      const offsetX = wide ? 0 : (navSize - displayW) / 2;
      const offsetY = wide ? (navSize - displayH) / 2 : 0;
      vp.style.display = '';
      vp.style.left = `${offsetX + navigatorRect.x * displayW}px`;
      vp.style.top = `${offsetY + navigatorRect.y * displayH}px`;
      vp.style.width = `${navigatorRect.w * displayW}px`;
      vp.style.height = `${navigatorRect.h * displayH}px`;
    } else {
      nav.style.display = 'none';
    }
  });
}
