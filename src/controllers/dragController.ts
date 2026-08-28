import { reorderFolderMembers, updateFolderMembership } from '../platform/folderApi';
import { gridController } from './gridController';
import { setTargetLifecycle, settleSelectionAfterMutation } from './entityMutations';
import type { EntityTarget } from '../shared/types/canonical';
import type { DropTarget, GridDragState } from '../features/grid/dragState';

const LIFECYCLE_BY_STATUS = ['inbox', 'active', 'trash'] as const;

export const dragController = {
  async executeDrop(
    itemIds: number[],
    target: DropTarget,
    sourceScope: GridDragState['sourceScope'],
  ): Promise<void> {
    if (itemIds.length === 0) return;
    const entityTarget: EntityTarget = { kind: 'explicit', root_ids: itemIds };

    if (target.kind === 'folder') {
      await updateFolderMembership(entityTarget, target.folderId, 'add');
      if (sourceScope?.kind === 'inbox' || sourceScope?.kind === 'trash') {
        await setTargetLifecycle(entityTarget, 'active');
      } else {
        settleSelectionAfterMutation();
      }
      return;
    }

    if (target.kind === 'status') {
      const lifecycle = LIFECYCLE_BY_STATUS[target.status];
      if (lifecycle) await setTargetLifecycle(entityTarget, lifecycle);
      return;
    }

    if (target.kind === 'reorder' && sourceScope?.kind === 'folder') {
      await reorderFolderMembers(sourceScope.folder_id, target.orderedItemIds);
      gridController.useManualFolderOrder();
      return;
    }
  },
};
