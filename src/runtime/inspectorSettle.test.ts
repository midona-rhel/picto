import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorPinnedAtom,
} from '../state/inspector';
import { gridSessionAtom } from '../state/grid';
import { activeNodeIdAtom } from '../state/navigation';
import { emptyGridSelection, gridSelectionAtom } from '../state/selection';

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
    store.set(displayedInspectorTargetAtom, { kind: 'none' });
    store.set(inspectorPinnedAtom, false);
    store.set(gridSelectionAtom, emptyGridSelection());
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: false });
  });

  it('reloads the displayed item after a library invalidation', () => {
    store.set(displayedInspectorItemDetailsAtom, { item_id: 17 } as never);
    const stop = startInspectorSettle();

    callbacks.get('library')?.();

    expect(loadInspectorData).toHaveBeenCalledWith(17);
    stop();
  });

  it('displays a query-wide selection and returns to its scope when cleared', () => {
    store.set(activeNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      totalCount: 25,
      active: true,
    });
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: 'system:active' });
    const stop = startInspectorSettle();

    store.set(gridSelectionAtom, {
      ...emptyGridSelection(),
      mode: 'query_results',
    });

    expect(store.get(displayedInspectorTargetAtom)).toEqual({
      kind: 'multi',
      count: 25,
      selectionMode: 'query_results',
    });

    store.set(gridSelectionAtom, emptyGridSelection());
    expect(store.get(displayedInspectorTargetAtom)).toEqual({
      kind: 'scope',
      nodeId: 'system:active',
    });
    stop();
  });
});
