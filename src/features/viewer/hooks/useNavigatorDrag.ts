/**
 * useNavigatorDrag — converts minimap mouse events to normalized pan coordinates.
 */

import { useCallback, useEffect, useRef, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize } from './useImageZoom';

export function useNavigatorDrag(
  navRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  panToNormalized: (nx: number, ny: number) => void,
): (e: React.MouseEvent) => void {
  const draggingRef = useRef(false);

  const panFromEvent = useCallback((clientX: number, clientY: number) => {
    const nav = navRef.current;
    const imgSize = imageSizeRef.current;
    if (!nav || !imgSize) return;
    const rect = nav.getBoundingClientRect();
    const nx = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    const ny = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
    panToNormalized(nx, ny);
  }, [navRef, imageSizeRef, panToNormalized]);

  useEffect(() => {
    const onMove = (e: MouseEvent) => { if (draggingRef.current) panFromEvent(e.clientX, e.clientY); };
    const onUp = () => { draggingRef.current = false; };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
  }, [panFromEvent]);

  return useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    draggingRef.current = true;
    panFromEvent(e.clientX, e.clientY);
  }, [panFromEvent]);
}
