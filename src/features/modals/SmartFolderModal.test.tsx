import { fireEvent, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { SmartFolderModal } from './SmartFolderModal';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { showErrorNotification } from '../../shared/lib/notifications';

vi.mock('../../controllers/smartFoldersController', () => ({
  smartFoldersController: { preview: vi.fn().mockResolvedValue(undefined) },
}));
vi.mock('../../shared/lib/notifications', async (original) => ({
  ...(await original<typeof import('../../shared/lib/notifications')>()),
  showErrorNotification: vi.fn(),
}));

vi.mock('../../platform/tagApi', () => ({
  getTagsById: vi.fn().mockResolvedValue([
    { tag_id: 31, namespace: 'creator', subname: 'huffslove' },
  ]),
  getTagsPaginated: vi.fn().mockResolvedValue({
    tags: [{ tag_id: 31, namespace: 'creator', subname: 'huffslove' }],
    next_cursor: null,
    revision: 1,
  }),
}));

describe('SmartFolderModal', () => {
  it('uses the flat rule editor and the shared compact icon picker', async () => {
    renderWithProviders(
      <SmartFolderModal
        open
        mode="create"
        onClose={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    await screen.findByRole('dialog', { name: 'New Smart Folder' });
    expect(screen.getAllByText('Match')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Remove group' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Remove rule' })).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Add group' }));
    expect(screen.getAllByText('Match')).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'Remove group' })[0]).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: 'Change icon' }));
    await waitFor(() => {
      expect(document.querySelector('[data-icon-picker-presentation="compact"]')).not.toBeNull();
    });
  });

  it('separates smart-folder metadata from predicate editing', async () => {
    const initial = {
      id: 7,
      name: 'Reference',
      icon: null,
      color: null,
      notes: 'Useful images',
      view: {
        filter: {
          kind: 'all' as const,
          value: [{
            kind: 'all' as const,
            value: [{
              kind: 'clause' as const,
              value: { clause: 'tags' as const, tag_ids: [31], mode: 'any' as const },
            }],
          }],
        },
        sort: { field: 'imported_at' as const, direction: 'descending' as const, random_seed: null },
      },
    };

    const { unmount } = renderWithProviders(
      <SmartFolderModal
        open
        mode="edit"
        editor="details"
        initial={initial}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    await screen.findByRole('dialog', { name: 'Edit Smart Folder' });
    expect(screen.getByDisplayValue('Reference')).toBeVisible();
    expect(screen.queryByText('Match')).not.toBeInTheDocument();
    unmount();

    renderWithProviders(
      <SmartFolderModal
        open
        mode="edit"
        editor="rules"
        initial={initial}
        onClose={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    await screen.findByRole('dialog', { name: 'Edit Rules' });
    expect(screen.getByText('Match')).toBeVisible();
    await waitFor(() => {
      expect(screen.getByLabelText('Tags').parentElement?.textContent).toContain('huffslove');
    });
    expect(screen.queryByDisplayValue('Reference')).not.toBeInTheDocument();
  });

  it('caps each smart folder at ten local rules', async () => {
    renderWithProviders(
      <SmartFolderModal
        open
        mode="create"
        onClose={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    await screen.findByRole('dialog', { name: 'New Smart Folder' });
    for (let index = 1; index < 10; index += 1) {
      fireEvent.click(screen.getAllByRole('button', { name: 'Add rule' })[0]);
    }
    expect(screen.getAllByRole('button', { name: 'Add rule' })).toHaveLength(10);
    expect(screen.getAllByRole('button', { name: 'Add rule' }).every((button) => button.hasAttribute('disabled'))).toBe(true);
    expect(screen.getByRole('button', { name: 'Add group' })).toBeDisabled();
  });

  describe('rule preview failures', () => {
    const editing = {
      id: 9,
      name: 'Previewed',
      icon: null,
      color: null,
      notes: null,
      view: {
        filter: { kind: 'all' as const, value: [] },
        sort: { field: 'imported_at' as const, direction: 'descending' as const, random_seed: null },
      },
    };

    it('reports a failed live preview instead of leaving it unhandled', async () => {
      vi.mocked(showErrorNotification).mockClear();
      vi.mocked(smartFoldersController.preview).mockReset().mockRejectedValue(new Error('disk full'));
      renderWithProviders(
        <SmartFolderModal open mode="edit" editor="rules" initial={editing} onClose={vi.fn()} onSave={vi.fn()} />,
      );
      await waitFor(() => {
        expect(showErrorNotification).toHaveBeenCalledWith({
          title: 'Could not preview smart folder rules',
          message: 'disk full',
        });
      });
    });

    it('stays open when Cancel cannot restore the original rules, and closes once it can', async () => {
      vi.mocked(showErrorNotification).mockClear();
      const preview = vi.mocked(smartFoldersController.preview).mockReset().mockResolvedValue(undefined);
      const onClose = vi.fn();
      renderWithProviders(
        <SmartFolderModal open mode="edit" editor="rules" initial={editing} onClose={onClose} onSave={vi.fn()} />,
      );
      await screen.findByRole('dialog', { name: 'Edit Rules' });
      await waitFor(() => expect(preview).toHaveBeenCalled());

      preview.mockRejectedValueOnce(new Error('locked'));
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      await waitFor(() => {
        expect(showErrorNotification).toHaveBeenCalledWith({
          title: 'Could not restore smart folder rules',
          message: 'locked',
        });
      });
      expect(onClose).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
      expect(preview).toHaveBeenLastCalledWith(9, expect.objectContaining({ view: editing.view }));
    });
  });
});
