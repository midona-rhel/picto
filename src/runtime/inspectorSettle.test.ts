import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { displayedInspectorItemDetailsAtom, inspectorPinnedAtom } from '../state/inspector';

const { callbacks } = vi.hoisted(() => ({ callbacks: new Map<string, () => void>() }));
vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: {
    register: vi.fn((resource: string, callback: () => void) => {
      callbacks.set(resource, callback);
      return () => callbacks.delete(resource);
    }),
  },
}));

const loadInspectorData = vi.hoisted(() => vi.fn());
vi.mock('../controllers/inspectorController', () => ({ loadInspectorData, cancelInspectorLoad: vi.fn() }));

import { startInspectorSettle } from './inspectorSettle';

const store = getDefaultStore();

describe('inspector invalidation', () => {
  afterEach(() => {
    callbacks.clear();
    loadInspectorData.mockReset();
    store.set(displayedInspectorItemDetailsAtom, null);
    store.set(inspectorPinnedAtom, false);
  });

  it('reloads the displayed item after a library invalidation', () => {
    store.set(displayedInspectorItemDetailsAtom, { item_id: 17 } as never);
    const stop = startInspectorSettle();

    callbacks.get('library')?.();

    expect(loadInspectorData).toHaveBeenCalledWith(17);
    stop();
  });
});
