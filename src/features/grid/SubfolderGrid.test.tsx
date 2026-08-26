import { act, fireEvent, render, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';

const { getCoverHashesMock } = vi.hoisted(() => ({ getCoverHashesMock: vi.fn() }));
const invalidationMock = vi.hoisted(() => ({ folders: null as null | (() => void) }));

vi.mock('../../controllers/foldersController', () => ({
  foldersController: { getCoverHashes: getCoverHashesMock },
}));
vi.mock('../../runtime/libraryInvalidation', () => ({
  libraryInvalidation: {
    register: (resource: string, callback: () => void) => {
      if (resource === 'folders') invalidationMock.folders = callback;
      return () => {
        if (invalidationMock.folders === callback) invalidationMock.folders = null;
      };
    },
  },
}));

import { SubfolderGrid } from './SubfolderGrid';

function folder(id: number): SidebarNodeDto {
  return {
    id: `folder:${id}`,
    kind: 'folder',
    parent_id: 'folder:9',
    name: `Folder ${id}`,
    count: 0,
    freshness: 'exact',
    selectable: true,
  };
}

describe('SubfolderGrid cover loading', () => {
  it('keeps child folders visible when the folder has no direct media', () => {
    getCoverHashesMock.mockReturnValueOnce(new Promise(() => {}));

    const { getByText, queryByText } = render(
      <SubfolderGrid
        childFolders={[folder(1), folder(2)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );

    expect(getByText('Folders (2)')).toBeTruthy();
    expect(getByText('Folder 1')).toBeTruthy();
    expect(getByText('Folder 2')).toBeTruthy();
    expect(queryByText('Content (0)')).toBeNull();
  });

  it('loads every child cover through one batch request', async () => {
    getCoverHashesMock.mockResolvedValueOnce([
      { folder_id: 1, entity_hash: 'cover-1', mime_type: 'image/jpeg' },
      { folder_id: 2, entity_hash: null, mime_type: null },
    ]);

    const { container } = render(
      <SubfolderGrid
        childFolders={[folder(1), folder(2)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );

    await waitFor(() => expect(getCoverHashesMock).toHaveBeenCalledWith([1, 2]));
    await waitFor(() => expect(container.querySelectorAll('img')).toHaveLength(1));
    expect(container.querySelector('img')?.getAttribute('src')).toBe('media://localhost/thumb/cover-1.jpg');
  });

  it('keeps covers mounted while zoom changes recreate the equivalent folder list', async () => {
    getCoverHashesMock.mockResolvedValueOnce([
      { folder_id: 1, entity_hash: 'cover-1', mime_type: 'image/jpeg' },
    ]);

    const { container, rerender } = render(
      <SubfolderGrid
        childFolders={[folder(1)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull());
    const cover = container.querySelector('img');
    const requestCount = getCoverHashesMock.mock.calls.length;

    rerender(
      <SubfolderGrid
        childFolders={[folder(1)]}
        targetSize={310}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );

    expect(getCoverHashesMock).toHaveBeenCalledTimes(requestCount);
    expect(container.querySelector('img')).toBe(cover);
  });

  it('keeps the decoded cover mounted while a folder invalidation refreshes it', async () => {
    let resolveRefresh: ((value: Array<{ folder_id: number; entity_hash: string | null; mime_type: string | null }>) => void) | null = null;
    getCoverHashesMock
      .mockResolvedValueOnce([{ folder_id: 1, entity_hash: 'cover-1', mime_type: 'image/jpeg' }])
      .mockReturnValueOnce(new Promise((resolve) => { resolveRefresh = resolve; }));

    const { container } = render(
      <SubfolderGrid
        childFolders={[folder(1)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );
    await waitFor(() => expect(container.querySelector('img')?.getAttribute('src')).toContain('cover-1'));
    const cover = container.querySelector('img');
    const requestCount = getCoverHashesMock.mock.calls.length;

    act(() => invalidationMock.folders?.());
    await waitFor(() => expect(getCoverHashesMock).toHaveBeenCalledTimes(requestCount + 1));
    expect(container.querySelector('img')).toBe(cover);
    expect(container.querySelector('img')?.getAttribute('src')).toContain('cover-1');

    await act(async () => {
      resolveRefresh?.([{ folder_id: 1, entity_hash: 'cover-2', mime_type: 'image/jpeg' }]);
    });
    await waitFor(() => expect(container.querySelector('img')?.getAttribute('src')).toContain('cover-2'));
  });

  it('clears the previous view cover immediately and renders the shared broken-file artwork for a broken cover', async () => {
    getCoverHashesMock
      .mockResolvedValueOnce([{ folder_id: 1, entity_hash: 'cover-1', mime_type: 'image/jpeg' }])
      .mockReturnValueOnce(new Promise(() => {}));

    const { container, rerender } = render(
      <SubfolderGrid
        childFolders={[folder(1)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );
    await waitFor(() => expect(container.querySelector('img')?.getAttribute('src')).toContain('cover-1'));

    fireEvent.error(container.querySelector('img')!);
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('[data-broken-thumbnail]')).not.toBeNull();
    expect(container.querySelector('[class*="folderPlaceholder"]')).toBeNull();

    rerender(
      <SubfolderGrid
        childFolders={[folder(2)]}
        targetSize={220}
        totalImageCount={0}
        onOpenFolder={() => {}}
      />,
    );
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('[data-broken-thumbnail]')).toBeNull();
  });

  it('renames a folder inline on the grid card', async () => {
    getCoverHashesMock.mockResolvedValueOnce([]);
    const onRenameFolder = vi.fn();
    const { getByLabelText } = render(
      <SubfolderGrid
        childFolders={[folder(1)]}
        targetSize={220}
        totalImageCount={0}
        renamingNodeId="folder:1"
        onRenameFolder={onRenameFolder}
        onOpenFolder={() => {}}
      />,
    );

    const input = getByLabelText('Rename Folder 1');
    await waitFor(() => expect(getCoverHashesMock).toHaveBeenCalledWith([1]));
    fireEvent.change(input, { target: { value: 'Reference' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onRenameFolder).toHaveBeenCalledWith('folder:1', 'Reference');
  });
});
