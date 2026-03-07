import { useEffect } from 'react';

/**
 * Attach a global `keydown` listener on `document` with automatic cleanup.
 *
 * @param handler  Stable callback (wrap in useCallback).
 * @param enabled  Gate — listener is only active when true (default true).
 * @param options  `{ capture }` — when true the handler fires in the capture phase.
 */
export function useGlobalKeydown(
  handler: (e: KeyboardEvent) => void,
  enabled = true,
  options?: { capture?: boolean },
): void {
  const capture = options?.capture ?? false;

  useEffect(() => {
    if (!enabled) return;
    document.addEventListener('keydown', handler, capture);
    return () => document.removeEventListener('keydown', handler, capture);
  }, [handler, enabled, capture]);
}
