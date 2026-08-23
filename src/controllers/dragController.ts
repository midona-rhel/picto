import { setItemLifecycle } from '../platform/entityApi';
import { reorderFolderMembers, updateFolderMembership } from '../platform/folderApi';
import { gridController } from './gridController';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { DropTarget, GridDragState } from '../features/grid/dragState';

const LIFECYCLE_BY_STATUS = ['inbox', 'active', 'trash'] as const;

export const dragController = {
  async executeDrop(
    itemIds: number[],
    target: DropTarget,
    sourceScope: GridDragState['sourceScope'],
  ): Promise<void> {
    if (itemIds.length === 0) return;
    const entityTarget: ItemTarget = { kind: 'explicit', item_ids: itemIds };

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
      gridController.useManualFolderOrder();
      return;
    }
  },
};
