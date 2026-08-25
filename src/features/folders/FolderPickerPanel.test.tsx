import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import { folderPickerPortalAtom } from '../../state/portals';
import { sidebarNodesAtom } from '../../state/sidebar';
import { foldersController } from '../../controllers/foldersController';
import { FolderPickerPanel } from './FolderPickerPanel';

vi.mock('../../controllers/foldersController', () => ({
  foldersController: { create: vi.fn().mockResolvedValue('folder:9') },
}));

const store = getDefaultStore();

describe('FolderPickerPanel row context actions', () => {
  beforeEach(() => {
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

  it('applies the selected matching rule for folder filters', async () => {
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
    fireEvent.click(screen.getByRole('button', { name: 'Apply (1)' }));

    expect(onApplyFolderFilter).toHaveBeenCalledWith([7], [], 'all');
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
});
