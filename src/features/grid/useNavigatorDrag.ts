import { useCallback, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize } from './useImageZoom';

/**
 * Handles minimap/navigator drag → normalized coordinate panning.
 * Returns a mousedown handler to attach to the navigator element.
 */
export function useNavigatorDrag(
  navRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  panToNormalized: (nx: number, ny: number) => void,
): (e: React.MouseEvent) => void {
  return useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const nav = navRef.current;
    if (!nav || !imageSizeRef.current) return;

    const calcNormalized = (ev: MouseEvent) => {
      const rect = nav.getBoundingClientRect();
      const iSize = imageSizeRef.current!;
      const imgAspect = iSize.width / iSize.height;
      let displayW: number, displayH: number, offsetX: number, offsetY: number;
      if (imgAspect > 1) {
        displayW = rect.width; displayH = rect.width / imgAspect; offsetX = 0; offsetY = (rect.height - displayH) / 2;
      } else {
        displayH = rect.height; displayW = rect.height * imgAspect; offsetX = (rect.width - displayW) / 2; offsetY = 0;
      }
      const nx = Math.max(0, Math.min(1, (ev.clientX - rect.left - offsetX) / displayW));
      const ny = Math.max(0, Math.min(1, (ev.clientY - rect.top - offsetY) / displayH));
      panToNormalized(nx, ny);
    };

    calcNormalized(e.nativeEvent);
    const handleMove = (ev: MouseEvent) => calcNormalized(ev);
    const handleUp = () => {
      window.removeEventListener('mousemove', handleMove);
      window.removeEventListener('mouseup', handleUp);
    };
    window.addEventListener('mousemove', handleMove);
    window.addEventListener('mouseup', handleUp);
  }, [navRef, imageSizeRef, panToNormalized]);
}
