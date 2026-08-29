import { getDefaultStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { SidebarNodeDto } from '../shared/types/canonical';
import { gridChildFoldersAtom, gridSessionAtom } from './grid';
import { sidebarNodesAtom } from './sidebar';

const store = getDefaultStore();

function folder(id: number, parentId: string, sortOrder: number): SidebarNodeDto {
  return {
    id: `folder:${id}`,
    kind: 'folder',
    parent_id: parentId,
    name: `Folder ${id}`,
    sort_order: sortOrder,
    count: 0,
    freshness: 'exact',
    selectable: true,
  };
}

describe('folder cards in the grid', () => {
  it('shows every descendant in stable depth-first order', () => {
    const initialSession = store.get(gridSessionAtom);
    const initialNodes = store.get(sidebarNodesAtom);
    store.set(sidebarNodesAtom, [
      folder(4, 'folder:2', 0),
      folder(3, 'folder:1', 1),
      folder(2, 'folder:1', 0),
      folder(5, 'folder:3', 0),
      folder(6, 'section:folders', 0),
    ]);
    store.set(gridSessionAtom, {
      ...initialSession,
      scope: { kind: 'folder', folder_id: 1 },
      view: { ...initialSession.view, showSubfolders: false },
    });

    expect(store.get(gridChildFoldersAtom).map((node) => node.id)).toEqual([
      'folder:2',
      'folder:3',
    ]);

    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      view: { ...initialSession.view, showSubfolders: true },
    });

    expect(store.get(gridChildFoldersAtom).map((node) => node.id)).toEqual([
      'folder:2',
      'folder:4',
      'folder:3',
      'folder:5',
    ]);

    store.set(sidebarNodesAtom, initialNodes);
    store.set(gridSessionAtom, initialSession);
  });
});
