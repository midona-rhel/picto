import { useEffect, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize, ZoomState, NavigatorRect } from './useImageZoom';

const DEFAULT_NAV_SIZE = 120;

/**
 * Direct DOM write effect that updates `img.style.transform` and
 * navigator viewport rect. Zero React re-renders during zoom/pan.
 *
 * When `thumbRef` is provided, the same transform is applied to the
 * thumbnail so it stays pixel-aligned with the full image during
 * zoom/pan (the thumbnail is stretched to full-image dimensions via
 * CSS width/height).
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
  useEffect(() => {
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
      let displayW: number, displayH: number, offsetX: number, offsetY: number;
      if (imgAspect > 1) {
        displayW = navSize; displayH = navSize / imgAspect; offsetX = 0; offsetY = (navSize - displayH) / 2;
      } else {
        displayH = navSize; displayW = navSize * imgAspect; offsetX = (navSize - displayW) / 2; offsetY = 0;
      }
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
