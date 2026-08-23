import { setItemLifecycle } from '../platform/entityApi';
import { reorderFolderMembers, updateFolderMembership } from '../platform/folderApi';
import type { EntityTarget } from '../shared/types/canonical';
import type { DropTarget, GridDragState } from '../features/grid/dragState';

const LIFECYCLE_BY_STATUS = ['inbox', 'active', 'trash'] as const;

export const dragController = {
  async executeDrop(
    itemIdKeys: string[],
    target: DropTarget,
    sourceScope: GridDragState['sourceScope'],
  ): Promise<void> {
    const itemIds = itemIdKeys
      .map(Number)
      .filter((itemId) => Number.isSafeInteger(itemId));
    if (itemIds.length === 0) return;
    const entityTarget: EntityTarget = { kind: 'explicit', item_ids: itemIds };

    if (target.kind === 'folder') {
      await updateFolderMembership(entityTarget, target.folderId, 'add');
      if (sourceScope?.kind === 'inbox' || sourceScope?.kind === 'trash') {
        await setItemLifecycle(entityTarget, 'active');
      }
      return;
    }

    if (target.kind === 'status') {
      const lifecycle = LIFECYCLE_BY_STATUS[target.status];
      if (lifecycle) await setItemLifecycle(entityTarget, lifecycle);
      return;
    }

    if (target.kind === 'reorder' && sourceScope?.kind === 'folder') {
      await reorderFolderMembers(sourceScope.folder_id, target.orderedItemIds);
      return;
    }
  },
};
