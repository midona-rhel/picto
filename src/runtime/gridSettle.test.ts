import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridController } from '../controllers/gridController';
import { gridActiveAtom, gridTransitionPhaseAtom } from '../state/grid';
import { startGridSettle } from './gridSettle';

const { callbacks } = vi.hoisted(() => ({
  callbacks: new Map<string, () => void>(),
}));

vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: {
    register: vi.fn((resource: string, callback: () => void) => {
      callbacks.set(resource, callback);
      return () => callbacks.delete(resource);
    }),
  },
}));

const store = getDefaultStore();

describe('grid invalidation', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    callbacks.clear();
    store.set(gridActiveAtom, true);
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
    store.set(gridActiveAtom, false);
    const stop = startGridSettle();

    callbacks.get('library')?.();

    expect(reload).not.toHaveBeenCalled();
    stop();
  });
});
