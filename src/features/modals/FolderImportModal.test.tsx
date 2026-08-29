import { fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { describe, expect, it, vi } from 'vitest';
import { FolderImportModal } from './FolderImportModal';

describe('FolderImportModal', () => {
  it('defaults to recursive archive expansion while omitting folders without media', () => {
    const onImport = vi.fn();
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={onImport}
        />
      </MantineProvider>);

    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    expect(onImport).toHaveBeenCalledWith({
      preserveStructure: true,
      includeSubfolders: true,
      expandArchives: true,
      includeFoldersWithoutMedia: false,
      watchSourceFolder: false,
    });
  });

  it('exposes every folder discovery policy explicitly', () => {
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={vi.fn()}
        />
      </MantineProvider>);

    expect(screen.getByText('Preserve folder structure')).toBeInTheDocument();
    expect(screen.getByText('Include subfolders')).toBeInTheDocument();
    expect(screen.getByText('Extract ZIP archives')).toBeInTheDocument();
    expect(screen.getByText('Include folders without media')).toBeInTheDocument();
    expect(screen.getByText('Watch this folder')).toBeInTheDocument();
  });

  it('keeps structure when watching the imported folder', () => {
    const onImport = vi.fn();
    render(<MantineProvider>
        <FolderImportModal
          open
          path="/tmp/Photos"
          onClose={vi.fn()}
          onImport={onImport}
        />
      </MantineProvider>);

    const switches = screen.getAllByRole('switch');
    fireEvent.click(switches[0]);
    fireEvent.click(switches[4]);
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    expect(onImport).toHaveBeenCalledWith(expect.objectContaining({
      preserveStructure: true,
      watchSourceFolder: true,
    }));
  });
});
