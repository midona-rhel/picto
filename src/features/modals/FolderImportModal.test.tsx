import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { FolderImportModal } from './FolderImportModal';

const analyzeFolderTree = vi.hoisted(() => vi.fn());
vi.mock('../../platform/folderApi', () => ({ analyzeFolderTree }));

describe('FolderImportModal', () => {
  it('defaults to recursive import while omitting folders without media', async () => {
    analyzeFolderTree.mockResolvedValue({
      source_depth: 2, destination_depth: 0, retained_depth: 2, consolidated_levels: 0,
    });
    const onImport = vi.fn();
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={onImport}
          targetFolderId={null}
        />
      </MantineProvider>);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Import' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    expect(onImport).toHaveBeenCalledWith({
      preserveStructure: true,
      includeSubfolders: true,
      includeFoldersWithoutMedia: false,
      watchSourceFolder: false,
    });
  });

  it('exposes every folder discovery policy explicitly', async () => {
    analyzeFolderTree.mockResolvedValue({
      source_depth: 2, destination_depth: 0, retained_depth: 2, consolidated_levels: 0,
    });
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={vi.fn()}
          targetFolderId={null}
        />
      </MantineProvider>);

    expect(screen.getByText('Preserve folder structure')).toBeInTheDocument();
    expect(screen.getByText('Include subfolders')).toBeInTheDocument();
    expect(screen.queryByText('Extract ZIP archives')).not.toBeInTheDocument();
    expect(screen.getByText('Include folders without media')).toBeInTheDocument();
    expect(screen.getByText('Watch this folder')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Import' })).toBeEnabled());
  });

  it('keeps structure when watching the imported folder', async () => {
    analyzeFolderTree.mockResolvedValue({
      source_depth: 2, destination_depth: 0, retained_depth: 2, consolidated_levels: 0,
    });
    const onImport = vi.fn();
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={onImport}
          targetFolderId={null}
        />
      </MantineProvider>);

    const switches = screen.getAllByRole('switch');
    fireEvent.click(switches[0]);
    fireEvent.click(switches[3]);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Import' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    expect(onImport).toHaveBeenCalledWith(expect.objectContaining({
      preserveStructure: true,
      watchSourceFolder: true,
    }));
  });

  it('explains deep-folder consolidation without implying media loss', async () => {
    analyzeFolderTree.mockResolvedValue({
      source_depth: 9, destination_depth: 5, retained_depth: 3, consolidated_levels: 6,
    });
    render(<MantineProvider>
      <FolderImportModal
        open
        path="/tmp/Photos"
        targetFolderId={12}
        onClose={vi.fn()}
        onImport={vi.fn()}
      />
    </MantineProvider>);

    expect(await screen.findByRole('status')).toHaveTextContent('keep the first 3 levels');
    expect(screen.getByRole('status')).toHaveTextContent('no media is skipped because of folder depth');
  });
});
