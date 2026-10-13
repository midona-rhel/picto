import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridSelectionAtom } from '../state/selection';
import { permanentDeletesInFlightAtom } from '../state/mutationActivity';
import { quickLookSessionAtom, viewerSessionAtom } from '../state/viewer';
import { confirmModalAtom } from '../state/modals';
import { clearNotifications, getCurrentNotification } from '../shared/lib/notifications';
import { getHistoryState } from '../platform/historyApi';
import { getSelectionSummary, deleteItems, rejectDeleteItems } from '../platform/entityApi';
import { getSettings } from '../platform/settingsApi';
import type { EntityTarget, EntityViewQuery, SelectionSummary } from '../shared/types/canonical';

const { loadInspectorDataMock, setItemLifecycleMock } = vi.hoisted(() => ({
  loadInspectorDataMock: vi.fn(),
  setItemLifecycleMock: vi.fn(),
}));

vi.mock('../platform/entityApi', () => ({
  applyEntityTags: vi.fn(),
  deleteItems: vi.fn(),
  rejectDeleteItems: vi.fn(),
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
vi.mock('../platform/settingsApi', () => ({ getSettings: vi.fn() }));
vi.mock('../platform/historyApi', () => ({ getHistoryState: vi.fn() }));
vi.mock('../shared/hooks/useRecentItems', () => ({ recordRecentItems: vi.fn() }));

import {
  permanentlyDeleteTarget,
  rejectTarget,
  setItemRating,
  setTargetLifecycle,
  settleSelectionAfterMutation,
} from './entityMutations';

const inboxQuery: EntityViewQuery = {
  scope: { kind: 'inbox' },
  view: { filter: { kind: 'all', value: [] }, sort: { field: 'name', direction: 'ascending', random_seed: null } },
};

describe('setTargetLifecycle', () => {
  const store = getDefaultStore();

  beforeEach(() => {
    vi.mocked(getHistoryState).mockReset().mockResolvedValue({ undo: null, redo: null });
    vi.mocked(getSelectionSummary).mockReset().mockResolvedValue({ selected_count: 2, media_count: 2 } as SelectionSummary);
    vi.mocked(getSettings).mockReset().mockResolvedValue({ rejectAction: 'delete', undoDeleteLimit: 500 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(deleteItems).mockReset().mockResolvedValue(undefined as never);
    vi.mocked(rejectDeleteItems).mockReset().mockResolvedValue(undefined as never);
    clearNotifications();
    store.set(confirmModalAtom, { open: false, title: '', message: '', onConfirm: () => {} });
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
    store.set(permanentDeletesInFlightAtom, 0);
    store.set(viewerSessionAtom, null);
    store.set(quickLookSessionAtom, null);
  });

  it('clears selection after moving the selected target to Trash', async () => {
    await setTargetLifecycle({ kind: 'explicit', root_ids: [4, 7] }, 'trash');

    expect(setItemLifecycleMock).toHaveBeenCalledWith(
      { kind: 'explicit', root_ids: [4, 7] },
      'trash',
    );
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
    expect(store.get(gridSelectionAtom).anchor).toBeNull();
  });

  it.each(['trash', 'delete'] as const)('rejects using the saved %s preference', async (rejectAction) => {
    const { getSettings } = await import('../platform/settingsApi');
    const { rejectDeleteItems } = await import('../platform/entityApi');
    vi.mocked(getSettings).mockResolvedValue({ rejectAction, undoDeleteLimit: 500 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(rejectDeleteItems).mockReset();
    const target = { kind: 'explicit', root_ids: [4, 7] } as const;

    await rejectTarget({ ...target, root_ids: [...target.root_ids] });

    if (rejectAction === 'delete') {
      expect(rejectDeleteItems).toHaveBeenCalledWith(target);
      expect(setItemLifecycleMock).not.toHaveBeenCalled();
    } else {
      expect(setItemLifecycleMock).toHaveBeenCalledWith(target, 'trash');
      expect(rejectDeleteItems).not.toHaveBeenCalled();
    }
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('clears selection after restoring selected items from Trash', async () => {
    await setTargetLifecycle({ kind: 'explicit', root_ids: [4, 7] }, 'active');

    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('does not clear selection when moving to Trash fails', async () => {
    setItemLifecycleMock.mockRejectedValueOnce(new Error('trash failed'));

    await expect(setTargetLifecycle(
      { kind: 'explicit', root_ids: [4, 7] },
      'trash',
    )).rejects.toThrow('trash failed');
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([4, 7]));
  });

  it('clears only after permanent deletion succeeds', async () => {
    const { deleteItems } = await import('../platform/entityApi');
    vi.mocked(deleteItems).mockResolvedValueOnce(undefined as never);

    await permanentlyDeleteTarget({ kind: 'explicit', root_ids: [4, 7] });
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('suppresses stale detail refreshes and closes viewers for the deleted root', async () => {
    const { deleteItems } = await import('../platform/entityApi');
    let finishDelete!: () => void;
    vi.mocked(deleteItems).mockReturnValueOnce(new Promise<void>((resolve) => {
      finishDelete = resolve;
    }) as never);
    store.set(viewerSessionAtom, { currentIndex: 0, currentItemId: 4 });
    store.set(quickLookSessionAtom, { currentIndex: 0, currentItemId: 4 });

    const deletion = permanentlyDeleteTarget({ kind: 'explicit', root_ids: [4] });
    expect(store.get(permanentDeletesInFlightAtom)).toBe(1);
    expect(store.get(viewerSessionAtom)?.currentItemId).toBe(4);

    await vi.waitFor(() => expect(finishDelete).toBeDefined());
    finishDelete();
    await deletion;

    expect(store.get(permanentDeletesInFlightAtom)).toBe(0);
    expect(store.get(viewerSessionAtom)).toBeNull();
    expect(store.get(quickLookSessionAtom)).toBeNull();
  });

  it('preserves selection when permanent deletion fails', async () => {
    const { deleteItems } = await import('../platform/entityApi');
    vi.mocked(deleteItems).mockRejectedValueOnce(new Error('delete failed'));

    await expect(permanentlyDeleteTarget(
      { kind: 'explicit', root_ids: [4, 7] },
    )).rejects.toThrow('delete failed');
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([4, 7]));
    expect(store.get(permanentDeletesInFlightAtom)).toBe(0);
  });

  it.each([
    { limit: 0, roots: 1, media: 1, target: { kind: 'explicit', root_ids: [4] } },
    { limit: 2, roots: 1, media: 3, target: { kind: 'explicit', root_ids: [4] } },
    { limit: 2, roots: 300, media: 300, target: { kind: 'query', query: inboxQuery, excluded_root_ids: [7] } },
    { limit: 2, roots: 3, media: 3, target: { kind: 'range', query: inboxQuery, spans: [{ anchor_root_id: 4, focus_root_id: 7 }] } },
  ])('confirms nonundoable rejects using backend counts: $limit/$media', async ({ limit, roots, media, target }) => {
    vi.mocked(getSettings).mockResolvedValue({ rejectAction: 'delete', undoDeleteLimit: limit } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getSelectionSummary).mockResolvedValue({ selected_count: roots, media_count: media } as SelectionSummary);

    await rejectTarget(target as EntityTarget);

    expect(getSelectionSummary).toHaveBeenCalledWith(target);
    expect(rejectDeleteItems).not.toHaveBeenCalled();
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([4, 7]));
    expect(store.get(confirmModalAtom).message).toContain('cannot be undone');
    store.get(confirmModalAtom).onConfirm();
    await vi.waitFor(() => expect(rejectDeleteItems).toHaveBeenCalledWith(target));
    await vi.waitFor(() => expect(store.get(gridSelectionAtom).itemIds.size).toBe(0));
  });

  it('does not reject when the backend selection cannot be resolved', async () => {
    vi.mocked(getSelectionSummary).mockRejectedValue(new Error('selection expired'));
    await expect(rejectTarget({ kind: 'explicit', root_ids: [4] })).rejects.toThrow('selection expired');
    expect(rejectDeleteItems).not.toHaveBeenCalled();
    expect(store.get(confirmModalAtom).open).toBe(false);
  });

  it('requires confirmation if the undo limit cannot be read', async () => {
    vi.mocked(getSettings).mockResolvedValueOnce({ rejectAction: 'delete' } as Awaited<ReturnType<typeof getSettings>>)
      .mockRejectedValueOnce(new Error('settings unavailable'));
    await rejectTarget({ kind: 'explicit', root_ids: [4] });
    expect(rejectDeleteItems).not.toHaveBeenCalled();
    expect(store.get(confirmModalAtom).open).toBe(true);
  });

  it.each(['items.delete', 'items.reject'])('does not offer an older %s entry as Undo for a new delete', async (command) => {
    const entry = { entry_id: 10, command, label: 'Delete', count: 1, detail: null };
    vi.mocked(getSettings).mockResolvedValue({ rejectAction: 'delete', undoDeleteLimit: 0 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getHistoryState).mockResolvedValue({ undo: entry, redo: null });
    const remove = command === 'items.delete' ? permanentlyDeleteTarget : rejectTarget;
    await remove({ kind: 'explicit', root_ids: [4] });
    if (command === 'items.reject') {
      store.get(confirmModalAtom).onConfirm();
      await vi.waitFor(() => expect(getHistoryState).toHaveBeenCalledTimes(2));
    }
    expect(getCurrentNotification()).toBeNull();

    vi.mocked(getSettings).mockResolvedValue({ rejectAction: 'delete', undoDeleteLimit: 500 } as Awaited<ReturnType<typeof getSettings>>);
    vi.mocked(getHistoryState).mockResolvedValueOnce({ undo: entry, redo: null })
      .mockResolvedValueOnce({ undo: { ...entry, entry_id: 11 }, redo: null });
    await remove({ kind: 'explicit', root_ids: [7] });
    expect(getCurrentNotification()?.action?.label).toBe('Undo');
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
      { kind: 'explicit', root_ids: [4] },
      { rating: 'five' },
    );
  });
});
