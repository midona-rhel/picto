import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { folderPickerPortalAtom } from '../../state/portals';
import { sidebarNodesAtom } from '../../state/sidebar';
import { foldersController } from '../../controllers/foldersController';
import { FolderPickerPanel } from './FolderPickerPanel';
import { readRecentFolderIds, setRecentFoldersLibrary } from '../../shared/hooks/useRecentFolders';

vi.mock('../../controllers/foldersController', () => ({
  foldersController: { create: vi.fn().mockResolvedValue('folder:9') },
}));

const store = getDefaultStore();

describe('FolderPickerPanel row context actions', () => {
  beforeEach(() => {
    setRecentFoldersLibrary('/test/Recent.library');
    localStorage.removeItem('picto-recent-folders:/test/Recent.library');
    vi.mocked(foldersController.create).mockClear();
    store.set(sidebarNodesAtom, [{
      id: 'folder:7',
      kind: 'folder',
      name: 'Reference',
      parent_id: 'section:folders',
    } as SidebarNodeDto]);
    store.set(folderPickerPortalAtom, { open: true });
  });

  it('creates a named child through the shared context menu', async () => {
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    fireEvent.contextMenu(screen.getByText('Reference'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'New Subfolder' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Folder name' }), { target: { value: 'Child' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(foldersController.create).toHaveBeenCalledWith('Child', 7));
  });

  it('creates a named sibling under the row parent', async () => {
    store.set(sidebarNodesAtom, [
      {
        id: 'folder:7', kind: 'folder', name: 'Parent', parent_id: 'section:folders',
      } as SidebarNodeDto,
      {
        id: 'folder:8', kind: 'folder', name: 'Reference', parent_id: 'folder:7',
      } as SidebarNodeDto,
    ]);
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    fireEvent.contextMenu(screen.getByText('Reference'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'New Sibling Folder' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Folder name' }), { target: { value: 'Sibling' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() => expect(foldersController.create).toHaveBeenCalledWith('Sibling', 7));
  });

  it('updates the selected matching rule for folder filters immediately', async () => {
    const onApplyFolderFilter = vi.fn();
    store.set(folderPickerPortalAtom, {
      open: true,
      selectedFolderIds: [7],
      excludedFolderIds: [],
      filterMatchMode: 'any',
      onApplyFolderFilter,
    });
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    fireEvent.click(screen.getByRole('button', { name: 'Match all' }));
    expect(onApplyFolderFilter).toHaveBeenCalledWith([7], [], 'all');
    expect(screen.getByText('L-Click')).toBeInTheDocument();
    expect(screen.getByText('R-Click')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Apply/ })).not.toBeInTheDocument();
  });

  it('records folders used by a filter and updates the mounted recent view', () => {
    const onApplyFolderFilter = vi.fn();
    store.set(folderPickerPortalAtom, {
      open: true,
      selectedFolderIds: [],
      excludedFolderIds: [],
      filterMatchMode: 'any',
      onApplyFolderFilter,
    });
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    fireEvent.click(screen.getByText('Reference'));
    expect(readRecentFolderIds()).toEqual([7]);
    fireEvent.click(screen.getByRole('button', { name: 'Recent folders' }));
    expect(screen.getByText('Reference')).toBeInTheDocument();
  });

  it('updates ordinary multi-folder assignment on each click without an Apply step', () => {
    const onApplyFolders = vi.fn();
    store.set(sidebarNodesAtom, [
      {
        id: 'folder:7', kind: 'folder', name: 'Reference', parent_id: 'section:folders',
      } as SidebarNodeDto,
      {
        id: 'folder:8', kind: 'folder', name: 'Archive', parent_id: 'section:folders',
      } as SidebarNodeDto,
    ]);
    store.set(folderPickerPortalAtom, {
      open: true,
      selectedFolderIds: [],
      onApplyFolders,
    });

    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    expect(screen.queryByRole('button', { name: /Apply/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Reference'));
    expect(onApplyFolders).toHaveBeenLastCalledWith([7]);
    fireEvent.click(screen.getByText('Archive'));
    expect(onApplyFolders).toHaveBeenLastCalledWith([7, 8]);
  });

  it('offers all, recent, and selected folder views', () => {
    localStorage.setItem('picto-recent-folders:/test/Recent.library', JSON.stringify([8]));
    store.set(sidebarNodesAtom, [
      { id: 'folder:7', kind: 'folder', name: 'Reference', parent_id: 'section:folders' } as SidebarNodeDto,
      { id: 'folder:8', kind: 'folder', name: 'Archive', parent_id: 'section:folders' } as SidebarNodeDto,
    ]);
    store.set(folderPickerPortalAtom, { open: true, selectedFolderIds: [7], onApplyFolders: vi.fn() });

    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    fireEvent.click(screen.getByRole('button', { name: 'Recent folders' }));
    expect(screen.getByText('Archive')).toBeInTheDocument();
    expect(screen.queryByText('Reference')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Selected folders' }));
    expect(screen.getByText('Reference')).toBeInTheDocument();
    expect(screen.queryByText('Archive')).not.toBeInTheDocument();
  });

  it('cycles folder views with Tab from the search field', () => {
    localStorage.setItem('picto-recent-folders:/test/Recent.library', JSON.stringify([8]));
    store.set(sidebarNodesAtom, [
      { id: 'folder:7', kind: 'folder', name: 'Reference', parent_id: 'section:folders' } as SidebarNodeDto,
      { id: 'folder:8', kind: 'folder', name: 'Archive', parent_id: 'section:folders' } as SidebarNodeDto,
    ]);
    store.set(folderPickerPortalAtom, { open: true, selectedFolderIds: [7], onApplyFolders: vi.fn() });
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);
    const search = screen.getByPlaceholderText('Search...');

    fireEvent.keyDown(search, { key: 'Tab' });
    expect(screen.getByText('Archive')).toBeInTheDocument();
    expect(screen.queryByText('Reference')).not.toBeInTheDocument();
    fireEvent.keyDown(search, { key: 'Tab' });
    expect(screen.getByText('Reference')).toBeInTheDocument();
    expect(screen.queryByText('Archive')).not.toBeInTheDocument();
    fireEvent.keyDown(search, { key: 'Tab' });
    expect(screen.getByText('Reference')).toBeInTheDocument();
    expect(screen.getByText('Archive')).toBeInTheDocument();
  });

  it('moves and toggles the focused folder with the advertised keys', () => {
    const onApplyFolders = vi.fn();
    store.set(sidebarNodesAtom, [
      { id: 'folder:7', kind: 'folder', name: 'Reference', parent_id: 'section:folders', sort_order: 0 } as SidebarNodeDto,
      { id: 'folder:8', kind: 'folder', name: 'Archive', parent_id: 'section:folders', sort_order: 1 } as SidebarNodeDto,
    ]);
    store.set(folderPickerPortalAtom, { open: true, selectedFolderIds: [], onApplyFolders });
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);
    const search = screen.getByPlaceholderText('Search...');

    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'Enter' });

    expect(onApplyFolders).toHaveBeenLastCalledWith([8]);
    expect(document.querySelector('[data-folder-id="8"]')?.className).toContain('rowFocused');
  });

  it('keeps keyboard selection single-choice when moving a folder', () => {
    const onApplyFolderParent = vi.fn();
    store.set(sidebarNodesAtom, [
      { id: 'folder:7', kind: 'folder', name: 'Reference', parent_id: 'section:folders', sort_order: 0 } as SidebarNodeDto,
      { id: 'folder:8', kind: 'folder', name: 'Archive', parent_id: 'section:folders', sort_order: 1 } as SidebarNodeDto,
    ]);
    store.set(folderPickerPortalAtom, { open: true, selectedFolderIds: [], onApplyFolderParent });
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);
    const search = screen.getByPlaceholderText('Search...');

    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    fireEvent.keyDown(search, { key: 'Enter' });
    fireEvent.keyDown(search, { key: 'Enter' });
    fireEvent.click(screen.getByRole('button', { name: 'Move' }));

    expect(onApplyFolderParent).toHaveBeenCalledWith(7);
  });

  it('keeps the selector compact and scrolls its folder tree internally', () => {
    render(<MantineProvider><FolderPickerPanel /></MantineProvider>);

    const panel = document.querySelector<HTMLElement>('[data-overlay-shell]');
    expect(panel?.style.width).toBe('360px');
    expect(panel?.style.height).toBe('480px');
    expect(screen.getByPlaceholderText('Search...')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Pin/ })).not.toBeInTheDocument();
    expect(screen.getByText('Switch')).toBeInTheDocument();
    expect(screen.getByText('Close')).toBeInTheDocument();
  });
});
