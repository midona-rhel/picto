/**
 * A Set<string> backed by localStorage for persistence across sessions.
 */

import { useState, useCallback } from 'react';

export function usePersistedSet(storageKey: string): [Set<string>, (id: string) => void] {
  const [set, setSet] = useState<Set<string>>(() => {
    try {
      const raw = localStorage.getItem(storageKey);
      return raw ? new Set(JSON.parse(raw)) : new Set();
    } catch {
      return new Set();
    }
  });

  const toggle = useCallback((id: string) => {
    setSet((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      try { localStorage.setItem(storageKey, JSON.stringify([...next])); } catch { /* */ }
      return next;
    });
  }, [storageKey]);

  return [set, toggle];
}
