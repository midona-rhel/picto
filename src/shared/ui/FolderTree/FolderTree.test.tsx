import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../types/canonical';
import { FolderTree } from './FolderTree';

const folder = {
  id: 'folder:7',
  kind: 'folder',
  name: 'Reference',
  parent_id: 'section:folders',
} as SidebarNodeDto;

const childFolder = {
  id: 'folder:8',
  kind: 'folder',
  name: 'Child',
  parent_id: 'folder:7',
} as SidebarNodeDto;

describe('FolderTree context target', () => {
  it('routes normal row context actions without changing selection', () => {
    const onToggle = vi.fn();
    const onContextMenu = vi.fn();
    render(
      <FolderTree
        nodes={[folder]}
        selected={new Set()}
        onToggle={onToggle}
        onContextMenu={onContextMenu}
      />,
    );

    fireEvent.contextMenu(screen.getByText('Reference'));
    expect(onContextMenu).toHaveBeenCalledWith(7, expect.anything());
    expect(onToggle).not.toHaveBeenCalled();
  });

  it('keeps filter exclusion as the right-click owner in filter mode', () => {
    const onContextMenu = vi.fn();
    const onExclude = vi.fn();
    render(
      <FolderTree
        nodes={[folder]}
        selected={new Set()}
        onToggle={vi.fn()}
        onExclude={onExclude}
        onContextMenu={onContextMenu}
      />,
    );

    fireEvent.contextMenu(screen.getByText('Reference'));
    expect(onExclude).toHaveBeenCalledWith(7, expect.anything());
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it('keeps selected-row highlighting out of the tree indentation', () => {
    render(
      <FolderTree
        nodes={[folder, childFolder]}
        selected={new Set([8])}
        onToggle={vi.fn()}
      />,
    );

    const childRow = screen.getByText('Child').closest('[style]') as HTMLElement;
    expect(childRow.style.getPropertyValue('--folder-row-indent')).toBe('28px');
    expect(childRow.querySelector('[data-folder-tree-branch]')).toHaveAttribute('data-folder-tree-branch', 'last');
  });
});
