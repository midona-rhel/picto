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
  options?: { target?: 'window' | 'document' },
): void {
  useEffect(() => {
    if (!active) return;
    const target = options?.target === 'document' ? document : window;
    const onMove = callbacks.onMove as unknown as EventListener;
    const onEnd = callbacks.onEnd as unknown as EventListener;
    target.addEventListener('mousemove', onMove);
    target.addEventListener('mouseup', onEnd);
    return () => {
      target.removeEventListener('mousemove', onMove);
      target.removeEventListener('mouseup', onEnd);
    };
  }, [active, callbacks.onMove, callbacks.onEnd, options?.target]);
}
