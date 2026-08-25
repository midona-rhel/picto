import { useEffect, useSyncExternalStore } from 'react';
import { getSettings, patchSettings } from '../../platform/settingsApi';
import { showErrorNotification } from '../../shared/lib/notifications';

export interface TagPreferences {
  showTagGroups: boolean;
  starredTags: string[];
}

const listeners = new Set<() => void>();
let snapshot: TagPreferences = { showTagGroups: true, starredTags: [] };
let loadPromise: Promise<void> | null = null;

function emit(): void {
  listeners.forEach((listener) => listener());
}

function setSnapshot(next: TagPreferences): void {
  snapshot = next;
  emit();
}

function ensureLoaded(): Promise<void> {
  if (!loadPromise) {
    loadPromise = getSettings()
      .then((settings) => setSnapshot({
        showTagGroups: settings.showTagGroups,
        starredTags: settings.starredTags,
      }))
      .catch((reason: unknown) => {
        loadPromise = null;
        showErrorNotification({
          title: 'Unable to load tag preferences',
          message: reason instanceof Error ? reason.message : String(reason),
        });
      });
  }
  return loadPromise;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useTagPreferences(): TagPreferences {
  useEffect(() => { void ensureLoaded(); }, []);
  return useSyncExternalStore(subscribe, () => snapshot, () => snapshot);
}

async function persist(next: TagPreferences, patch: Partial<TagPreferences>): Promise<void> {
  const previous = snapshot;
  setSnapshot(next);
  try {
    await patchSettings(patch);
  } catch (reason: unknown) {
    // Do not roll back a newer preference write that completed while this one was pending.
    if (snapshot === next) setSnapshot(previous);
    showErrorNotification({
      title: 'Unable to save tag preferences',
      message: reason instanceof Error ? reason.message : String(reason),
    });
  }
}

export function setTagGroupsVisible(visible: boolean): Promise<void> {
  return persist({ ...snapshot, showTagGroups: visible }, { showTagGroups: visible });
}

export function setTagStarred(tag: string, starred: boolean): Promise<void> {
  const starredTags = starred
    ? [...new Set([...snapshot.starredTags, tag])].sort()
    : snapshot.starredTags.filter((item) => item !== tag);
  return persist({ ...snapshot, starredTags }, { starredTags });
}

export function replaceStarredTag(previousTag: string, nextTag: string): Promise<void> {
  if (!snapshot.starredTags.includes(previousTag) || previousTag === nextTag) return Promise.resolve();
  const starredTags = [...new Set(snapshot.starredTags
    .filter((item) => item !== previousTag)
    .concat(nextTag))].sort();
  return persist({ ...snapshot, starredTags }, { starredTags });
}
