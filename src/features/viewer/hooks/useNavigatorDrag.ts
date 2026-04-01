/**
 * useNavigatorDrag — converts minimap mouse events to normalized pan coordinates.
 *
 * Accounts for the image display offset inside the square navigator container.
 * Uses RAF lerp for smooth panning when dragging.
 */

import { useCallback, useEffect, useRef, type RefObject, type MutableRefObject } from 'react';
import type { ImageSize } from './useImageZoom';

const NAV_SIZE = 120;
const LERP_SPEED = 0.3;

export function useNavigatorDrag(
  navRef: RefObject<HTMLDivElement | null>,
  imageSizeRef: MutableRefObject<ImageSize | null>,
  panToNormalized: (nx: number, ny: number) => void,
): (e: React.MouseEvent) => void {
  const draggingRef = useRef(false);
  const targetRef = useRef({ nx: 0.5, ny: 0.5 });
  const currentRef = useRef({ nx: 0.5, ny: 0.5 });
  const rafRef = useRef<number | null>(null);
  const panRef = useRef(panToNormalized);
  panRef.current = panToNormalized;
  const tickRef = useRef<() => void>(() => {});

  function getNorm(clientX: number, clientY: number, nav: HTMLDivElement, imgSize: ImageSize) {
    const rect = nav.getBoundingClientRect();
    const imgAspect = imgSize.width / imgSize.height;
    const wide = imgAspect > 1;
    const displayW = wide ? NAV_SIZE : NAV_SIZE * imgAspect;
    const displayH = wide ? NAV_SIZE / imgAspect : NAV_SIZE;
    const offsetX = wide ? 0 : (NAV_SIZE - displayW) / 2;
    const offsetY = wide ? (NAV_SIZE - displayH) / 2 : 0;
    const localX = clientX - rect.left - offsetX;
    const localY = clientY - rect.top - offsetY;
    return {
      nx: Math.max(0, Math.min(1, localX / displayW)),
      ny: Math.max(0, Math.min(1, localY / displayH)),
    };
  }

  useEffect(() => {
    function tick() {
      if (!draggingRef.current) { rafRef.current = null; return; }
      const cur = currentRef.current;
      const tgt = targetRef.current;
      const dx = tgt.nx - cur.nx;
      const dy = tgt.ny - cur.ny;
      if (Math.abs(dx) < 0.001 && Math.abs(dy) < 0.001) {
        currentRef.current = { nx: tgt.nx, ny: tgt.ny };
        panRef.current(tgt.nx, tgt.ny);
      } else {
        const next = { nx: cur.nx + dx * LERP_SPEED, ny: cur.ny + dy * LERP_SPEED };
        currentRef.current = next;
        panRef.current(next.nx, next.ny);
      }
      rafRef.current = requestAnimationFrame(tick);
    }
    tickRef.current = tick;

    function onMove(e: MouseEvent) {
      if (!draggingRef.current) return;
      const nav = navRef.current;
      const imgSize = imageSizeRef.current;
      if (!nav || !imgSize) return;
      targetRef.current = getNorm(e.clientX, e.clientY, nav, imgSize);
    }

    function onUp() {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      if (rafRef.current != null) { cancelAnimationFrame(rafRef.current); rafRef.current = null; }
      const tgt = targetRef.current;
      currentRef.current = tgt;
      panRef.current(tgt.nx, tgt.ny);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, [navRef, imageSizeRef]);

  return useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const nav = navRef.current;
    const imgSize = imageSizeRef.current;
    if (!nav || !imgSize) return;
    const norm = getNorm(e.clientX, e.clientY, nav, imgSize);
    currentRef.current = norm;
    targetRef.current = norm;
    panRef.current(norm.nx, norm.ny);
    draggingRef.current = true;
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(tickRef.current);
  }, [navRef, imageSizeRef]);
}
