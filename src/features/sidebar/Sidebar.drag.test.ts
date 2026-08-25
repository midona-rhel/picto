import { describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { resolveSidebarTreeDrop } from './Sidebar';

function row(attribute: 'folderDropId' | 'smartDropId', id: string): HTMLElement {
  const element = document.createElement('div');
  element.dataset[attribute] = id;
  element.getBoundingClientRect = vi.fn(() => ({ top: 0, height: 30 } as DOMRect));
  return element;
}

const nodes = [
  { id: 'folder:1', parent_id: null },
  { id: 'folder:2', parent_id: null },
] as SidebarNodeDto[];

describe('sidebar tree drop eligibility', () => {
  it('lets folders target folders but never smart folders or headers', () => {
    expect(resolveSidebarTreeDrop(row('folderDropId', '2'), 15, 'folder:1', nodes)).toEqual({
      targetId: 'folder:2', position: 'inside',
    });
    expect(resolveSidebarTreeDrop(row('smartDropId', '2'), 15, 'folder:1', nodes)).toBeNull();
    expect(resolveSidebarTreeDrop(document.createElement('div'), 15, 'folder:1', nodes)).toBeNull();
  });

  it('lets smart folders target only smart folders', () => {
    const smartNodes = [
      { id: 'smart:1', parent_id: null },
      { id: 'smart:2', parent_id: null },
    ] as SidebarNodeDto[];
    expect(resolveSidebarTreeDrop(row('smartDropId', '2'), 15, 'smart:1', smartNodes)).toEqual({
      targetId: 'smart:2', position: 'inside',
    });
    expect(resolveSidebarTreeDrop(row('folderDropId', '2'), 15, 'smart:1', smartNodes)).toBeNull();
  });
});
