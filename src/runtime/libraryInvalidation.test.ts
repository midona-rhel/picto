import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createLibraryInvalidationRegistry,
  LIBRARY_CHANGED_EVENT,
  type LibraryChangedPayload,
} from './libraryInvalidation';

type EventHandler = (event: { payload: LibraryChangedPayload }) => void;

const { listeners, removeListener } = vi.hoisted(() => ({
  listeners: [] as EventHandler[],
  removeListener: vi.fn(),
}));

vi.mock('../platform/ipc', () => ({
  listen: vi.fn(async (name: string, handler: EventHandler) => {
    expect(name).toBe(LIBRARY_CHANGED_EVENT);
    listeners.push(handler);
    return removeListener;
  }),
}));

function emit(payload: LibraryChangedPayload) {
  for (const listener of listeners) listener({ payload });
}

describe('library invalidation', () => {
  afterEach(() => {
    listeners.length = 0;
    removeListener.mockClear();
  });

  it('coalesces a burst without relabeling older resources as a newer revision', async () => {
    const registry = createLibraryInvalidationRegistry();
    const libraryCallback = vi.fn();
    const sidebarCallback = vi.fn();
    registry.register('library', libraryCallback);
    registry.register('sidebar', sidebarCallback);
    registry.start();

    emit({ revision: 4, resources: ['library', 'sidebar'], item_ids: [7] });
    emit({ revision: 5, resources: ['tags', 'library'], item_ids: [8, 7] });

    expect(libraryCallback).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(libraryCallback).toHaveBeenCalledOnce();
    expect(libraryCallback).toHaveBeenCalledWith({
      revision: 5,
      resources: ['library'],
      item_ids: [7, 8],
    });
    expect(sidebarCallback).toHaveBeenCalledOnce();
    expect(sidebarCallback).toHaveBeenCalledWith({
      revision: 4,
      resources: ['sidebar'],
      item_ids: [7],
    });
    expect(registry.latestRevision('library')).toBe(5);
    expect(registry.latestRevision('sidebar')).toBe(4);
  });

  it('suppresses revisions already delivered', async () => {
    const registry = createLibraryInvalidationRegistry();
    const callback = vi.fn();
    registry.register('library', callback);
    registry.start();

    emit({ revision: 8, resources: ['library'], item_ids: [] });
    await Promise.resolve();
    emit({ revision: 7, resources: ['library'], item_ids: [1] });
    emit({ revision: 8, resources: ['library'], item_ids: [2] });
    await Promise.resolve();

    expect(callback).toHaveBeenCalledTimes(1);
  });

  it('tracks resource revisions independently', async () => {
    const registry = createLibraryInvalidationRegistry();
    const libraryCallback = vi.fn();
    registry.register('library', libraryCallback);
    registry.start();

    emit({ revision: 8, resources: ['sidebar'], item_ids: [] });
    await Promise.resolve();
    emit({ revision: 7, resources: ['library'], item_ids: [] });
    await Promise.resolve();

    expect(libraryCallback).toHaveBeenCalledTimes(1);
    expect(libraryCallback).toHaveBeenCalledWith({
      revision: 7,
      resources: ['library'],
      item_ids: [],
    });
  });

  it('does not retain per-item watermarks without a mounted item consumer', async () => {
    const registry = createLibraryInvalidationRegistry();
    registry.register('library', vi.fn());
    registry.start();

    emit({ revision: 3, resources: ['library'], item_ids: [10, 11, 12] });
    await Promise.resolve();

    expect(registry.latestRevision('library')).toBe(3);
    expect(registry.latestRevision('item:10')).toBe(-Infinity);
  });

  it('drops queued state when the last consumer unregisters before delivery', async () => {
    const registry = createLibraryInvalidationRegistry();
    const callback = vi.fn();
    const unregister = registry.register('library', callback);
    registry.start();

    emit({ revision: 6, resources: ['library'], item_ids: [1] });
    unregister();
    await Promise.resolve();

    expect(callback).not.toHaveBeenCalled();
    expect(registry.latestRevision('library')).toBe(-Infinity);
  });

  it('matches exact resources and item keys without duplicate callback delivery', async () => {
    const registry = createLibraryInvalidationRegistry();
    const libraryCallback = vi.fn();
    const itemCallback = vi.fn();
    const unrelatedCallback = vi.fn();
    registry.register('library', libraryCallback);
    registry.register('item:42', itemCallback);
    registry.register('sidebar', itemCallback);
    registry.register('folder:9', unrelatedCallback);
    registry.start();

    emit({ revision: 1, resources: ['library', 'sidebar'], item_ids: [42] });
    await Promise.resolve();

    expect(libraryCallback).toHaveBeenCalledTimes(1);
    expect(itemCallback).toHaveBeenCalledTimes(1);
    expect(unrelatedCallback).not.toHaveBeenCalled();
  });

  it('stops delivery and removes the IPC listener', async () => {
    const registry = createLibraryInvalidationRegistry();
    const callback = vi.fn();
    registry.register('library', callback);
    registry.start();
    await Promise.resolve();
    await Promise.resolve();

    registry.stop();
    emit({ revision: 1, resources: ['library'], item_ids: [] });
    await Promise.resolve();

    expect(callback).not.toHaveBeenCalled();
    expect(removeListener).toHaveBeenCalledTimes(1);
  });
});
