import { beforeEach, describe, expect, it, vi } from 'vitest';

const { reorderFolderMembersMock, updateTargetFolderMembershipMock, setTargetLifecycleMock, settleSelectionAfterMutationMock, useManualFolderOrderMock } = vi.hoisted(() => ({
  reorderFolderMembersMock: vi.fn(),
  updateTargetFolderMembershipMock: vi.fn(),
  setTargetLifecycleMock: vi.fn(),
  settleSelectionAfterMutationMock: vi.fn(),
  useManualFolderOrderMock: vi.fn(),
}));

vi.mock('../platform/folderApi', () => ({
  reorderFolderMembers: reorderFolderMembersMock,
}));
vi.mock('./entityMutations', () => ({
  setTargetLifecycle: setTargetLifecycleMock,
  settleSelectionAfterMutation: settleSelectionAfterMutationMock,
  updateTargetFolderMembership: updateTargetFolderMembershipMock,
}));
vi.mock('./gridController', () => ({
  gridController: { useManualFolderOrder: useManualFolderOrderMock },
}));

import { dragController } from './dragController';

describe('dragController folder ordering', () => {
  beforeEach(() => {
    reorderFolderMembersMock.mockReset();
    reorderFolderMembersMock.mockResolvedValue(undefined);
    updateTargetFolderMembershipMock.mockReset();
    updateTargetFolderMembershipMock.mockResolvedValue(undefined);
    useManualFolderOrderMock.mockReset();
    setTargetLifecycleMock.mockReset();
    setTargetLifecycleMock.mockResolvedValue(undefined);
    settleSelectionAfterMutationMock.mockReset();
  });

  it('shows the persisted order after a successful folder reorder', async () => {
    await dragController.executeDrop(
      [2],
      { kind: 'reorder', orderedItemIds: [2, 1] },
      { kind: 'folder', folder_id: 7 },
    );

    expect(reorderFolderMembersMock).toHaveBeenCalledWith(7, [2, 1]);
    expect(useManualFolderOrderMock).toHaveBeenCalledOnce();
  });

  it('does not change the visible sort when persistence fails', async () => {
    reorderFolderMembersMock.mockRejectedValueOnce(new Error('reorder failed'));

    await expect(dragController.executeDrop(
      [2],
      { kind: 'reorder', orderedItemIds: [2, 1] },
      { kind: 'folder', folder_id: 7 },
    )).rejects.toThrow('reorder failed');

    expect(useManualFolderOrderMock).not.toHaveBeenCalled();
  });

  it('routes a manual drop on Trash through the canonical lifecycle mutation', async () => {
    await dragController.executeDrop(
      [2, 3],
      { kind: 'status', status: 2 },
      { kind: 'all' },
    );

    expect(setTargetLifecycleMock).toHaveBeenCalledWith(
      { kind: 'explicit', root_ids: [2, 3] },
      'trash',
    );
  });

  it('settles selection after dropping selected items into another folder', async () => {
    await dragController.executeDrop(
      [2, 3],
      { kind: 'folder', folderId: 9, nodeId: 'folder:9' },
      { kind: 'folder', folder_id: 7 },
    );

    expect(settleSelectionAfterMutationMock).toHaveBeenCalledOnce();
    expect(updateTargetFolderMembershipMock).toHaveBeenCalledWith(
      { kind: 'explicit', root_ids: [2, 3] },
      9,
      'add',
    );
  });

  it('keeps selection when the folder drop mutation fails', async () => {
    updateTargetFolderMembershipMock.mockRejectedValueOnce(new Error('drop failed'));

    await expect(dragController.executeDrop(
      [2, 3],
      { kind: 'folder', folderId: 9, nodeId: 'folder:9' },
      { kind: 'folder', folder_id: 7 },
    )).rejects.toThrow('drop failed');

    expect(settleSelectionAfterMutationMock).not.toHaveBeenCalled();
  });

  it('preserves selection for an in-place manual reorder', async () => {
    await dragController.executeDrop(
      [2],
      { kind: 'reorder', orderedItemIds: [2, 1] },
      { kind: 'folder', folder_id: 7 },
    );

    expect(settleSelectionAfterMutationMock).not.toHaveBeenCalled();
  });
});
