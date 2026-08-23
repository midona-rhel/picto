import { render, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../../shared/types/canonical';

const { getCoverHashesMock } = vi.hoisted(() => ({ getCoverHashesMock: vi.fn() }));

vi.mock('../../controllers/foldersController', () => ({
  foldersController: { getCoverHashes: getCoverHashesMock },
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
  it('loads every child cover through one batch request', async () => {
    getCoverHashesMock.mockResolvedValueOnce([
      { folder_id: 1, entity_hash: 'cover-1' },
      { folder_id: 2, entity_hash: null },
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
});
