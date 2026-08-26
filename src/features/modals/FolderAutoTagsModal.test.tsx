import { MantineProvider } from '@mantine/core';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { foldersController } from '../../controllers/foldersController';
import { tagSelectPortalAtom } from '../../state/portals';
import { FolderAutoTagsModal } from './FolderAutoTagsModal';

describe('FolderAutoTagsModal', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    getDefaultStore().set(tagSelectPortalAtom, { open: false, anchor: null });
  });

  it('uses the subscription tag assignment control and applies picker changes', async () => {
    const setAutoTags = vi.spyOn(foldersController, 'setAutoTags').mockResolvedValue();
    render(
      <MantineProvider>
        <FolderAutoTagsModal
          open
          folderIds={[4, 7]}
          initialTags={[]}
          onClose={vi.fn()}
        />
      </MantineProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add tags' }));
    const portal = getDefaultStore().get(tagSelectPortalAtom);
    expect(portal).toMatchObject({ open: true, selectedTags: [] });
    await act(async () => {
      portal.onApplyTags?.(['creator:alice']);
    });

    await waitFor(() => {
      expect(setAutoTags).toHaveBeenCalledWith(4, ['creator:alice']);
      expect(setAutoTags).toHaveBeenCalledWith(7, ['creator:alice']);
    });
  });
});
