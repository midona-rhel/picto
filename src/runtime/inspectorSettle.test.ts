import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import {
  displayedInspectorEntityDataAtom,
  inspectorPinnedAtom,
} from '../state/inspector';

let eventHandler: ((event: { payload: { changes: Record<string, unknown> } }) => void) | undefined;

vi.mock('../platform/ipc', () => ({
  listen: vi.fn(async (
    _name: string,
    handler: (event: { payload: { changes: Record<string, unknown> } }) => void,
  ) => {
    eventHandler = handler;
    return () => {};
  }),
}));

const loadInspectorData = vi.hoisted(() => vi.fn());
vi.mock('../controllers/inspectorController', () => ({ loadInspectorData }));

import { startInspectorSettle } from './inspectorSettle';

const store = getDefaultStore();

describe('inspector runtime settling', () => {
  afterEach(() => {
    eventHandler = undefined;
    loadInspectorData.mockReset();
    store.set(displayedInspectorEntityDataAtom, null);
    store.set(inspectorPinnedAtom, false);
  });

  it('reloads the displayed entity after tag structure changes', async () => {
    store.set(displayedInspectorEntityDataAtom, { entity_hash: 'entity-1' } as never);
    const stop = startInspectorSettle();
    await Promise.resolve();

    eventHandler?.({
      payload: {
        changes: {
          tag_structure_changed: true,
          entity_hashes: ['another-entity'],
        },
      },
    });

    expect(loadInspectorData).toHaveBeenCalledWith('entity-1');
    stop();
  });
});
