import { describe, expect, it } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { availableFolderMoveTargets, planSidebarTreeDrop } from './Sidebar';

function folder(id: number, parentId: number | null, sortOrder = 0): SidebarNodeDto {
  return {
    id: `folder:${id}`,
    kind: 'folder',
    name: `Folder ${id}`,
    parent_id: parentId == null ? 'section:folders' : `folder:${parentId}`,
    sort_order: sortOrder,
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

describe('planSidebarTreeDrop', () => {
  it('reorders a folder within its existing hierarchy', () => {
    const nodes = [folder(1, null, 0), folder(2, null, 1), folder(3, null, 2)];
    expect(planSidebarTreeDrop(nodes, ['folder:3'], 'folder:3', 'folder:1', 'before')).toEqual({
      movingIds: ['folder:3'],
      parentId: null,
      orderedChildIds: ['folder:3', 'folder:1', 'folder:2'],
    });
  });

  it('moves selected siblings and their child subtrees together', () => {
    const nodes = [
      folder(1, null, 0),
      folder(2, 1, 0),
      folder(3, null, 1),
      folder(4, null, 2),
      folder(5, 4, 0),
    ];
    expect(planSidebarTreeDrop(
      nodes,
      ['folder:1', 'folder:2', 'folder:3'],
      'folder:1',
      'folder:4',
      'inside',
    )).toEqual({
      movingIds: ['folder:1', 'folder:3'],
      parentId: 'folder:4',
      orderedChildIds: ['folder:5', 'folder:1', 'folder:3'],
    });
  });

  it('moves selected roots from different parents before one target', () => {
    const nodes = [
      folder(1, null, 0),
      folder(2, 1, 0),
      folder(3, null, 1),
      folder(4, 3, 0),
      folder(5, null, 2),
    ];
    expect(planSidebarTreeDrop(
      nodes,
      ['folder:2', 'folder:4'],
      'folder:2',
      'folder:5',
      'before',
    )).toEqual({
      movingIds: ['folder:2', 'folder:4'],
      parentId: null,
      orderedChildIds: ['folder:1', 'folder:3', 'folder:2', 'folder:4', 'folder:5'],
    });
  });

  it('rejects a target inside any selected subtree', () => {
    const nodes = [folder(1, null), folder(2, null), folder(3, 2)];
    expect(planSidebarTreeDrop(
      nodes,
      ['folder:1', 'folder:2'],
      'folder:1',
      'folder:3',
      'inside',
    )).toBeNull();
  });
});
