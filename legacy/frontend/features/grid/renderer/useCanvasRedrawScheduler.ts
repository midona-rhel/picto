import { useCallback, useRef, type RefObject } from 'react';

export function useCanvasRedrawScheduler(args: {
  frozenRef: RefObject<boolean>;
  drawBaseRef: RefObject<() => void>;
  drawOverlayRef: RefObject<() => void>;
}) {
  const { frozenRef, drawBaseRef, drawOverlayRef } = args;
  const dirtyRef = useRef<{ base: boolean; overlay: boolean }>({ base: false, overlay: false });
  const rafScheduledRef = useRef(false);

  const markDirty = useCallback((lanes: 'base' | 'overlay' | 'both') => {
    const dirty = dirtyRef.current;
    if (lanes === 'base' || lanes === 'both') dirty.base = true;
    if (lanes === 'overlay' || lanes === 'both') dirty.overlay = true;
    if (rafScheduledRef.current) return;
    rafScheduledRef.current = true;
    requestAnimationFrame(() => {
      rafScheduledRef.current = false;
      const nextDirty = dirtyRef.current;
      if (nextDirty.base) {
        nextDirty.base = false;
        drawBaseRef.current?.();
      }
      if (nextDirty.overlay) {
        nextDirty.overlay = false;
        drawOverlayRef.current?.();
      }
    });
  }, [drawBaseRef, drawOverlayRef, frozenRef]);

  return {
    dirtyRef,
    markDirty,
  };
}
