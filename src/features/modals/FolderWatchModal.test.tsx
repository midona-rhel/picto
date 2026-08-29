import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { FolderWatchModal } from './FolderWatchModal';

const analyzeFolderTree = vi.hoisted(() => vi.fn());
vi.mock('../../platform/folderApi', () => ({ analyzeFolderTree }));

describe('FolderWatchModal', () => {
  it('previews how deep watched subfolders will be retained', async () => {
    analyzeFolderTree.mockResolvedValue({
      source_depth: 5, destination_depth: 6, retained_depth: 2, consolidated_levels: 3,
    });

    render(<MantineProvider>
      <FolderWatchModal
        open
        folderId={42}
        initial={{ watchPath: '/tmp/Photos', subfolders: true }}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />
    </MantineProvider>);

    expect(await screen.findByRole('status')).toHaveTextContent('keep the first 2 levels');
    expect(analyzeFolderTree).toHaveBeenCalledWith(expect.objectContaining({
      destination_folder_id: 42,
      include_source_root: false,
    }));
  });
});
