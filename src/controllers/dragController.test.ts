import { beforeEach, describe, expect, it, vi } from 'vitest';

const { reorderFolderMembersMock, useManualFolderOrderMock } = vi.hoisted(() => ({
  reorderFolderMembersMock: vi.fn(),
  useManualFolderOrderMock: vi.fn(),
}));

vi.mock('../platform/entityApi', () => ({ setItemLifecycle: vi.fn() }));
vi.mock('../platform/folderApi', () => ({
  reorderFolderMembers: reorderFolderMembersMock,
  updateFolderMembership: vi.fn(),
}));
vi.mock('./gridController', () => ({
  gridController: { useManualFolderOrder: useManualFolderOrderMock },
}));

import { dragController } from './dragController';

describe('dragController folder ordering', () => {
  beforeEach(() => {
    reorderFolderMembersMock.mockReset();
    reorderFolderMembersMock.mockResolvedValue(undefined);
    useManualFolderOrderMock.mockReset();
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
});
