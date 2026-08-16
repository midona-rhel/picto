import { setEntityStatus } from '../platform/entityApi';
import { reorderFolderMembers, updateFolderMembership } from '../platform/folderApi';
import type { EntityTarget } from '../shared/types/canonical';
import type { DropTarget, GridDragState } from '../features/grid/dragState';

export const dragController = {
  async executeDrop(
    hashes: string[],
    target: DropTarget,
    sourceScope: GridDragState['sourceScope'],
  ): Promise<void> {
    const entityTarget: EntityTarget = {
      kind: 'entity_hashes',
      entity_hashes: hashes.filter((h) => !h.startsWith('folder:')),
    };
    const entityHashes = entityTarget.entity_hashes ?? [];
    if (entityHashes.length === 0) return;

    if (target.kind === 'folder') {
      await updateFolderMembership(entityTarget, target.folderId, 'add');
      if (sourceScope?.key === 'inbox' || sourceScope?.key === 'trash') {
        await setEntityStatus(entityTarget, 1);
      }
      return;
    }

    if (target.kind === 'status') {
      await setEntityStatus(entityTarget, target.status);
      return;
    }

    if (sourceScope?.kind === 'folder' && sourceScope.id != null) {
      await reorderFolderMembers(sourceScope.id, target.orderedEntityIds);
      return;
    }

  },
};
