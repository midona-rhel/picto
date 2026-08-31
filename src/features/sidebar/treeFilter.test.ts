import { describe, expect, it } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { filterSidebarTree } from './treeFilter';

const node = (id: string, name: string, parent_id: string | null = null) => ({ id, name, parent_id }) as SidebarNodeDto;

describe('filterSidebarTree', () => {
  it('keeps matching folders and their ancestors without including unrelated siblings', () => {
    const nodes = [
      node('section:folders', 'Folders'),
      node('folder:1', 'References', 'section:folders'),
      node('folder:2', 'Type Samples', 'folder:1'),
      node('folder:3', 'Archive', 'section:folders'),
    ];

    expect(filterSidebarTree(nodes, 'type').map(({ id }) => id)).toEqual([
      'section:folders', 'folder:1', 'folder:2',
    ]);
  });

  it('filters smart-folder trees independently and leaves an empty query untouched', () => {
    const smartNodes = [
      node('section:smart_folders', 'Smart Folders'),
      node('smart:1', 'Needs review', 'section:smart_folders'),
      node('smart:2', 'Favorites', 'section:smart_folders'),
    ];

    expect(filterSidebarTree(smartNodes, 'review').map(({ id }) => id)).toEqual([
      'section:smart_folders', 'smart:1',
    ]);
    expect(filterSidebarTree(smartNodes, '   ')).toBe(smartNodes);
  });

  it('normalizes filtered tree ordering so matching siblings sort alphabetically', () => {
    const smartNodes = [
      { ...node('smart:1', 'Zebra review', 'section:smart_folders'), sort_order: 0 },
      { ...node('smart:2', 'Alpha review', 'section:smart_folders'), sort_order: 1 },
    ];

    expect(filterSidebarTree(smartNodes, 'review').map(({ name, sort_order }) => (
      [name, sort_order]
    ))).toEqual([
      ['Alpha review', 0],
      ['Zebra review', 0],
    ]);
  });
});
