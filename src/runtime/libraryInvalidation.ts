import { listen, type UnlistenFn } from '../platform/ipc';
import type { LibraryChanged } from '../shared/types/generated/application/LibraryChanged';

export const LIBRARY_CHANGED_EVENT = 'library/changed';

export type LibraryChangedPayload = LibraryChanged;

export type LibraryInvalidationCallback = (payload: LibraryChangedPayload) => void;

export interface LibraryInvalidationRegistry {
  register(resource: string, callback: LibraryInvalidationCallback): () => void;
  latestRevision(resource: string): number;
  start(): void;
  stop(): void;
}

interface PendingBatch {
  revision: number;
  resources: Set<string>;
  itemIds: Set<number>;
  revisionsByResource: Map<string, number>;
}

export function createLibraryInvalidationRegistry(): LibraryInvalidationRegistry {
  const callbacksByResource = new Map<string, Set<LibraryInvalidationCallback>>();
  let started = false;
  let listenerGeneration = 0;
  let unlisten: UnlistenFn | undefined;
  let listenerPromise: Promise<void> | undefined;
  const pendingByRevision = new Map<number, PendingBatch>();
  let flushScheduled = false;
  const latestRevisionByResource = new Map<string, number>();
  const latestPendingRevisionByResource = new Map<string, number>();

  const itemResource = (itemId: number) => `item:${itemId}`;

  const isNewRevision = (resource: string, revision: number) => (
    revision > (latestRevisionByResource.get(resource) ?? -Infinity)
    && revision > (latestPendingRevisionByResource.get(resource) ?? -Infinity)
  );

  const flush = () => {
    flushScheduled = false;
    if (!started || pendingByRevision.size === 0) return;

    const batches = [...pendingByRevision.values()].sort((left, right) => left.revision - right.revision);
    pendingByRevision.clear();
    latestPendingRevisionByResource.clear();
    const deliveries = new Map<LibraryInvalidationCallback, {
      revision: number;
      resources: Set<string>;
      itemIds: Set<number>;
    }>();

    const addDelivery = (
      callback: LibraryInvalidationCallback,
      revision: number,
      resource: string | null,
      itemIds: ReadonlySet<number>,
    ) => {
      const current = deliveries.get(callback);
      if (!current || revision > current.revision) {
        deliveries.set(callback, {
          revision,
          resources: new Set(resource == null ? [] : [resource]),
          itemIds: new Set([...(current?.itemIds ?? []), ...itemIds]),
        });
        return;
      }
      itemIds.forEach((itemId) => current.itemIds.add(itemId));
      if (revision === current.revision && resource != null) current.resources.add(resource);
    };

    for (const batch of batches) {
      batch.revisionsByResource.forEach((revision, resource) => {
        if (callbacksByResource.has(resource)) latestRevisionByResource.set(resource, revision);
      });

      for (const resource of batch.resources) {
        for (const callback of callbacksByResource.get(resource) ?? []) {
          addDelivery(callback, batch.revision, resource, batch.itemIds);
        }
      }
      for (const itemId of batch.itemIds) {
        for (const callback of callbacksByResource.get(itemResource(itemId)) ?? []) {
          addDelivery(callback, batch.revision, null, new Set([itemId]));
        }
      }
    }

    for (const [callback, delivery] of deliveries) {
      callback({
        revision: delivery.revision,
        resources: [...delivery.resources],
        item_ids: [...delivery.itemIds],
      });
    }
  };

  const receive = (payload: LibraryChangedPayload) => {
    if (!started) return;

    const resources = payload.resources.filter((resource) => (
      callbacksByResource.has(resource) && isNewRevision(resource, payload.revision)
    ));
    const trackedItemIds = payload.item_ids.filter((itemId) => {
      const resource = itemResource(itemId);
      return callbacksByResource.has(resource) && isNewRevision(resource, payload.revision);
    });
    // Resource consumers use item IDs to avoid unnecessary detail refreshes. Keep
    // them only in the pending frame; do not retain a watermark per historical item.
    const itemIds = resources.length > 0 ? payload.item_ids : trackedItemIds;
    if (resources.length === 0 && itemIds.length === 0) return;

    let batch = pendingByRevision.get(payload.revision);
    if (!batch) {
      batch = {
        revision: payload.revision,
        resources: new Set(),
        itemIds: new Set(),
        revisionsByResource: new Map(),
      };
      pendingByRevision.set(payload.revision, batch);
    }
    resources.forEach((resource) => {
      batch.resources.add(resource);
      batch.revisionsByResource.set(resource, payload.revision);
      latestPendingRevisionByResource.set(resource, payload.revision);
    });
    itemIds.forEach((itemId) => batch.itemIds.add(itemId));
    trackedItemIds.forEach((itemId) => {
      const resource = itemResource(itemId);
      batch.revisionsByResource.set(resource, payload.revision);
      latestPendingRevisionByResource.set(resource, payload.revision);
    });

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
    pendingByRevision.clear();
    latestPendingRevisionByResource.clear();
    latestRevisionByResource.clear();
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
        if (callbacks.size === 0) {
          callbacksByResource.delete(resource);
          latestRevisionByResource.delete(resource);
          latestPendingRevisionByResource.delete(resource);
        }
      };
    },
    latestRevision(resource) {
      return Math.max(
        latestRevisionByResource.get(resource) ?? -Infinity,
        latestPendingRevisionByResource.get(resource) ?? -Infinity,
      );
    },
    start,
    stop,
  };
}

export const libraryInvalidation = createLibraryInvalidationRegistry();

const hot = (import.meta as ImportMeta & { hot?: { dispose(callback: () => void): void } }).hot;
hot?.dispose(() => libraryInvalidation.stop());
