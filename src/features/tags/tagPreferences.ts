import { useEffect, useSyncExternalStore } from 'react';
import { getSettings, patchSettings } from '../../platform/settingsApi';
import { registerAppSettingsReload } from '../../runtime/appSettingsSettle';
import { showErrorNotification } from '../../shared/lib/notifications';
import { t } from '../../i18n';

export interface TagPreferences {
  showTagGroups: boolean;
  showTagPrefixes: boolean;
  starredTags: string[];
  tagGroupColors: Record<string, string>;
}

const listeners = new Set<() => void>();
let snapshot: TagPreferences = { showTagGroups: true, showTagPrefixes: false, starredTags: [], tagGroupColors: {} };
let loadPromise: Promise<void> | null = null;
let unregisterSettingsReload: (() => void) | null = null;

function emit(): void {
  listeners.forEach((listener) => listener());
}

function setSnapshot(next: TagPreferences): void {
  snapshot = next;
  if (typeof document !== 'undefined') {
    document.documentElement.dataset.hideTagPrefixes = String(!next.showTagPrefixes);
  }
  emit();
}

function ensureLoaded(): Promise<void> {
  if (!loadPromise) {
    loadPromise = getSettings()
      .then((settings) => setSnapshot({
        showTagGroups: settings.showTagGroups,
        showTagPrefixes: settings.showTagPrefixes,
        starredTags: settings.starredTags,
        tagGroupColors: settings.tagGroupColors,
      }))
      .catch((reason: unknown) => {
        loadPromise = null;
        showErrorNotification({
          title: t("Unable to load tag preferences"),
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

function ensureSettingsReloadRegistered(): void {
  if (unregisterSettingsReload) return;
  unregisterSettingsReload = registerAppSettingsReload(() => {
    loadPromise = null;
    void ensureLoaded();
  });
}

export function useTagPreferences(): TagPreferences {
  useEffect(() => {
    ensureSettingsReloadRegistered();
    void ensureLoaded();
  }, []);
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
      title: t("Unable to save tag preferences"),
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

export function setTagGroupColor(namespace: string, color: string | null): Promise<void> {
  const tagGroupColors = { ...snapshot.tagGroupColors };
  if (color) tagGroupColors[namespace] = color;
  else delete tagGroupColors[namespace];
  return persist({ ...snapshot, tagGroupColors }, { tagGroupColors });
}

export function replaceTagGroupColor(previousNamespace: string, nextNamespace: string): Promise<void> {
  if (!(previousNamespace in snapshot.tagGroupColors) || previousNamespace === nextNamespace) {
    return Promise.resolve();
  }
  const tagGroupColors = { ...snapshot.tagGroupColors };
  tagGroupColors[nextNamespace] = tagGroupColors[previousNamespace];
  delete tagGroupColors[previousNamespace];
  return persist({ ...snapshot, tagGroupColors }, { tagGroupColors });
}

export function removeTagGroupColor(namespace: string): Promise<void> {
  if (!(namespace in snapshot.tagGroupColors)) return Promise.resolve();
  const tagGroupColors = { ...snapshot.tagGroupColors };
  delete tagGroupColors[namespace];
  return persist({ ...snapshot, tagGroupColors }, { tagGroupColors });
}
