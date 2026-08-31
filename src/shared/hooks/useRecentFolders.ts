import { useCallback, useEffect, useState } from 'react';

const STORAGE_PREFIX = 'picto-recent-folders:';
const DEFAULT_MAX = 30;

let activeLibraryPath: string | null = null;
const listeners = new Set<() => void>();

function storageKey(): string | null {
  return activeLibraryPath ? `${STORAGE_PREFIX}${activeLibraryPath}` : null;
}

function emitChange(): void {
  for (const listener of listeners) listener();
}

export function setRecentFoldersLibrary(path: string | null): void {
  if (activeLibraryPath === path) return;
  activeLibraryPath = path;
  emitChange();
}

export function readRecentFolderIds(): number[] {
  const key = storageKey();
  if (!key) return [];
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? '[]');
    if (!Array.isArray(value)) return [];
    return value.filter((id): id is number => Number.isInteger(id) && id > 0);
  } catch {
    return [];
  }
}

/** Record a deliberate user interaction with folders, newest first. */
export function recordRecentFolderUse(folderIds: readonly number[], maxItems = DEFAULT_MAX): void {
  const key = storageKey();
  const validIds = folderIds.filter((id) => Number.isInteger(id) && id > 0);
  if (!key || validIds.length === 0) return;
  const used = new Set(validIds);
  const next = [...validIds, ...readRecentFolderIds().filter((id) => !used.has(id))].slice(0, maxItems);
  try {
    localStorage.setItem(key, JSON.stringify(next));
    emitChange();
  } catch { /* localStorage may be unavailable */ }
}

export function useRecentFolders(maxItems = DEFAULT_MAX): [number[], (folderIds: readonly number[]) => void] {
  const [folderIds, setFolderIds] = useState(readRecentFolderIds);

  useEffect(() => {
    const refresh = () => setFolderIds(readRecentFolderIds());
    listeners.add(refresh);
    refresh();
    return () => { listeners.delete(refresh); };
  }, []);

  const record = useCallback((ids: readonly number[]) => {
    recordRecentFolderUse(ids, maxItems);
  }, [maxItems]);

  return [folderIds, record];
}
