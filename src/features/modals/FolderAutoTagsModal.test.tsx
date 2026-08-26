import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { foldersController } from '../../controllers/foldersController';
import { FolderAutoTagsModal } from './FolderAutoTagsModal';

describe('FolderAutoTagsModal', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  const renderModal = (modal: ReactNode) => render(
    <MantineProvider>{modal}</MantineProvider>,
  );

  it('keeps edits local until Save and writes the final tags once', async () => {
    const setAutoTags = vi.spyOn(foldersController, 'setAutoTags').mockResolvedValue();
    const onClose = vi.fn();
    renderModal(
      <FolderAutoTagsModal
        open
        folderIds={[4]}
        folderName="References"
        initialTags={['creator:alice']}
        onClose={onClose}
      />,
    );

    const input = screen.getByRole('textbox', { name: 'Auto Tags' });
    fireEvent.change(input, { target: { value: 'rating:safe' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(setAutoTags).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(setAutoTags).toHaveBeenCalledWith(4, [
      'creator:alice',
      'rating:safe',
    ]));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('discards the draft on Cancel', () => {
    const setAutoTags = vi.spyOn(foldersController, 'setAutoTags').mockResolvedValue();
    const onClose = vi.fn();
    renderModal(
      <FolderAutoTagsModal
        open
        folderIds={[4]}
        folderName="References"
        initialTags={[]}
        onClose={onClose}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(setAutoTags).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
