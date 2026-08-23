import { listen, type UnlistenFn } from '../platform/ipc';

export const LIBRARY_CHANGED_EVENT = 'library/changed';

export interface LibraryChangedPayload {
  revision: number;
  resources: string[];
  item_ids: number[];
}

export type LibraryInvalidationCallback = (payload: LibraryChangedPayload) => void;

export interface LibraryInvalidationRegistry {
  register(resource: string, callback: LibraryInvalidationCallback): () => void;
  start(): void;
  stop(): void;
}

interface PendingBatch {
  revision: number;
  resources: Set<string>;
  itemIds: Set<number>;
}

export function createLibraryInvalidationRegistry(): LibraryInvalidationRegistry {
  const callbacksByResource = new Map<string, Set<LibraryInvalidationCallback>>();
  let started = false;
  let listenerGeneration = 0;
  let unlisten: UnlistenFn | undefined;
  let listenerPromise: Promise<void> | undefined;
  let pending: PendingBatch | undefined;
  let flushScheduled = false;
  let lastDeliveredRevision = -Infinity;

  const flush = () => {
    flushScheduled = false;
    if (!started || !pending) return;

    const batch = pending;
    pending = undefined;
    if (batch.revision <= lastDeliveredRevision) return;
    lastDeliveredRevision = batch.revision;

    const payload: LibraryChangedPayload = {
      revision: batch.revision,
      resources: [...batch.resources],
      item_ids: [...batch.itemIds],
    };
    const callbacks = new Set<LibraryInvalidationCallback>();
    for (const resource of payload.resources) {
      for (const callback of callbacksByResource.get(resource) ?? []) callbacks.add(callback);
    }
    for (const itemId of payload.item_ids) {
      for (const callback of callbacksByResource.get(`item:${itemId}`) ?? []) callbacks.add(callback);
    }
    for (const callback of callbacks) callback(payload);
  };

  const receive = (payload: LibraryChangedPayload) => {
    if (!started || payload.revision <= lastDeliveredRevision) return;

    if (!pending) {
      pending = {
        revision: payload.revision,
        resources: new Set(payload.resources),
        itemIds: new Set(payload.item_ids),
      };
    } else {
      pending.revision = Math.max(pending.revision, payload.revision);
      payload.resources.forEach((resource) => pending!.resources.add(resource));
      payload.item_ids.forEach((itemId) => pending!.itemIds.add(itemId));
    }

    if (!flushScheduled) {
      flushScheduled = true;
      queueMicrotask(flush);
    }
  };

  const start = () => {
    if (started) return;
    started = true;
    const generation = ++listenerGeneration;
    listenerPromise = listen<LibraryChangedPayload>(LIBRARY_CHANGED_EVENT, ({ payload }) => {
      if (generation === listenerGeneration) receive(payload);
    }).then((removeListener) => {
      if (!started || generation !== listenerGeneration) {
        removeListener();
        return;
      }
      unlisten = removeListener;
    }).catch(() => {});
  };

  const stop = () => {
    if (!started && !listenerPromise && !unlisten) return;
    started = false;
    listenerGeneration += 1;
    pending = undefined;
    const removeListener = unlisten;
    unlisten = undefined;
    removeListener?.();
    listenerPromise = undefined;
  };

  return {
    register(resource, callback) {
      const callbacks = callbacksByResource.get(resource) ?? new Set<LibraryInvalidationCallback>();
      callbacks.add(callback);
      callbacksByResource.set(resource, callbacks);
      return () => {
        callbacks.delete(callback);
        if (callbacks.size === 0) callbacksByResource.delete(resource);
      };
    },
    start,
    stop,
  };
}

export const libraryInvalidation = createLibraryInvalidationRegistry();

const hot = (import.meta as ImportMeta & { hot?: { dispose(callback: () => void): void } }).hot;
hot?.dispose(() => libraryInvalidation.stop());
