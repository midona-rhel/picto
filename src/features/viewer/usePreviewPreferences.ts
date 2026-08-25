import { useEffect, useSyncExternalStore } from 'react';
import { settingsController, type AppSettings } from '../../controllers/settingsController';

export interface PreviewPreferences {
  imageRendering: 'smooth' | 'pixelated';
  imageDefaultZoom: 'fit' | 'actual';
  showTransparencyGrid: boolean;
  videoAutoPlay: boolean;
  videoLoop: boolean;
  viewerTrackpadGestures: boolean;
}

const DEFAULTS: PreviewPreferences = {
  imageRendering: 'smooth',
  imageDefaultZoom: 'fit',
  showTransparencyGrid: false,
  videoAutoPlay: true,
  videoLoop: true,
  viewerTrackpadGestures: false,
};

let current = DEFAULTS;
let loaded = false;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function toPreviewPreferences(settings: AppSettings): PreviewPreferences {
  return {
    imageRendering: settings.imageRendering,
    imageDefaultZoom: settings.imageDefaultZoom,
    showTransparencyGrid: settings.showTransparencyGrid,
    videoAutoPlay: settings.videoAutoPlay,
    videoLoop: settings.videoLoop,
    viewerTrackpadGestures: settings.viewerTrackpadGestures,
  };
}

export function applyPreviewPreferences(settings: AppSettings): void {
  const next = toPreviewPreferences(settings);
  loaded = true;
  if (
    next.imageRendering === current.imageRendering
    && next.imageDefaultZoom === current.imageDefaultZoom
    && next.showTransparencyGrid === current.showTransparencyGrid
    && next.videoAutoPlay === current.videoAutoPlay
    && next.videoLoop === current.videoLoop
    && next.viewerTrackpadGestures === current.viewerTrackpadGestures
  ) return;
  current = next;
  listeners.forEach((listener) => listener());
}

function loadPreviewPreferences(): Promise<void> {
  if (loading) return loading;
  loading = settingsController.getSettings()
    .then(applyPreviewPreferences)
    .catch(() => {})
    .finally(() => { loading = null; });
  return loading;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): PreviewPreferences {
  return current;
}

/** Shared preview behavior for inline, Quick Look, and standalone viewers. */
export function usePreviewPreferences(): PreviewPreferences {
  const preferences = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  useEffect(() => {
    if (!loaded) void loadPreviewPreferences();
  }, []);
  return preferences;
}
