import { listen, type UnlistenFn } from '../../platform/ipc';

export const THUMBNAIL_CHANGED_EVENT = 'picto:thumbnail-changed';
export const DOMINANT_COLOR_CHANGED_EVENT = 'picto:dominant-color-changed';

export interface DominantColorChanged {
  fileHash: string;
  dominantColorHex: string | null;
}

export function listenThumbnailChanged(handler: (fileHash: string) => void): Promise<UnlistenFn> {
  return listen<{ fileHash: string }>(THUMBNAIL_CHANGED_EVENT, ({ payload }) => handler(payload.fileHash));
}

export function listenDominantColorChanged(
  handler: (change: DominantColorChanged) => void,
): Promise<UnlistenFn> {
  return listen<DominantColorChanged>(DOMINANT_COLOR_CHANGED_EVENT, ({ payload }) => handler(payload));
}

const thumbnailSubscribers = new Set<(fileHash: string) => void>();
let sharedListenerStarted = false;

/** Share one native listener across every DOM thumbnail instead of installing
 * an IPC listener per image. */
export function subscribeThumbnailChanged(handler: (fileHash: string) => void): UnlistenFn {
  thumbnailSubscribers.add(handler);
  if (!sharedListenerStarted) {
    sharedListenerStarted = true;
    try {
      void listenThumbnailChanged((fileHash) => {
        for (const subscriber of thumbnailSubscribers) subscriber(fileHash);
      }).catch(() => { sharedListenerStarted = false; });
    } catch {
      // Browser-only tests and previews do not install the Electron bridge.
      sharedListenerStarted = false;
    }
  }
  return () => thumbnailSubscribers.delete(handler);
}
