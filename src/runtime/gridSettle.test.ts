import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridController } from '../controllers/gridController';
import { gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { startGridSettle } from './gridSettle';

const { callbacks, derivative } = vi.hoisted(() => ({
  callbacks: new Map<string, () => void>(),
  derivative: { onColor: undefined as undefined | ((payload: { fileHash: string; dominantColorHex: string | null }) => void) },
}));

vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: {
    register: vi.fn((resource: string, callback: () => void) => {
      callbacks.set(resource, callback);
      return () => callbacks.delete(resource);
    }),
  },
}));

vi.mock('../shared/lib/thumbnailChanges', () => ({
  listenDominantColorChanged: vi.fn((callback) => {
    derivative.onColor = callback;
    return Promise.resolve(() => { derivative.onColor = undefined; });
  }),
}));

const store = getDefaultStore();

describe('grid invalidation', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    callbacks.clear();
    derivative.onColor = undefined;
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true });
    store.set(gridTransitionPhaseAtom, 'idle');
  });

  it('re-queries the canonical grid for a library invalidation', () => {
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);
    const stop = startGridSettle();

    callbacks.get('library')?.();

    expect(reload).toHaveBeenCalledOnce();
    expect(reload).toHaveBeenCalledWith({ preserveItems: true });
    stop();
  });

  it('coalesces invalidations received during a scope transition', () => {
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);
    store.set(gridTransitionPhaseAtom, 'waiting');
    const stop = startGridSettle();

    callbacks.get('library')?.();
    callbacks.get('library')?.();
    expect(reload).not.toHaveBeenCalled();

    store.set(gridTransitionPhaseAtom, 'idle');
    expect(reload).toHaveBeenCalledOnce();
    stop();
  });

  it('does not query while the grid is inactive', () => {
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: false });
    const stop = startGridSettle();

    callbacks.get('library')?.();

    expect(reload).not.toHaveBeenCalled();
    stop();
  });

  it('patches a completed dominant color without re-querying the grid', () => {
    const reload = vi.spyOn(gridController, 'loadFirstPage').mockResolvedValue(undefined);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [{
        item_id: 7,
        kind: 'media',
        lifecycle: 'active',
        name: 'image',
        display_file_hash: 'hash-7',
        display_mime_type: 'image/jpeg',
        pixel_width: 100,
        pixel_height: 100,
        duration_ms: null,
        frame_count: null,
        dominant_color_hex: null,
        rating: null,
        media_count: 1,
      }],
    });
    const stop = startGridSettle();

    derivative.onColor?.({ fileHash: 'hash-7', dominantColorHex: '#123456' });

    expect(store.get(gridSessionAtom).items[0].dominant_color_hex).toBe('#123456');
    expect(reload).not.toHaveBeenCalled();
    stop();
  });
});
