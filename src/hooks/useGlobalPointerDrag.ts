import { useEffect } from 'react';

/**
 * Attach global `mousemove` + `mouseup` listeners while a drag is active.
 *
 * @param callbacks  `{ onMove, onEnd }` — must be stable references (useCallback).
 * @param active     When true, listeners are attached; when false, they are removed.
 */
export function useGlobalPointerDrag(
  callbacks: { onMove: (e: MouseEvent) => void; onEnd: (e: MouseEvent) => void },
  active: boolean,
): void {
  useEffect(() => {
    if (!active) return;
    window.addEventListener('mousemove', callbacks.onMove);
    window.addEventListener('mouseup', callbacks.onEnd);
    return () => {
      window.removeEventListener('mousemove', callbacks.onMove);
      window.removeEventListener('mouseup', callbacks.onEnd);
    };
  }, [active, callbacks.onMove, callbacks.onEnd]);
}
