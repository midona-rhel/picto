import { fireEvent, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { SmartFolderModal } from './SmartFolderModal';

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
});
