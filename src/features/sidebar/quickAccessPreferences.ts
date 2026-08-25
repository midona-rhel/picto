import { useEffect, useSyncExternalStore } from 'react';
import { getSettings, patchSettings } from '../../platform/settingsApi';
import { registerAppSettingsReload } from '../../runtime/appSettingsSettle';
import { showErrorNotification } from '../../shared/lib/notifications';

const listeners = new Set<() => void>();
let snapshot: string[] = [];
let loading: Promise<void> | null = null;

function emit(): void {
  listeners.forEach((listener) => listener());
}

function setSnapshot(next: string[]): void {
  snapshot = next;
  emit();
}

async function reload(): Promise<void> {
  const settings = await getSettings();
  setSnapshot(settings.sidebarQuickAccess);
}

function ensureLoaded(): Promise<void> {
  if (!loading) {
    loading = reload().catch((reason: unknown) => {
      loading = null;
      showErrorNotification({
        title: 'Unable to load Quick Access',
        message: reason instanceof Error ? reason.message : String(reason),
      });
    });
  }
  return loading;
}

export function useQuickAccess(): string[] {
  useEffect(() => {
    void ensureLoaded();
    return registerAppSettingsReload(() => { void reload(); });
  }, []);
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot,
  );
}

async function persist(next: string[]): Promise<void> {
  const previous = snapshot;
  setSnapshot(next);
  try {
    await patchSettings({ sidebarQuickAccess: next });
  } catch (reason: unknown) {
    if (snapshot === next) setSnapshot(previous);
    showErrorNotification({
      title: 'Unable to update Quick Access',
      message: reason instanceof Error ? reason.message : String(reason),
    });
  }
}

export function addQuickAccess(nodeId: string): Promise<void> {
  return snapshot.includes(nodeId) ? Promise.resolve() : persist([...snapshot, nodeId]);
}

export function removeQuickAccess(nodeId: string): Promise<void> {
  return persist(snapshot.filter((id) => id !== nodeId));
}

export function reorderQuickAccess(nodeIds: string[]): Promise<void> {
  const unique = [...new Set(nodeIds)];
  return persist(unique);
}
