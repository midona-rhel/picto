/**
 * A bounded MRU (most-recently-used) list backed by localStorage.
 * Items are stored newest-first. Duplicates are moved to front on re-use.
 */

import { useState, useCallback } from 'react';

const DEFAULT_MAX = 30;

/** Read the current list directly from localStorage (no React dependency). */
export function readRecentItems(storageKey: string): string[] {
  try {
    const raw = localStorage.getItem(storageKey);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

/** Prepend items to the MRU list, deduplicating and capping length. */
export function recordRecentItems(storageKey: string, ids: string[], maxItems = DEFAULT_MAX): void {
  if (ids.length === 0) return;
  try {
    const prev = readRecentItems(storageKey);
    const idSet = new Set(ids);
    const merged = [...ids, ...prev.filter((id) => !idSet.has(id))].slice(0, maxItems);
    localStorage.setItem(storageKey, JSON.stringify(merged));
  } catch { /* */ }
}

/** React hook: reads MRU list on mount, exposes a record function that also updates state. */
export function useRecentItems(storageKey: string, maxItems = DEFAULT_MAX): [string[], (ids: string[]) => void] {
  const [items, setItems] = useState<string[]>(() => readRecentItems(storageKey));

  const record = useCallback((ids: string[]) => {
    if (ids.length === 0) return;
    recordRecentItems(storageKey, ids, maxItems);
    setItems(readRecentItems(storageKey));
  }, [storageKey, maxItems]);

  return [items, record];
}
