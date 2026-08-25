import { describe, expect, it } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { availableFolderMoveTargets } from './Sidebar';

function folder(id: number, parentId: number | null): SidebarNodeDto {
  return {
    id: `folder:${id}`,
    kind: 'folder',
    name: `Folder ${id}`,
    parent_id: parentId == null ? 'section:folders' : `folder:${parentId}`,
  } as SidebarNodeDto;
}

describe('availableFolderMoveTargets', () => {
  it('omits the moving folder and its entire descendant subtree', () => {
    const nodes = [
      folder(1, null),
      folder(2, 1),
      folder(3, 2),
      folder(4, null),
      folder(5, 4),
    ];

    expect(availableFolderMoveTargets(nodes, 1)).toEqual([4, 5]);
    expect(availableFolderMoveTargets(nodes, 2)).toEqual([1, 4, 5]);
  });
});
