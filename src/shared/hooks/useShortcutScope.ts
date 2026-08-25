import { useEffect, useRef } from 'react';
import {
  acquireShortcutSuspension,
  registerShortcutScope,
  type ShortcutScopeHandler,
  type ShortcutScopeOptions,
} from '../../runtime/shortcutRuntime';

export function useShortcutScope(
  handler: ShortcutScopeHandler,
  options: ShortcutScopeOptions & { enabled?: boolean } = {},
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  const { enabled = true, priority = 0, allowInEditable = false } = options;

  useEffect(() => {
    if (!enabled) return;
    return registerShortcutScope((event) => handlerRef.current(event), {
      priority,
      allowInEditable,
    });
  }, [allowInEditable, enabled, priority]);
}

export function useShortcutSuspension(active: boolean): void {
  useEffect(() => {
    if (!active) return;
    return acquireShortcutSuspension();
  }, [active]);
}
