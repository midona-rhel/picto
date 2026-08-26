import { fireEvent, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { SmartFolderModal } from './SmartFolderModal';

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
      predicate: { groups: [{ match_mode: 'all' as const, negate: false, rules: [{ field: 'tags', op: 'include_any', values: ['artist:test'] }] }] },
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
    expect(screen.queryByDisplayValue('Reference')).not.toBeInTheDocument();
  });
});
