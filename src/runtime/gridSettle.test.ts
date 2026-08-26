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

  it('reconciles the canonical grid for a library invalidation', () => {
    const reconcile = vi.spyOn(gridController, 'reconcile').mockResolvedValue(true);
    const stop = startGridSettle();

    callbacks.get('library')?.();

    expect(reconcile).toHaveBeenCalledOnce();
    expect(reconcile).toHaveBeenCalledWith([]);
    stop();
  });

  it('reconciles recent-view changes only while that scope is active', () => {
    const reconcile = vi.spyOn(gridController, 'reconcile').mockResolvedValue(true);
    const stop = startGridSettle();

    callbacks.get('recently_viewed')?.();
    expect(reconcile).not.toHaveBeenCalled();

    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'recently_viewed' },
    });
    callbacks.get('recently_viewed')?.();
    expect(reconcile).toHaveBeenCalledOnce();
    stop();
  });

  it('coalesces invalidations received during a scope transition', () => {
    const reconcile = vi.spyOn(gridController, 'reconcile').mockResolvedValue(true);
    store.set(gridTransitionPhaseAtom, 'waiting');
    const stop = startGridSettle();

    callbacks.get('library')?.();
    callbacks.get('library')?.();
    expect(reconcile).not.toHaveBeenCalled();

    store.set(gridTransitionPhaseAtom, 'idle');
    expect(reconcile).toHaveBeenCalledOnce();
    stop();
  });

  it('does not query while the grid is inactive', () => {
    const reconcile = vi.spyOn(gridController, 'reconcile').mockResolvedValue(true);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: false });
    const stop = startGridSettle();

    callbacks.get('library')?.();

    expect(reconcile).not.toHaveBeenCalled();
    stop();
  });

  it('serializes a burst into one active and one trailing reconciliation', async () => {
    vi.useFakeTimers();
    let finishFirst: (() => void) | undefined;
    const reconcile = vi.spyOn(gridController, 'reconcile')
      .mockImplementationOnce(() => new Promise<boolean>((resolve) => {
        finishFirst = () => resolve(true);
      }))
      .mockResolvedValue(true);
    const stop = startGridSettle();

    callbacks.get('library')?.();
    callbacks.get('library')?.();
    callbacks.get('library')?.();
    expect(reconcile).toHaveBeenCalledOnce();

    finishFirst?.();
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(1_999);
    expect(reconcile).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1);
    expect(reconcile).toHaveBeenCalledTimes(2);

    stop();
    vi.useRealTimers();
  });

  it('patches a completed dominant color without re-querying the grid', () => {
    const reconcile = vi.spyOn(gridController, 'reconcile').mockResolvedValue(true);
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
    expect(reconcile).not.toHaveBeenCalled();
    stop();
  });
});
