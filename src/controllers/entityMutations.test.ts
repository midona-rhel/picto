import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridSelectionAtom } from '../state/selection';

const { loadInspectorDataMock, setItemLifecycleMock } = vi.hoisted(() => ({
  loadInspectorDataMock: vi.fn(),
  setItemLifecycleMock: vi.fn(),
}));

vi.mock('../platform/entityApi', () => ({
  applyEntityTags: vi.fn(),
  deleteItems: vi.fn(),
  getSelectionSummary: vi.fn(),
  patchMediaEntities: vi.fn(),
  renameItem: vi.fn(),
  setItemLifecycle: setItemLifecycleMock,
}));
vi.mock('../platform/folderApi', () => ({
  removeEntitiesFromFolder: vi.fn(),
  updateFolderMembership: vi.fn(),
}));
vi.mock('./inspectorController', () => ({ loadInspectorData: loadInspectorDataMock }));
vi.mock('../shared/hooks/useRecentItems', () => ({ recordRecentItems: vi.fn() }));

import {
  permanentlyDeleteTarget,
  setItemRating,
  setTargetLifecycle,
  settleSelectionAfterMutation,
} from './entityMutations';

describe('setTargetLifecycle', () => {
  const store = getDefaultStore();

  beforeEach(() => {
    setItemLifecycleMock.mockReset();
    setItemLifecycleMock.mockResolvedValue(undefined);
    loadInspectorDataMock.mockReset();
    store.set(gridSelectionAtom, {
      mode: 'explicit',
      itemIds: new Set([4, 7]),
      excludedItemIds: new Set<number>(),
      folderNodeIds: new Set<string>(),
      anchor: { kind: 'item', id: 4 },
    });
  });

  it('clears selection after moving the selected target to Trash', async () => {
    await setTargetLifecycle({ kind: 'explicit', item_ids: [4, 7] }, 'trash');

    expect(setItemLifecycleMock).toHaveBeenCalledWith(
      { kind: 'explicit', item_ids: [4, 7] },
      'trash',
    );
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
    expect(store.get(gridSelectionAtom).anchor).toBeNull();
  });

  it('clears selection after restoring selected items from Trash', async () => {
    await setTargetLifecycle({ kind: 'explicit', item_ids: [4, 7] }, 'active');

    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('does not clear selection when moving to Trash fails', async () => {
    setItemLifecycleMock.mockRejectedValueOnce(new Error('trash failed'));

    await expect(setTargetLifecycle(
      { kind: 'explicit', item_ids: [4, 7] },
      'trash',
    )).rejects.toThrow('trash failed');
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([4, 7]));
  });

  it('clears only after permanent deletion succeeds', async () => {
    const { deleteItems } = await import('../platform/entityApi');
    vi.mocked(deleteItems).mockResolvedValueOnce(undefined as never);

    await permanentlyDeleteTarget({ kind: 'explicit', item_ids: [4, 7] });
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('preserves selection when permanent deletion fails', async () => {
    const { deleteItems } = await import('../platform/entityApi');
    vi.mocked(deleteItems).mockRejectedValueOnce(new Error('delete failed'));

    await expect(permanentlyDeleteTarget(
      { kind: 'explicit', item_ids: [4, 7] },
    )).rejects.toThrow('delete failed');
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([4, 7]));
  });

  it('exposes one settlement owner for successful relocation mutations', () => {
    settleSelectionAfterMutation();
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
    expect(store.get(gridSelectionAtom).anchor).toBeNull();
  });

  it('sends the selected rating through the canonical metadata patch', async () => {
    const { patchMediaEntities } = await import('../platform/entityApi');
    vi.mocked(patchMediaEntities).mockResolvedValueOnce(undefined as never);

    await setItemRating(4, 5);

    expect(patchMediaEntities).toHaveBeenCalledWith(
      { kind: 'explicit', item_ids: [4] },
      { rating: 5 },
    );
  });
});
